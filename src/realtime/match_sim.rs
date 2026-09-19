use std::collections::{HashMap, HashSet, VecDeque};

use rand::Rng;
use uuid::Uuid;

use super::aoi;
use super::bots::{self, BotMind};
use super::generals_roster::{self, faction_ok};
use super::grid::{SpatialGrid, MAX_ENTITY_RADIUS, MAX_UNIT_RADIUS};
use super::protocol::{
    BuildableInfo, EntityView, MatchPlayerStats, MatchSnapshot, MountainView, PondView,
    ResourcesView, ScoreboardRow, ShotEvent, TrainableInfo,
};

pub const TICK_HZ: u32 = 20;
pub const BROADCAST_EVERY: u32 = 2; // 10 Hz to clients
/// Smallest playable edge (2 commanders).
pub const MIN_MAP_SIZE: u16 = 96;
/// Cap for auto-scaled arenas.
pub const MAX_MAP_SIZE: u16 = 2048;

/// Auto map edge from commander count — large gaps between bases.
/// Map size still clamps to MAX_MAP_SIZE; commander count itself is uncapped.
pub fn map_size_for_players(max_players: u16) -> u16 {
    let n = max_players.max(2) as f32;
    let spacing = 112.0;
    let size = (spacing * n.sqrt() + 48.0).round() as i32;
    let size = ((size + 1) / 2) * 2; // even
    (size as u16).clamp(MIN_MAP_SIZE, MAX_MAP_SIZE)
}

/// Minimum nearest-HQ clearance we want when carving an overflow base.
fn min_spawn_clearance(commander_count: u16) -> f32 {
    let n = commander_count.max(2) as f32;
    // Softer than the design 112 spacing — still roomy enough for a starter ring.
    (112.0 * (2.0 / n.sqrt()).sqrt()).clamp(48.0, 96.0)
}

/// Total living+queued units with a single home HQ — room to grow over a long match.
pub const HOME_UNIT_BUDGET: usize = 36;
/// Extra units per captured colony HQ (= half of home → x + x/2 + x/2 …).
pub const COLONY_UNIT_BUDGET: usize = HOME_UNIT_BUDGET / 2;
/// Non-HQ structures at home (econ + 5 Patriots + expansion headroom).
pub const HOME_BUILDING_BUDGET: usize = 14;
/// Extra structures unlocked per captured colony HQ (= half of home).
pub const COLONY_BUILDING_BUDGET: usize = HOME_BUILDING_BUDGET / 2;
/// Wipe / claim radius around a fallen HQ (city footprint).
pub const CITY_CLAIM_RADIUS: f32 = 14.0;

/// Lifetime combat / economy counters for the post-match report.
#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub buildings_built: u32,
    pub buildings_destroyed: u32,
    pub buildings_lost: u32,
    pub infantry_killed: u32,
    pub tanks_killed: u32,
    pub aircraft_killed: u32,
    pub infantry_produced: u32,
    pub tanks_produced: u32,
    pub aircraft_produced: u32,
    pub units_lost: u32,
    pub gold_earned: u32,
    pub power_earned: u32,
    pub bases_captured: u32,
}

fn unit_is_vehicle(kind: &str) -> bool {
    let k = kind;
    k.contains("tank")
        || k.contains("mlrs")
        || k.contains("humvee")
        || k.contains("technical")
        || k.contains("buggy")
        || k.contains("scorpion")
        || k.contains("tomahawk")
        || k.contains("scud")
        || k.contains("inferno")
        || k.contains("crawler")
        || k.contains("battle_bus")
        || k.contains("bomb_truck")
        || k.contains("radar_van")
        || k.contains("quad")
        || k.contains("paladin")
        || k.contains("marauder")
        || k.contains("overlord")
        || k.contains("microwave")
}

fn credit_unit_kill(stats: &mut PlayerStats, kind: &str) {
    if is_air_kind(kind) {
        stats.aircraft_killed = stats.aircraft_killed.saturating_add(1);
    } else if unit_is_vehicle(kind) {
        stats.tanks_killed = stats.tanks_killed.saturating_add(1);
    } else {
        stats.infantry_killed = stats.infantry_killed.saturating_add(1);
    }
}

fn credit_unit_produced(stats: &mut PlayerStats, kind: &str) {
    if is_air_kind(kind) {
        stats.aircraft_produced = stats.aircraft_produced.saturating_add(1);
    } else if unit_is_vehicle(kind) {
        stats.tanks_produced = stats.tanks_produced.saturating_add(1);
    } else {
        stats.infantry_produced = stats.infantry_produced.saturating_add(1);
    }
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub user_id: Uuid,
    pub name: String,
    pub faction: String,
    pub team: u8,
    pub flag: Option<String>,
    /// Three-band identity colors (body / stripe / accent) — shared across many players via schemes.
    pub colors: [u32; 3],
    pub resources: Resources,
    pub focus: [f32; 2],
    pub alive: bool,
    pub connected: bool,
    /// Entity ids last acknowledged in this player's vision (enter/leave sync).
    pub aoi_known: HashSet<Uuid>,
    /// Permanent explored shroud (Generals-style).
    pub explored: aoi::ExploredMap,
    /// Computer commander — `None` for human players.
    pub bot: Option<BotMind>,
    /// Dev cheat: this commander sees the whole map (M key). Others still fogged.
    pub debug_omniscient: bool,
    /// Captured colony count (extra HQs beyond the first). Drives army budget.
    pub colonies: u32,
    /// Original Command Center — H-key / scoreboard prefer this over later colonies.
    pub home_hq: Option<Uuid>,
    pub stats: PlayerStats,
}

impl PlayerState {
    pub fn label(&self) -> String {
        if self.bot.is_some() {
            format!("{} [BOT]", self.name)
        } else {
            format!("{} ({})", self.name, self.faction)
        }
    }

    pub fn is_bot(&self) -> bool {
        self.bot.is_some()
    }

    pub fn is(&self, id: Uuid) -> bool {
        self.user_id == id
    }
}

/// Distinct tricolor schemes so ownership stays readable even with many players.
/// Reuses by wrap-around past the table length (100 players → still recognizable bands).
pub fn color_scheme_for_slot(slot: usize) -> [u32; 3] {
    const SCHEMES: &[[u32; 3]] = &[
        [0xc62828, 0xf5f5f5, 0x1565c0], // red · white · blue
        [0xf9a825, 0x212121, 0x2e7d32], // yellow · black · green
        [0x6a1b9a, 0xff6f00, 0x00838f], // purple · orange · teal
        [0xffffff, 0xc62828, 0x212121], // white · red · black
        [0x1565c0, 0xf9a825, 0xffffff], // blue · yellow · white
        [0x2e7d32, 0xffffff, 0xc62828], // green · white · red
        [0xff6f00, 0x1565c0, 0x212121], // orange · blue · black
        [0x00838f, 0xf5f5f5, 0x6a1b9a], // teal · white · purple
        [0xad1457, 0x81d4fa, 0x33691e], // magenta · lightblue · darkgreen
        [0x4e342e, 0xffeb3b, 0xd32f2f], // brown · yellow · red
        [0x1a237e, 0xeeff41, 0xe65100], // navy · lime · orange
        [0x00695c, 0xffcdd2, 0x311b92], // green · pink · indigo
        [0xbf360c, 0xb3e5fc, 0x263238], // deep orange · sky · charcoal
        [0x4527a0, 0xa5d6a7, 0xff8f00], // violet · mint · amber
        [0x37474f, 0xff1744, 0x00e5ff], // slate · neon red · cyan
        [0xfafafa, 0x00c853, 0x0d47a1], // white · green · blue
        [0xffd600, 0x880e4f, 0x00bfa5], // gold · wine · aqua
        [0x3e2723, 0xffffff, 0x1565c0], // brown · white · blue
        [0xd50000, 0x00e676, 0x212121], // red · lime · black
        [0x0277bd, 0xffecb3, 0x4a148c], // blue · cream · purple
        [0x558b2f, 0xff5252, 0xeceff1], // olive · coral · silver
        [0x5d4037, 0x40c4ff, 0xffab00], // brown · azure · amber
        [0x7b1fa2, 0xc8e6c9, 0xb71c1c], // purple · pale green · red
        [0x01579b, 0xfff176, 0x1b5e20], // blue · pale yellow · green
    ];
    SCHEMES[slot % SCHEMES.len()]
}

#[derive(Clone, Debug)]
pub struct Resources {
    /// Single spendable currency (was supplies/fuel/munitions).
    pub gold: i32,
    /// Available power from finished generators (+ HQ base).
    pub power: i32,
    /// Power reserved by finished consumer buildings.
    pub power_used: i32,
}

impl Resources {
    pub fn starter() -> Self {
        Self {
            gold: 4_000,
            // HQ grants base power on spawn — start dark until then.
            power: 0,
            power_used: 0,
        }
    }

    pub fn has_power(&self) -> bool {
        self.power_used <= self.power
    }

    pub fn view(&self) -> ResourcesView {
        ResourcesView {
            gold: self.gold,
            power: self.power,
            power_used: self.power_used,
            units: 0,
            units_cap: 0,
            buildings: 0,
            buildings_cap: 0,
            bases: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub id: Uuid,
    pub kind: String,
    pub owner: Uuid,
    pub team: u8,
    pub x: f32,
    pub y: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub building: bool,
    pub unit: bool,
    pub flag: Option<String>,
    pub build_remaining_ms: u32,
    pub train_queue: VecDeque<TrainJob>,
    pub target: Option<Uuid>,
    pub move_to: Option<(f32, f32)>,
    pub speed: f32,
    pub damage: f32,
    pub range: f32,
    pub attack_cooldown_ms: u32,
    /// Rounds left in the current rifle magazine (0 = unused / not a rifle).
    pub mag_ammo: u8,
    pub dirty: bool,
    /// Frames with little/no progress toward the goal.
    pub stuck_frames: u16,
    /// Temporary escape waypoint when pathing is jammed.
    pub detour: Option<(f32, f32)>,
    /// Remaining ticks to honor the detour before forcing a re-aim at the true goal.
    pub detour_ttl: u16,
    /// Last escape heading (radians) — avoid picking the same jam twice.
    pub last_escape_ang: f32,
    /// Infantry drops in a firefight (smaller silhouette).
    pub prone: bool,
    /// Stay down until this tick so they don't pop up between shots.
    pub prone_until_tick: u64,
    /// Tank barrel world yaw (atan2(dx, dy)); slewed before the gun fires.
    pub aim_yaw: f32,
    /// Roof MG cyclic; independent of the main gun reload.
    pub mg_cooldown_ms: u32,
    /// Last commander who damaged this entity (for HQ capture attribution).
    pub last_hit_by: Option<Uuid>,
    /// Building disabled by a hacker until this tick (production / income / guns freeze).
    pub hacked_until_tick: u64,
    /// Specialist ability recharge (spy sabotage / hack / revolt).
    pub ability_cooldown_ms: u32,
}

#[derive(Clone, Debug)]
pub struct TrainJob {
    pub unit: String,
    pub remaining_ms: u32,
}

#[derive(Clone, Debug)]
pub struct BuildDef {
    pub kind: &'static str,
    pub name: &'static str,
    /// `"usa"` | `"china"` | `"gla"` | `"any"`
    pub faction: &'static str,
    pub cost_gold: i32,
    pub build_ms: u32,
    pub power: i32,
    pub hp: f32,
}

#[derive(Clone, Debug)]
pub struct UnitDef {
    pub unit: &'static str,
    pub name: &'static str,
    pub faction: &'static str,
    pub from_building: &'static str,
    pub cost_gold: i32,
    pub train_ms: u32,
    pub hp: f32,
    pub damage: f32,
    pub speed: f32,
    pub range: f32,
    /// Time between shots (realistic reload / burst spacing).
    pub attack_ms: u32,
}

#[inline]
fn is_power_producer(kind: &str) -> bool {
    matches!(kind, "power_plant" | "nuclear_reactor")
}

pub fn buildables() -> &'static [BuildDef] {
    generals_roster::buildables()
}

pub fn trainables() -> &'static [UnitDef] {
    generals_roster::trainables()
}

fn attack_cooldown_for(kind: &str) -> u32 {
    match kind {
        // Patriot / Stinger — guided missile salvo cadence
        "turret" | "stinger_site" | "particle_cannon" => 1_850,
        // China Bunker / GLA Tunnel — MG nest
        "bunker" | "tunnel_network" => 90,
        // Gattling Cannon — continuous spin-up fire
        "gatling_cannon" => 100,
        // Fire Base 155mm howitzer — slow artillery
        "firebase" => 3_200,
        _ => trainables()
            .iter()
            .find(|u| u.unit == kind)
            .map(|u| u.attack_ms)
            .unwrap_or(1_000),
    }
}

const RIFLE_MAG: u8 = 30;
const RIFLE_RELOAD_MS: u32 = 5_000;
/// MIM-104 class engagement bubble — long-range guided intercept (early USA tuning).
const PATRIOT_RANGE: f32 = 17.0;
const PATRIOT_DAMAGE: f32 = 780.0;
/// China Gattling: shreds soft targets, weak vs heavy armor. Range ~225 ≈ 8.0.
const GATLING_RANGE: f32 = 8.0;
const GATLING_DAMAGE: f32 = 42.0;
/// ZH Fire Base howitzer — outranges Patriots.
const FIREBASE_RANGE: f32 = 12.5;
const FIREBASE_DAMAGE: f32 = 560.0;
/// Pillbox / tunnel MG — short anti-infantry.
const BUNKER_RANGE: f32 = 4.8;
const BUNKER_DAMAGE: f32 = 72.0;
const BUNKER_MAG: u8 = 40;
const BUNKER_RELOAD_MS: u32 = 2_200;
const BUNKER_SLEW_RATE: f32 = 1.85;
const BUNKER_SCAN_RATE: f32 = 0.75;
const BUNKER_AIM_ALIGN: f32 = 0.12;

/// F-16 strike: gold per takeoff (not per bomb). Rare, decisive sorties.
const F16_SORTIE_GOLD: i32 = 6_500;
/// One bomb per sortie — drop and RTB immediately.
const F16_BOMBS: u8 = 1;
const F16_REARM_MS: u32 = 80_000;

#[inline]
fn is_f16_kind(kind: &str) -> bool {
    kind == "f16" || kind.contains("f16")
}

#[inline]
fn is_vehicle_kind(kind: &str) -> bool {
    kind.contains("tank")
        || kind.contains("mlrs")
        || kind.contains("humvee")
        || kind.contains("technical")
        || kind.contains("buggy")
        || kind.contains("crawler")
        || kind.contains("cannon")
        || kind.contains("overlord")
        || kind.contains("tomahawk")
        || kind.contains("microwave")
        || kind.contains("inferno")
        || kind.contains("scud")
        || kind.contains("bomb_truck")
        || kind.contains("radar_van")
        || kind.contains("outpost")
        || kind.contains("ecm")
        || kind.contains("bus")
        || kind.contains("raptor")
        || kind.contains("mig")
        || kind.contains("f16")
        || kind.contains("comanche")
        || kind.contains("helix")
        || kind.contains("chinook")
}

#[inline]
fn is_air_kind(kind: &str) -> bool {
    kind.contains("raptor")
        || kind.contains("mig")
        || kind.contains("f16")
        || kind.contains("comanche")
        || kind.contains("helix")
        || kind.contains("chinook")
}

#[inline]
fn is_air_bomb_kind(kind: &str) -> bool {
    kind.contains("raptor")
        || kind.contains("mig")
        || kind.contains("f16")
        || kind.contains("comanche")
        || kind.contains("helix")
}

#[inline]
fn is_soft_unit(kind: &str) -> bool {
    !is_vehicle_kind(kind)
}

fn is_rifle_infantry(kind: &str) -> bool {
    matches!(
        kind,
        "ranger"
            | "spy"
            | "red_guard"
            | "rebel"
            | "pathfinder"
            | "colonel_burton"
            | "hacker"
            | "hijacker"
            | "terrorist"
            | "black_lotus"
    )
}

/// Spy / hacker / terrorist — fully stealthed vs enemies (no FOW, no auto-target).
/// Unarmed: no gun combat. Still die to splash / crush if caught in the blast.
#[inline]
fn is_stealth_specialist(kind: &str) -> bool {
    matches!(kind, "spy" | "hacker" | "terrorist")
}

/// Match client `atan2(dx, dz)` — yaw 0 faces +Y / +Z.
fn world_aim_yaw(dx: f32, dy: f32) -> f32 {
    dx.atan2(dy)
}

fn shortest_angle(from: f32, to: f32) -> f32 {
    let mut d = to - from;
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    while d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

/// M1A1 turret traverse ≈ 40°/s → 0.70 rad/s; client uses ~1.15 for readable slew.
const TANK_TURRET_RATE: f32 = 1.05;
const TANK_AIM_ALIGN: f32 = 0.06;
/// Coax M240 — shorter than main gun, still beyond rifle.
const TANK_MG_RANGE: f32 = 6.5;
const TANK_MG_COOLDOWN_MS: u32 = 100;
/// M269 LLM traverse ≈ 5°/s real — kept slightly faster for play (~9°/s).
const MLRS_POD_RATE: f32 = 0.16;
const MLRS_AIM_ALIGN: f32 = 0.08;
/// Rockets in one ripple before the long reload (one LPC bay = 6).
const MLRS_SALVO: u8 = 6;
/// Gap between rockets in a ripple (~real M270 spacing, shortened for game pace).
const MLRS_RIPPLE_MS: u32 = 160;
const MLRS_RELOAD_MS: u32 = 12_000;
/// Patriot launcher slew — faster track than before (still waits for bearing).
const PATRIOT_SLEW_RATE: f32 = 1.25;
const PATRIOT_AIM_ALIGN: f32 = 0.07;
/// Idle search sweep when no contact.
const PATRIOT_SCAN_RATE: f32 = 0.65;

/// Infantry: a few rifle hits drop a soldier. Tank HE in the burst radius is lethal.
/// Hierarchy mirrors real roles: rifle ≪ mortar ≪ MG nest (soft) ≪ tank ≪ MLRS AoE ≪ Patriot.
fn hit_damage(attacker_kind: &str, target: &Entity, base: f32) -> f32 {
    let infantry = target.unit && is_soft_unit(&target.kind);
    let armored = is_vehicle_kind(&target.kind);
    let soft_vehicle = target.kind.contains("mlrs");
    if infantry && attacker_kind.contains("tank") {
        10_000.0
    } else if infantry && (attacker_kind == "turret" || attacker_kind == "stinger_site") {
        // Generals Patriot/Stinger — poor vs infantry (missiles overshoot soft targets).
        (base * 0.28).max(55.0)
    } else if infantry && attacker_kind == "firebase" {
        // 155mm HE — deadly to soft targets in the blast.
        (base * 1.15).max(280.0)
    } else if infantry && attacker_kind == "gatling_cannon" {
        (base * 1.35).max(48.0)
    } else if infantry && attacker_kind.contains("mlrs") {
        // Rocket HE / DPICM — lethal in the seat.
        10_000.0
    } else if infantry && is_f16_kind(attacker_kind) {
        10_000.0
    } else if infantry && attacker_kind.contains("mortar") {
        (base * 0.95).max(250.0)
    } else if infantry && (attacker_kind == "bunker" || attacker_kind == "tunnel_network") {
        (base * 1.25).max(58.0)
    } else if armored && (attacker_kind == "bunker" || attacker_kind == "tunnel_network") {
        // MG vs armor — sparks only.
        (base * 0.08).max(3.0)
    } else if armored && attacker_kind == "gatling_cannon" {
        // Gattling vs heavy armor — weak (Generals).
        (base * 0.18).max(8.0)
    } else if armored && attacker_kind == "firebase" {
        // Howitzer HE vs AFV — solid.
        if soft_vehicle {
            (base * 1.2).max(base)
        } else {
            (base * 0.85).max(base * 0.7)
        }
    } else if armored && is_rifle_infantry(attacker_kind) {
        // 5.56/7.62 vs AFV — negligible.
        (base * 0.06).max(2.0)
    } else if armored && (attacker_kind == "turret" || attacker_kind == "stinger_site") {
        // Guided hit — brutal vs soft launchers, heavy punch vs MBT.
        if soft_vehicle {
            (base * 1.45).max(base)
        } else {
            (base * 1.05).max(base)
        }
    } else if armored && attacker_kind.contains("mlrs") {
        // Saturation HE vs armor — mission-kills soft AFVs faster than MBTs.
        if soft_vehicle {
            (base * 0.75).max(200.0)
        } else {
            (base * 0.42).max(140.0)
        }
    } else if armored && is_f16_kind(attacker_kind) {
        // Mk84 / JDAM — catastrophic to AFVs under the seat; soft launchers erased.
        if soft_vehicle {
            (base * 1.35).max(base)
        } else {
            (base * 1.05).max(base)
        }
    } else if armored && attacker_kind.contains("mortar") {
        (base * 0.22).max(55.0)
    } else if armored
        && (attacker_kind.contains("abrams")
            || attacker_kind.contains("paladin")
            || attacker_kind.contains("marauder")
            || attacker_kind.contains("overlord"))
    {
        // DU / heavy APFSDS — designed to crack peer armor.
        if soft_vehicle {
            (base * 2.4).max(base)
        } else {
            (base * 1.28).max(base)
        }
    } else if armored && attacker_kind.contains("tank") {
        // M1A1 M829 APFSDS — designed to defeat peer armor; soft AFVs catastrophic.
        if soft_vehicle {
            (base * 2.3).max(base)
        } else {
            (base * 1.22).max(base)
        }
    } else if target.building
        && (attacker_kind.contains("abrams")
            || attacker_kind.contains("paladin")
            || attacker_kind.contains("marauder")
            || attacker_kind.contains("overlord")
            || attacker_kind.contains("tank"))
    {
        (base * 1.12).max(base)
    } else if target.building && (attacker_kind == "turret" || attacker_kind == "stinger_site") {
        (base * 0.75).max(280.0)
    } else if target.building && attacker_kind == "firebase" {
        (base * 1.05).max(base)
    } else if target.building && attacker_kind.contains("mlrs") {
        (base * 1.1).max(base)
    } else if target.building && is_f16_kind(attacker_kind) {
        (base * 0.95).max(base * 0.85)
    } else {
        base
    }
}

/// How much of the target is actually visible from this firing line.
#[derive(Clone, Copy)]
struct ShotCover {
    /// Solid wall / hull fully between shooter and target — no shot.
    blocked: bool,
    /// 1 = open field; ~0.2 = peeking a corner. Multiplies hit chance.
    exposure: f32,
}

/// Combat hit probability — much easier up close, hard at the edge of range.
/// Inverse-square falloff like real aiming: a few metres away is a different fight
/// than shooting across the weapon's max distance.
fn shot_hit_chance(
    attacker_kind: &str,
    target: &Entity,
    dist: f32,
    range: f32,
    exposure: f32,
) -> f32 {
    let range = range.max(0.05);
    // Distance where hit chance has dropped to ~50% of point-blank.
    let d0 = if attacker_kind.contains("tank") {
        // M1A1 ballistic computer + laser RF — stays accurate farther out.
        range * 0.55
    } else if attacker_kind.contains("mlrs") {
        range * 0.52
    } else if attacker_kind == "bunker" {
        range * 0.40
    } else if attacker_kind.contains("mortar") {
        range * 0.50
    } else if attacker_kind.contains("missile") || attacker_kind == "turret" {
        // Guided: stays lethal farther out.
        range * 0.62
    } else {
        range * 0.30
    };
    let dist_mul = 1.0 / (1.0 + (dist / d0).powi(2));

    // Point-blank connect rate (before size / cover / prone).
    let weapon_near = if attacker_kind.contains("tank") {
        0.90
    } else if attacker_kind == "turret" || attacker_kind.contains("missile") {
        0.92
    } else if attacker_kind.contains("mlrs") {
        0.70
    } else if attacker_kind == "bunker" {
        0.72
    } else if attacker_kind.contains("mortar") {
        0.70
    } else {
        0.78
    };

    let size_mul = if target.building {
        1.55
    } else if is_vehicle_kind(&target.kind) {
        1.40
    } else {
        1.12
    };

    let vis = exposure.clamp(0.12, 1.35);
    let prone_mul = if target.prone && target.unit && is_soft_unit(&target.kind) {
        0.55
    } else {
        1.0
    };
    (weapon_near * dist_mul * size_mul * vis * prone_mul).clamp(0.012, 0.90)
}

/// Closest-point distance from circle center to segment A→B, plus t along the segment.
fn segment_point_gap(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> (f32, f32) {
    let abx = bx - ax;
    let aby = by - ay;
    let ab2 = abx * abx + aby * aby;
    if ab2 < 1e-8 {
        let dx = cx - ax;
        let dy = cy - ay;
        return ((dx * dx + dy * dy).sqrt(), 0.0);
    }
    let t = ((cx - ax) * abx + (cy - ay) * aby) / ab2;
    let tc = t.clamp(0.0, 1.0);
    let px = ax + tc * abx;
    let py = ay + tc * aby;
    let dx = cx - px;
    let dy = cy - py;
    ((dx * dx + dy * dy).sqrt(), tc)
}

fn occluder_radius(entity: &Entity) -> Option<f32> {
    if entity.building {
        Some(building_radius(&entity.kind) * 0.92)
    } else if entity.unit && is_vehicle_kind(&entity.kind) {
        Some(unit_radius(&entity.kind) * 1.15)
    } else {
        None
    }
}

/// Scatter impact for a miss so tracers fly wide of the target.
fn miss_impact(rng: &mut impl Rng, target: &Entity, from_x: f32, from_y: f32) -> (f32, f32) {
    let dx = target.x - from_x;
    let dy = target.y - from_y;
    let dist = (dx * dx + dy * dy).sqrt().max(0.01);
    let ux = dx / dist;
    let uy = dy / dist;
    // Perpendicular scatter + over/under-shoot along the line of fire.
    let px = -uy;
    let py = ux;
    let lateral = if target.building {
        0.9 + rng.gen_range(0.0..1.0) * 1.6
    } else if is_vehicle_kind(&target.kind) {
        0.45 + rng.gen_range(0.0..1.0) * 0.95
    } else {
        0.22 + rng.gen_range(0.0..1.0) * 0.85
    };
    let side = if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
    let along = (rng.gen_range(0.0..1.0) - 0.35) * (if target.building { 1.4 } else { 0.9 });
    (
        target.x + px * lateral * side + ux * along,
        target.y + py * lateral * side + uy * along,
    )
}

/// Spread group move orders so units don't all fight for one exact point.
fn formation_slot(index: usize, count: usize, radius: f32) -> (f32, f32) {
    if count <= 1 || index == 0 {
        return (0.0, 0.0);
    }
    let spacing = (radius * 2.4 + 0.12).max(0.22);
    // Golden-angle spiral around the click point.
    const GOLDEN: f32 = 2.399_963;
    let r = spacing * (index as f32).sqrt();
    let ang = index as f32 * GOLDEN;
    (ang.cos() * r, ang.sin() * r)
}

/// Dense opening-army pack (golden-angle spiral).
fn tight_pack_slot(index: usize, radius: f32) -> (f32, f32) {
    if index == 0 {
        return (0.0, 0.0);
    }
    let spacing = (radius * 2.05 + 0.02).max(0.08);
    const GOLDEN: f32 = 2.399_963;
    let r = spacing * (index as f32).sqrt();
    let ang = index as f32 * GOLDEN;
    (ang.cos() * r, ang.sin() * r)
}

/// Soft arrival: close enough to stop even if the exact point is occupied.
fn move_arrive_radius(self_r: f32) -> f32 {
    (self_r * 2.2 + 0.04).clamp(0.06, 0.18)
}

/// Client `BUILDING_MODELS[].target` — max visual dimension after fit.
fn building_visual_size(kind: &str) -> f32 {
    match kind {
        "hq" => 2.15,
        "war_factory" | "arms_dealer" => 2.1,
        "barracks" => 1.35,
        "power_plant" | "nuclear_reactor" | "supply" | "supply_stash" => 1.7,
        "airfield" => 2.35,
        "strategy_center" | "propaganda_center" | "palace" | "internet_center" | "black_market" => {
            1.9
        }
        "turret" | "stinger_site" | "gatling_cannon" => 0.55,
        "bunker" | "tunnel_network" | "demo_trap" => 0.34,
        "firebase" => 0.85,
        "radar" => 0.85,
        "particle_cannon" | "nuclear_silo" | "scud_storm" => 2.2,
        _ => 1.35,
    }
}

/// Horizontal collision radius from visual size (not a fat generic circle).
pub fn building_radius(kind: &str) -> f32 {
    // Footprint is roughly square; half-extent ≈ 0.40–0.45 of fitted max dim.
    building_visual_size(kind) * 0.42
}

/// Matches client unit footprint on the ground plane.
pub fn unit_radius(kind: &str) -> f32 {
    if is_air_kind(kind) {
        0.12
    } else if kind.contains("overlord") {
        0.16
    } else if kind.contains("tank") || kind.contains("cannon") || kind.contains("crawler") {
        0.1
    } else if kind.contains("mlrs")
        || kind.contains("tomahawk")
        || kind.contains("inferno")
        || kind.contains("scud")
    {
        0.11
    } else if kind.contains("humvee")
        || kind.contains("technical")
        || kind.contains("buggy")
        || kind.contains("bus")
    {
        0.08
    } else if kind.contains("mortar") || kind.contains("defender") || kind.contains("hunter") || kind.contains("rpg") {
        0.022
    } else {
        0.017
    }
}

fn entity_radius(entity: &Entity) -> f32 {
    if entity.building {
        building_radius(&entity.kind)
    } else if entity.unit {
        unit_radius(&entity.kind)
    } else {
        0.05
    }
}

/// Tiny gap so meshes don't Z-fight when brushing past.
/// Must stay small vs infantry radius (~0.017) or a 50-man blob cannot take a step.
fn collision_pad() -> f32 {
    0.006
}

/// Deterministic ponds from match id — client paints the same discs from snapshot.
fn generate_ponds(match_id: Uuid, map_size: u16) -> Vec<PondView> {
    let mut seed = match_id.as_u128() as u64 ^ ((match_id.as_u128() >> 64) as u64);
    if seed == 0 {
        seed = 0x9e37_79b9_7f4a_7c15;
    }
    let map = map_size as f32;
    let count = 5 + (seed % 4) as usize; // 5–8 lakes (fewer, larger)
    let mut ponds: Vec<PondView> = Vec::with_capacity(count);
    let mut s = seed;
    let mut attempts = 0;
    while ponds.len() < count && attempts < count * 50 {
        attempts += 1;
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(attempts as u64 + 1);
        let x = 14.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 28.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(17);
        let y = 14.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 28.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(31);
        // Mix of ponds and mid-size lakes (~3.5–9.5 wu).
        let r = 3.5 + ((s % 80) as f32) * 0.075;
        // Prefer mid-map lakes; keep clear of west/east spawn bands.
        if x < map * 0.18 || x > map * 0.82 {
            continue;
        }
        let mut overlap = false;
        for p in &ponds {
            let dx = p.x - x;
            let dy = p.y - y;
            let min = p.r + r + 4.5;
            if dx * dx + dy * dy < min * min {
                overlap = true;
                break;
            }
        }
        if overlap {
            continue;
        }
        ponds.push(PondView { x, y, r });
    }
    ponds
}

/// Rocky mountain masses — ground pathing must go around (air flies over).
fn generate_mountains(
    match_id: Uuid,
    map_size: u16,
    ponds: &[PondView],
) -> Vec<MountainView> {
    let mut seed = match_id.as_u128() as u64
        ^ ((match_id.as_u128() >> 64) as u64).wrapping_mul(0xA5A5_5A5A)
        ^ 0xD00D_CAFE_BEEF;
    if seed == 0 {
        seed = 0xC001_D00D;
    }
    let map = map_size as f32;
    let count = 4 + (seed % 4) as usize; // 4–7 ranges
    let mut mountains: Vec<MountainView> = Vec::with_capacity(count);
    let mut s = seed;
    let mut attempts = 0;
    while mountains.len() < count && attempts < count * 60 {
        attempts += 1;
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(attempts as u64 + 7);
        let x = 16.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 32.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(19);
        let y = 16.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 32.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(41);
        // Footprint large enough that units must detour (~4.5–9 wu).
        let r = 4.5 + ((s % 70) as f32) * 0.065;
        if x < map * 0.14 || x > map * 0.86 {
            continue;
        }
        let mut overlap = false;
        for p in ponds {
            let dx = p.x - x;
            let dy = p.y - y;
            let min = p.r + r + 5.0;
            if dx * dx + dy * dy < min * min {
                overlap = true;
                break;
            }
        }
        if overlap {
            continue;
        }
        for m in &mountains {
            let dx = m.x - x;
            let dy = m.y - y;
            let min = m.r + r + 3.5;
            if dx * dx + dy * dy < min * min {
                overlap = true;
                break;
            }
        }
        if overlap {
            continue;
        }
        mountains.push(MountainView { x, y, r });
    }
    mountains
}

pub struct MatchSim {
    pub id: Uuid,
    pub map_size: u16,
    pub ffa: bool,
    pub tick: u64,
    pub players: HashMap<Uuid, PlayerState>,
    pub entities: HashMap<Uuid, Entity>,
    /// Impassable water discs — blocks ground units and building placement.
    pub ponds: Vec<PondView>,
    /// Impassable rock — ground units / buildings must go around.
    pub mountains: Vec<MountainView>,
    /// Spatial hash of `entities` — rebuilt/kept in sync for neighbor queries.
    pub(crate) grid: SpatialGrid,
    pub removed: Vec<Uuid>,
    /// Shots fired since last client broadcast (cleared in clear_frame_flags).
    pub shots: Vec<ShotEvent>,
    pub ended: bool,
    pub winner_team: Option<u8>,
    pub end_reason: String,
    /// Pending stream jobs (build completes etc.) mirrored conceptually to Redis Streams.
    pub stream_jobs: VecDeque<StreamJob>,
}

#[derive(Clone, Debug)]
pub struct StreamJob {
    pub due_tick: u64,
}

impl MatchSim {
    pub fn new(
        id: Uuid,
        map_size: u16,
        ffa: bool,
        roster: Vec<(Uuid, String, String, u8, Option<String>)>,
        target_players: u16,
    ) -> Self {
        let map_size = map_size.clamp(MIN_MAP_SIZE, MAX_MAP_SIZE);
        let ponds = generate_ponds(id, map_size);
        let mountains = generate_mountains(id, map_size, &ponds);
        let mut sim = Self {
            id,
            map_size,
            ffa,
            tick: 0,
            players: HashMap::new(),
            entities: HashMap::new(),
            ponds,
            mountains,
            grid: SpatialGrid::new(),
            removed: Vec::new(),
            shots: Vec::new(),
            ended: false,
            winner_team: None,
            end_reason: String::new(),
            stream_jobs: VecDeque::new(),
        };

        for (user_id, name, faction, team, flag) in roster.into_iter() {
            sim.spawn_commander(
                user_id,
                name,
                faction,
                team,
                flag,
                true,
                None,
            );
        }

        let target = (target_players as usize).max(2);
        bots::seed_opening_bots(&mut sim, target);
        if !sim.ffa {
            sim.rebalance_allied_teams();
        }
        sim
    }

    /// Ally skirmish: random ~50/50 team split (not geographic west/east).
    /// Alone (ffa): each commander is their own team (set at spawn).
    fn rebalance_allied_teams(&mut self) {
        if self.ffa {
            return;
        }
        let mut ids: Vec<Uuid> = self.players.keys().copied().collect();
        if ids.len() < 2 {
            return;
        }
        // Deterministic shuffle from match id so reconnects / mid-join rebalance stay stable.
        let mut seed = self.id.as_u128() as u64
            ^ ((self.id.as_u128() >> 64) as u64)
            ^ (ids.len() as u64).wrapping_mul(0x9e37_79b9);
        if seed == 0 {
            seed = 0xC0FFEE;
        }
        for i in (1..ids.len()).rev() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1);
            let j = (seed as usize) % (i + 1);
            ids.swap(i, j);
        }
        let mid = ids.len().div_ceil(2);
        for (i, id) in ids.iter().enumerate() {
            let team = if i < mid { 0u8 } else { 1u8 };
            if let Some(player) = self.players.get_mut(id) {
                player.team = team;
            }
            for entity in self.entities.values_mut() {
                if entity.owner == *id {
                    entity.team = team;
                    entity.dirty = true;
                }
            }
        }
    }

    /// Ally mid-join / bot seed: join the side with fewer commanders.
    pub(crate) fn pick_allied_team(&self) -> u8 {
        let mut t0 = 0usize;
        let mut t1 = 0usize;
        for p in self.players.values() {
            if p.team == 0 {
                t0 += 1;
            } else if p.team == 1 {
                t1 += 1;
            } else {
                // Odd leftover teams count toward the fuller side sense — treat as t1.
                t1 += 1;
            }
        }
        if t0 <= t1 {
            0
        } else {
            1
        }
    }

    pub(crate) fn spawn_commander(
        &mut self,
        user_id: Uuid,
        name: String,
        faction: String,
        team: u8,
        flag: Option<String>,
        connected: bool,
        bot: Option<BotMind>,
    ) {
        let (x, y) = self.allocate_spawn_xy(team);
        let slot = self.players.len();
        let colors = color_scheme_for_slot(slot);

        self.players.insert(
            user_id,
            PlayerState {
                user_id,
                name,
                faction,
                team,
                flag: flag.clone(),
                colors,
                resources: Resources::starter(),
                focus: [x, y],
                alive: true,
                connected,
                aoi_known: HashSet::new(),
                explored: aoi::ExploredMap::new(self.map_size),
                bot,
                debug_omniscient: false,
                colonies: 0,
                home_hq: None,
                stats: PlayerStats::default(),
            },
        );

        let hq_id = Uuid::new_v4();
        self.put_entity(Entity {
            id: hq_id,
            kind: "hq".into(),
            owner: user_id,
            team,
            x,
            y,
            hp: 12_000.0,
            max_hp: 12_000.0,
            building: true,
            unit: false,
            flag,
            build_remaining_ms: 0,
            train_queue: VecDeque::new(),
            target: None,
            move_to: None,
            speed: 0.0,
            damage: 0.0,
            range: 0.0,
            attack_cooldown_ms: 0,
            mag_ammo: 0,
            dirty: true,
            stuck_frames: 0,
            detour: None,
            detour_ttl: 0,
            last_escape_ang: 0.0,
            prone: false,
            prone_until_tick: 0,
            aim_yaw: 0.0,
            mg_cooldown_ms: 0,
            last_hit_by: None,
            hacked_until_tick: 0,
            ability_cooldown_ms: 0,
        });
        if let Some(player) = self.players.get_mut(&user_id) {
            player.home_hq = Some(hq_id);
        }
        self.spawn_starting_force(user_id, team, x, y);
        self.reveal_vision_for(user_id);
    }

    /// Spread HQs across the whole map — maximize distance to existing bases.
    /// Avoids the old center-spiral which stacked late joins near the middle.
    fn allocate_spawn_xy(&self, _team: u8) -> (f32, f32) {
        let map = self.map_size as f32;
        let margin = (map * 0.07).clamp(14.0, 36.0);
        let hq_positions: Vec<(f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.kind == "hq" && e.hp > 0.0)
            .map(|e| (e.x, e.y))
            .collect();
        let hq_r = building_radius("hq") + 0.5;
        let usable = (map - 2.0 * margin).max(8.0);

        // Sample a dense lattice; pick the cell farthest from every living HQ.
        let steps = ((map / 10.0).clamp(16.0, 56.0)) as i32;
        let mut best = (map * 0.5, map * 0.5);
        let mut best_score = -1.0f32;

        // Match-seeded jitter so grids aren't identical every game.
        let mut spin = self.id.as_u128() as u64 ^ (self.players.len() as u64 * 31);
        spin = spin
            .wrapping_mul(6364136223846793005)
            .wrapping_add(7);
        let jx = ((spin % 1000) as f32 / 1000.0 - 0.5) * (usable / steps as f32) * 0.35;
        spin = spin
            .wrapping_mul(6364136223846793005)
            .wrapping_add(11);
        let jy = ((spin % 1000) as f32 / 1000.0 - 0.5) * (usable / steps as f32) * 0.35;

        for iy in 0..=steps {
            for ix in 0..=steps {
                let x = margin + usable * (ix as f32 / steps as f32) + jx;
                let y = margin + usable * (iy as f32 / steps as f32) + jy;
                let fx = x.clamp(margin, map - margin).floor() + 0.5;
                let fy = y.clamp(margin, map - margin).floor() + 0.5;
                if self.ground_blocks(fx, fy, hq_r) {
                    continue;
                }
                let score = if hq_positions.is_empty() {
                    // First commander: prefer a corner/side, not the dead center.
                    let dx = fx - map * 0.5;
                    let dy = fy - map * 0.5;
                    (dx * dx + dy * dy).sqrt()
                } else {
                    hq_positions
                        .iter()
                        .map(|(hx, hy)| {
                            let dx = fx - hx;
                            let dy = fy - hy;
                            (dx * dx + dy * dy).sqrt()
                        })
                        .fold(f32::MAX, f32::min)
                };
                if score > best_score {
                    best_score = score;
                    best = (fx, fy);
                }
            }
        }

        if best_score >= 0.0 && !self.ground_blocks(best.0, best.1, hq_r) {
            return best;
        }

        // Fallback spiral if the lattice somehow failed (rare).
        let (bx, by) = best;
        for k in 0..96 {
            let ang = k as f32 * 0.7;
            let dist = 3.0 + k as f32 * 0.55;
            let x = (bx + ang.cos() * dist).clamp(margin, map - margin);
            let y = (by + ang.sin() * dist).clamp(margin, map - margin);
            if !self.ground_blocks(x, y, hq_r) {
                return (x.floor() + 0.5, y.floor() + 0.5);
            }
        }
        (bx, by)
    }

    /// Drop-in join: a human replaces an existing bot commander (army + base stay).
    /// Keeps match size fixed — no new HQ is spawned.
    pub fn take_over_bot(
        &mut self,
        user_id: Uuid,
        name: String,
        faction: String,
        flag: Option<String>,
    ) -> Result<(), &'static str> {
        if self.ended {
            return Err("Match already ended");
        }
        if self.players.contains_key(&user_id) {
            return Err("Already in match");
        }

        // Prefer living bots so the joiner inherits a playable base.
        let bot_id = self
            .players
            .values()
            .filter(|p| p.is_bot())
            .max_by_key(|p| (p.alive as u8, p.colonies, p.resources.gold))
            .map(|p| p.user_id)
            .ok_or("No bot slot available")?;

        let mut state = self
            .players
            .remove(&bot_id)
            .ok_or("No bot slot available")?;
        state.user_id = user_id;
        state.name = name;
        state.faction = faction;
        state.flag = flag.clone();
        state.bot = None;
        state.connected = true;
        state.aoi_known.clear();
        // Keep team, colors, resources, explored, home_hq, colonies, stats, focus, alive.

        for entity in self.entities.values_mut() {
            if entity.owner == bot_id {
                entity.owner = user_id;
                if flag.is_some() {
                    entity.flag = flag.clone();
                }
                entity.dirty = true;
            }
            if entity.last_hit_by == Some(bot_id) {
                entity.last_hit_by = Some(user_id);
            }
        }

        for player in self.players.values_mut() {
            if let Some(mind) = player.bot.as_mut() {
                if mind.war_owner == Some(bot_id) {
                    mind.war_owner = Some(user_id);
                }
            }
        }

        self.players.insert(user_id, state);
        self.reveal_vision_for(user_id);
        Ok(())
    }

    /// Overflow join when every bot slot is already human: grow the map if the
    /// design curve / clearance needs it, then plant a new HQ at the farthest
    /// free ground from existing bases (`allocate_spawn_xy`).
    /// Returns `Ok(true)` when the playable edge grew.
    pub fn add_player(
        &mut self,
        user_id: Uuid,
        name: String,
        faction: String,
        flag: Option<String>,
    ) -> Result<bool, &'static str> {
        if self.ended {
            return Err("Match already ended");
        }
        if self.players.contains_key(&user_id) {
            return Err("Already in match");
        }

        let next_count = (self.players.len() + 1) as u16;
        let grew = self.ensure_map_for_commanders(next_count);

        let index = self.players.len();
        let team = if self.ffa {
            (index.min(u8::MAX as usize)) as u8
        } else {
            self.pick_allied_team()
        };

        self.spawn_commander(user_id, name, faction, team, flag, true, None);
        Ok(grew)
    }

    /// Expand playable edge so `commander_count` fits the spacing curve (and
    /// a usable spawn clearance). No-op at `MAX_MAP_SIZE`. Returns whether size grew.
    pub fn ensure_map_for_commanders(&mut self, commander_count: u16) -> bool {
        let need = min_spawn_clearance(commander_count);
        let curve = map_size_for_players(commander_count);
        let mut grew = false;

        // First: match the design curve for this commander count.
        if curve > self.map_size {
            self.grow_map_to(curve);
            grew = true;
        }

        // Then: if the best pocket is still too tight, keep stepping out.
        while self.best_spawn_clearance() < need && self.map_size < MAX_MAP_SIZE {
            let next = ((self.map_size as u32 + 48).min(MAX_MAP_SIZE as u32) as u16 + 1) / 2 * 2;
            if next <= self.map_size {
                break;
            }
            self.grow_map_to(next);
            grew = true;
        }
        grew
    }

    fn grow_map_to(&mut self, new_size: u16) {
        let new_size = new_size.clamp(MIN_MAP_SIZE, MAX_MAP_SIZE);
        if new_size <= self.map_size {
            return;
        }
        let old = self.map_size;
        self.map_size = new_size;
        for player in self.players.values_mut() {
            player.explored.expand_to(new_size);
        }
        self.extend_terrain_rim(old, new_size);
    }

    /// Sprinkle a few lakes/rocks into the new L-shaped border so the rim isn't empty.
    fn extend_terrain_rim(&mut self, old_size: u16, new_size: u16) {
        if new_size <= old_size {
            return;
        }
        let old = old_size as f32;
        let map = new_size as f32;
        let mut seed = self.id.as_u128() as u64
            ^ ((old_size as u64) << 17)
            ^ ((new_size as u64) << 3);
        if seed == 0 {
            seed = 0xBEE5_F00Du64;
        }
        let extras = 2 + (seed % 3) as usize;
        let mut added_ponds = 0usize;
        let mut attempts = 0usize;
        while added_ponds < extras && attempts < extras * 40 {
            attempts += 1;
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(attempts as u64 + 3);
            let x = 12.0 + ((seed % 10_000) as f32 / 10_000.0) * (map - 24.0).max(8.0);
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(11);
            let y = 12.0 + ((seed % 10_000) as f32 / 10_000.0) * (map - 24.0).max(8.0);
            // Only place in the newly added rim.
            if x < old - 2.0 && y < old - 2.0 {
                continue;
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(23);
            let r = 3.2 + ((seed % 70) as f32) * 0.07;
            let mut overlap = false;
            for p in &self.ponds {
                let dx = p.x - x;
                let dy = p.y - y;
                let min = p.r + r + 4.0;
                if dx * dx + dy * dy < min * min {
                    overlap = true;
                    break;
                }
            }
            if overlap {
                continue;
            }
            for m in &self.mountains {
                let dx = m.x - x;
                let dy = m.y - y;
                let min = m.r + r + 4.5;
                if dx * dx + dy * dy < min * min {
                    overlap = true;
                    break;
                }
            }
            if overlap {
                continue;
            }
            self.ponds.push(PondView { x, y, r });
            added_ponds += 1;
        }

        let mut added_mt = 0usize;
        attempts = 0;
        while added_mt < extras && attempts < extras * 40 {
            attempts += 1;
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(attempts as u64 + 41);
            let x = 14.0 + ((seed % 10_000) as f32 / 10_000.0) * (map - 28.0).max(8.0);
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(19);
            let y = 14.0 + ((seed % 10_000) as f32 / 10_000.0) * (map - 28.0).max(8.0);
            if x < old - 2.0 && y < old - 2.0 {
                continue;
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(37);
            let r = 4.2 + ((seed % 60) as f32) * 0.07;
            let mut overlap = false;
            for p in &self.ponds {
                let dx = p.x - x;
                let dy = p.y - y;
                let min = p.r + r + 5.0;
                if dx * dx + dy * dy < min * min {
                    overlap = true;
                    break;
                }
            }
            if overlap {
                continue;
            }
            for m in &self.mountains {
                let dx = m.x - x;
                let dy = m.y - y;
                let min = m.r + r + 3.5;
                if dx * dx + dy * dy < min * min {
                    overlap = true;
                    break;
                }
            }
            if overlap {
                continue;
            }
            self.mountains.push(MountainView { x, y, r });
            added_mt += 1;
        }
    }

    /// Nearest-HQ distance of the best empty lattice cell (same scoring as spawn).
    fn best_spawn_clearance(&self) -> f32 {
        let map = self.map_size as f32;
        let margin = (map * 0.07).clamp(14.0, 36.0);
        let hq_positions: Vec<(f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.kind == "hq" && e.hp > 0.0)
            .map(|e| (e.x, e.y))
            .collect();
        if hq_positions.is_empty() {
            return map;
        }
        let hq_r = building_radius("hq") + 0.5;
        let usable = (map - 2.0 * margin).max(8.0);
        let steps = ((map / 10.0).clamp(16.0, 56.0)) as i32;
        let mut best_score = 0.0f32;
        for iy in 0..=steps {
            for ix in 0..=steps {
                let x = margin + usable * (ix as f32 / steps as f32);
                let y = margin + usable * (iy as f32 / steps as f32);
                let fx = x.clamp(margin, map - margin).floor() + 0.5;
                let fy = y.clamp(margin, map - margin).floor() + 0.5;
                if self.ground_blocks(fx, fy, hq_r) {
                    continue;
                }
                let score = hq_positions
                    .iter()
                    .map(|(hx, hy)| {
                        let dx = fx - hx;
                        let dy = fy - hy;
                        (dx * dx + dy * dy).sqrt()
                    })
                    .fold(f32::MAX, f32::min);
                if score > best_score {
                    best_score = score;
                }
            }
        }
        best_score
    }

    pub fn bot_count(&self) -> usize {
        self.players.values().filter(|p| p.is_bot()).count()
    }

    pub fn human_count(&self) -> usize {
        self.players.values().filter(|p| !p.is_bot()).count()
    }

    /// Opening base + army (USA roster for everyone while factions are locked).
    fn spawn_starting_force(&mut self, user_id: Uuid, team: u8, hx: f32, hy: f32) {
        let faction = "usa".to_string();
        if let Some(player) = self.players.get_mut(&user_id) {
            player.faction = faction.clone();
            // HQ base + margin so 5 Patriots (−150) don't brown out the starter plant.
            player.resources.power = player.resources.power.saturating_add(160);
        }

        // Ring of finished starter structures around the Command Center.
        let slots: [(&str, f32, f32); 4] = [
            ("power_plant", 3.2, 0.4),
            ("supply", 2.6, 2.0),
            ("barracks", 0.2, 3.4),
            ("war_factory", -2.8, 2.2),
        ];

        for (kind, ox, oy) in slots {
            self.spawn_finished_building(user_id, team, &faction, kind, hx + ox, hy + oy);
        }

        // Five Patriots on the perimeter — early all-ins should stall, not snowball.
        let patriot_slots: [(f32, f32); 5] = [
            (4.2, -3.4),
            (-4.2, -3.4),
            (4.6, 2.8),
            (-4.6, 2.8),
            (0.0, 5.0),
        ];
        for (ox, oy) in patriot_slots {
            self.spawn_finished_building(user_id, team, &faction, "turret", hx + ox, hy + oy);
        }

        let Some(tank) = trainables().iter().find(|u| u.unit == "tank") else {
            return;
        };

        // Small opening armor only — big armies are earned over a long match.
        const TANK_COUNT: usize = 3;
        let hq_r = building_radius("hq");
        let tr = unit_radius(tank.unit);
        let pack_cx = hx + 4.2;
        let pack_cy = hy - 0.2;
        let map = self.map_size as f32;
        for i in 0..TANK_COUNT {
            let (ox, oy) = tight_pack_slot(i, tr);
            let pack_x = (pack_cx + ox).clamp(0.5, map - 0.5);
            let pack_y = (pack_cy + oy).clamp(0.5, map - 0.5);
            let (sx, sy) = if self.collides_at(Uuid::nil(), pack_x, pack_y, tr, None, true)
                || self.point_hits_solid(pack_x, pack_y, tr, hx, hy, hq_r)
            {
                self.find_free_spawn_near(
                    pack_x,
                    pack_y,
                    tr,
                    Uuid::nil(),
                    Some((hx, hy, hq_r)),
                )
            } else {
                (pack_x, pack_y)
            };
            self.insert_unit(user_id, team, tank, sx, sy);
        }
    }

    /// Instant finished building for opening bases (no gold charge).
    fn spawn_finished_building(
        &mut self,
        owner: Uuid,
        team: u8,
        faction: &str,
        kind: &str,
        x: f32,
        y: f32,
    ) {
        let Some(def) = buildables()
            .iter()
            .find(|b| b.kind == kind && faction_ok(b.faction, faction))
        else {
            return;
        };
        let map = self.map_size as f32;
        let br = building_radius(kind);
        let fx = x.clamp(1.0, map - 1.0);
        let fy = y.clamp(1.0, map - 1.0);
        let (px, py) = if self.collides_at(Uuid::nil(), fx, fy, br, None, true) {
            self.find_free_spawn_near(fx, fy, br, Uuid::nil(), None)
        } else {
            (fx, fy)
        };

        let flag = self.players.get(&owner).and_then(|p| p.flag.clone());
        let id = Uuid::new_v4();
        let (damage, range, mag) = match def.kind {
            "turret" | "stinger_site" => (PATRIOT_DAMAGE, PATRIOT_RANGE, 0u8),
            "bunker" | "tunnel_network" => (BUNKER_DAMAGE, BUNKER_RANGE, BUNKER_MAG),
            "gatling_cannon" => (GATLING_DAMAGE, GATLING_RANGE, 60u8),
            "firebase" => (FIREBASE_DAMAGE, FIREBASE_RANGE, 0u8),
            _ => (0.0, 0.0, 0u8),
        };
        self.put_entity(Entity {
            id,
            kind: def.kind.into(),
            owner,
            team,
            x: px,
            y: py,
            hp: def.hp,
            max_hp: def.hp,
            building: true,
            unit: false,
            flag,
            build_remaining_ms: 0,
            train_queue: VecDeque::new(),
            target: None,
            move_to: None,
            speed: 0.0,
            damage,
            range,
            attack_cooldown_ms: 0,
            mag_ammo: mag,
            dirty: true,
            stuck_frames: 0,
            detour: None,
            detour_ttl: 0,
            last_escape_ang: 0.0,
            prone: false,
            prone_until_tick: 0,
            aim_yaw: 0.0,
            mg_cooldown_ms: 0,
            last_hit_by: None,
            hacked_until_tick: 0,
            ability_cooldown_ms: 0,
        });

        if let Some(player) = self.players.get_mut(&owner) {
            if def.power > 0 {
                player.resources.power = player.resources.power.saturating_add(def.power);
            } else if def.power < 0 {
                player.resources.power_used += -def.power;
            }
        }
    }

    fn point_hits_solid(&self, x: f32, y: f32, r: f32, sx: f32, sy: f32, sr: f32) -> bool {
        let dx = sx - x;
        let dy = sy - y;
        let min_d = r + sr + collision_pad();
        dx * dx + dy * dy < min_d * min_d
    }

    fn insert_unit(&mut self, owner: Uuid, team: u8, def: &UnitDef, x: f32, y: f32) {
        let id = Uuid::new_v4();
        self.put_entity(Entity {
            id,
            kind: def.unit.into(),
            owner,
            team,
            x,
            y,
            hp: def.hp,
            max_hp: def.hp,
            building: false,
            unit: true,
            flag: None,
            build_remaining_ms: 0,
            train_queue: VecDeque::new(),
            target: None,
            move_to: None,
            speed: def.speed,
            damage: def.damage,
            range: def.range,
            attack_cooldown_ms: 0,
            mag_ammo: if is_rifle_infantry(def.unit) {
                RIFLE_MAG
            } else if def.unit == "mlrs" {
                MLRS_SALVO
            } else {
                0
            },
            dirty: true,
            stuck_frames: 0,
            detour: None,
            detour_ttl: 0,
            last_escape_ang: 0.0,
            prone: false,
            prone_until_tick: 0,
            aim_yaw: 0.0,
            mg_cooldown_ms: 0,
            last_hit_by: None,
            hacked_until_tick: 0,
            ability_cooldown_ms: 0,
        });
    }

    fn put_entity(&mut self, entity: Entity) {
        self.grid.upsert(entity.id, entity.x, entity.y);
        self.entities.insert(entity.id, entity);
    }

    fn take_entity(&mut self, id: Uuid) -> Option<Entity> {
        self.grid.remove(id);
        self.entities.remove(&id)
    }

    pub fn buildable_info_for(faction: &str) -> Vec<BuildableInfo> {
        buildables()
            .iter()
            .filter(|b| faction_ok(b.faction, faction))
            .map(|b| BuildableInfo {
                kind: b.kind.into(),
                name: b.name.into(),
                faction: b.faction.into(),
                cost_gold: b.cost_gold,
                build_ms: b.build_ms,
                power: b.power,
            })
            .collect()
    }

    pub fn trainable_info_for(faction: &str) -> Vec<TrainableInfo> {
        trainables()
            .iter()
            .filter(|u| faction_ok(u.faction, faction))
            .map(|u| TrainableInfo {
                unit: u.unit.into(),
                name: u.name.into(),
                faction: u.faction.into(),
                from_building: u.from_building.into(),
                cost_gold: u.cost_gold,
                train_ms: u.train_ms,
                hp: u.hp,
                damage: u.damage,
                speed: u.speed,
                range: u.range,
            })
            .collect()
    }

    pub fn snapshot_for(&self, user_id: Uuid) -> Option<MatchSnapshot> {
        let player = self.players.values().find(|p| p.is(user_id))?;
        let focus = player.focus;
        let entities = self
            .visible_ids_for(user_id)
            .into_iter()
            .filter_map(|id| self.entities.get(&id).map(|e| self.entity_view(e)))
            .collect();

        Some(MatchSnapshot {
            match_id: self.id,
            map_size: self.map_size,
            tick: self.tick,
            you: user_id,
            you_name: player.label(),
            you_faction: player.faction.clone(),
            team: player.team,
            ffa: self.ffa,
            aoi_radius: aoi::AOI_RADIUS,
            global_vision: self.player_has_global_vision(user_id),
            focus,
            explored: player.explored.to_bytes(),
            resources: self.resources_view_for(user_id).unwrap_or_else(|| player.resources.view()),
            entities,
            buildable: Self::buildable_info_for(&player.faction),
            trainable: Self::trainable_info_for(&player.faction),
            scoreboard: self.scoreboard_for(user_id),
            ponds: self.ponds.clone(),
            mountains: self.mountains.clone(),
        })
    }

    /// Tab scoreboard: every commander, army size, and economy (FOW-independent).
    /// Single entity pass — O(entities + players), not O(players × entities).
    pub fn scoreboard_for(&self, viewer: Uuid) -> Vec<ScoreboardRow> {
        let viewer_team = self.players.get(&viewer).map(|p| p.team);
        let ffa = self.ffa;

        struct Acc {
            infantry: u32,
            tanks: u32,
            buildings: u32,
            bases: u32,
            hq_pos: Option<(f32, f32)>,
        }
        let mut acc: HashMap<Uuid, Acc> = HashMap::with_capacity(self.players.len());
        for p in self.players.values() {
            acc.insert(
                p.user_id,
                Acc {
                    infantry: 0,
                    tanks: 0,
                    buildings: 0,
                    bases: 0,
                    hq_pos: None,
                },
            );
        }
        for e in self.entities.values() {
            if e.hp <= 0.0 {
                continue;
            }
            let Some(a) = acc.get_mut(&e.owner) else {
                continue;
            };
            if e.kind == "hq" {
                a.bases += 1;
                let home = self
                    .players
                    .get(&e.owner)
                    .and_then(|p| p.home_hq);
                if home == Some(e.id) {
                    a.hq_pos = Some((e.x, e.y));
                } else if a.hq_pos.is_none() {
                    a.hq_pos = Some((e.x, e.y));
                }
            }
            if e.building {
                a.buildings += 1;
            } else if e.unit {
                let k = e.kind.as_str();
                if k.contains("tank")
                    || k.contains("mlrs")
                    || k.contains("humvee")
                    || k.contains("technical")
                    || k.contains("buggy")
                    || k.contains("scorpion")
                    || k.contains("marauder")
                    || k.contains("paladin")
                    || k.contains("overlord")
                    || k.contains("battlemaster")
                    || k.contains("tomahawk")
                    || k.contains("inferno")
                    || k.contains("scud")
                    || k.contains("quad")
                    || k.contains("microwave")
                    || k.contains("crawler")
                    || k.contains("battle_bus")
                    || k.contains("bomb_truck")
                    || k.contains("radar_van")
                    || k.contains("outpost")
                    || k.contains("ecm")
                    || k.contains("gatling")
                {
                    a.tanks += 1;
                } else if k.contains("raptor")
                    || k.contains("mig")
                    || k.contains("f16")
                    || k.contains("comanche")
                    || k.contains("helix")
                    || k.contains("chinook")
                {
                    // aircraft counted with tanks column as "vehicles" budget — keep infantry clean
                    a.tanks += 1;
                } else {
                    a.infantry += 1;
                }
            }
        }

        let mut rows: Vec<ScoreboardRow> = self
            .players
            .values()
            .map(|p| {
                let a = acc.get(&p.user_id);
                let (infantry, tanks, buildings, bases, hq_pos) = match a {
                    Some(a) => (a.infantry, a.tanks, a.buildings, a.bases, a.hq_pos),
                    None => (0, 0, 0, 0, None),
                };
                ScoreboardRow {
                    id: p.user_id,
                    name: p.label(),
                    faction: p.faction.clone(),
                    colors: p.colors,
                    team: p.team,
                    alive: p.alive,
                    bot: p.is_bot(),
                    you: p.user_id == viewer,
                    infantry,
                    tanks,
                    buildings,
                    bases,
                    gold: p.resources.gold,
                    power: p.resources.power,
                    power_used: p.resources.power_used,
                    hq_x: hq_pos.map(|(x, _)| x),
                    hq_y: hq_pos.map(|(_, y)| y),
                }
            })
            .collect();
        // Power ranking: alive → bases → army → buildings → gold (live ladder).
        let _ = (viewer_team, ffa);
        rows.sort_by(|a, b| {
            b.alive
                .cmp(&a.alive)
                .then_with(|| b.bases.cmp(&a.bases))
                .then_with(|| (b.infantry + b.tanks).cmp(&(a.infantry + a.tanks)))
                .then_with(|| b.buildings.cmp(&a.buildings))
                .then_with(|| b.gold.cmp(&a.gold))
                .then_with(|| a.name.cmp(&b.name))
        });
        rows
    }

    /// Full roster stats for the post-match report.
    pub fn match_end_roster(&self, viewer: Uuid) -> Vec<MatchPlayerStats> {
        let mut rows: Vec<MatchPlayerStats> = self
            .players
            .values()
            .map(|p| {
                let s = &p.stats;
                let won = self.winner_team.map(|t| t == p.team).unwrap_or(false);
                MatchPlayerStats {
                    id: p.user_id,
                    name: p.label(),
                    faction: p.faction.clone(),
                    team: p.team,
                    bot: p.is_bot(),
                    you: p.user_id == viewer,
                    won,
                    buildings_built: s.buildings_built,
                    buildings_destroyed: s.buildings_destroyed,
                    buildings_lost: s.buildings_lost,
                    infantry_killed: s.infantry_killed,
                    tanks_killed: s.tanks_killed,
                    aircraft_killed: s.aircraft_killed,
                    infantry_produced: s.infantry_produced,
                    tanks_produced: s.tanks_produced,
                    aircraft_produced: s.aircraft_produced,
                    units_lost: s.units_lost,
                    gold_earned: s.gold_earned,
                    power_earned: s.power_earned,
                    bases_captured: s.bases_captured,
                }
            })
            .collect();
        rows.sort_by(|a, b| {
            b.won
                .cmp(&a.won)
                .then_with(|| {
                    let ak = a.buildings_destroyed + a.infantry_killed + a.tanks_killed + a.aircraft_killed;
                    let bk = b.buildings_destroyed + b.infantry_killed + b.tanks_killed + b.aircraft_killed;
                    bk.cmp(&ak)
                })
                .then_with(|| a.name.cmp(&b.name))
        });
        rows
    }

    /// Stamp current unit/building vision into the player's explored map.
    /// Allied matches share teammate vision discs.
    pub fn reveal_vision_for(&mut self, user_id: Uuid) -> Vec<u16> {
        if self.player_has_global_vision(user_id) {
            if let Some(player) = self.players.get_mut(&user_id) {
                player.explored.reveal_all();
            }
            return Vec::new();
        }
        let viewer_team = self.players.get(&user_id).map(|p| p.team);
        let share = !self.ffa;
        let sources: Vec<(f32, f32, f32)> = self
            .entities
            .values()
            .filter(|e| {
                if !aoi::entity_provides_vision(e) {
                    return false;
                }
                e.owner == user_id || (share && viewer_team == Some(e.team))
            })
            .map(|e| (e.x, e.y, aoi::vision_radius(e)))
            .collect();
        let Some(player) = self.players.get_mut(&user_id) else {
            return Vec::new();
        };
        let mut newly = Vec::new();
        for (x, y, radius) in sources {
            newly.extend(player.explored.reveal_circle(x, y, radius));
        }
        newly
    }

    /// Dev only: this commander pressed M (personal full-map; others stay fogged).
    pub fn player_has_global_vision(&self, user_id: Uuid) -> bool {
        self.players
            .get(&user_id)
            .is_some_and(|p| p.debug_omniscient)
    }

    pub fn toggle_debug_vision(&mut self, user_id: Uuid) -> bool {
        let Some(player) = self.players.get_mut(&user_id) else {
            return false;
        };
        player.debug_omniscient = !player.debug_omniscient;
        let on = player.debug_omniscient;
        if on {
            // Force a full resync of every entity now that the map is open.
            player.aoi_known.clear();
            player.explored.reveal_all();
            return true;
        }
        // Keep aoi_known so the next delta emits `removed` for units that fall
        // back into the fog — clearing it left ghosts on the client (full map + lag).
        player.explored = aoi::ExploredMap::new(self.map_size);
        let _ = self.reveal_vision_for(user_id);
        false
    }

    pub fn place_building(
        &mut self,
        user_id: Uuid,
        kind: &str,
        x: i32,
        y: i32,
    ) -> Result<(), &'static str> {
        if self.ended {
            return Err("Match already ended");
        }
        let faction = {
            let player = self.players.get(&user_id).ok_or("Not in match")?;
            if !player.alive {
                return Err("Eliminated");
            }
            player.faction.clone()
        };
        let def = buildables()
            .iter()
            .find(|b| b.kind == kind && faction_ok(b.faction, &faction))
            .ok_or("Unknown building")?;
        let my_team = self.players.get(&user_id).map(|p| p.team).unwrap_or(0);

        // One construction at a time — bots and humans both (Generals dozer rule).
        if self
            .entities
            .values()
            .any(|e| e.owner == user_id && e.building && e.hp > 0.0 && e.build_remaining_ms > 0)
        {
            return Err("Already constructing — finish the current building first");
        }

        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;
        if x < 0 || y < 0 || x >= self.map_size as i32 || y >= self.map_size as i32 {
            return Err("Out of bounds");
        }

        let place_r = building_radius(kind);
        let mut blocked = false;
        self.grid.for_each_nearby(
            fx,
            fy,
            place_r + MAX_ENTITY_RADIUS + collision_pad(),
            |id| {
                let Some(e) = self.entities.get(&id) else {
                    return false;
                };
                if !e.building {
                    return false;
                }
                let other_r = building_radius(&e.kind);
                let dx = e.x - fx;
                let dy = e.y - fy;
                let min_dist = place_r + other_r + collision_pad();
                if dx * dx + dy * dy < min_dist * min_dist {
                    blocked = true;
                    return true;
                }
                false
            },
        );
        if blocked {
            return Err("Tile occupied");
        }

        // Don't plant on top of living ground units — that traps them inside the footprint.
        let mut unit_blocked = false;
        self.grid.for_each_nearby(
            fx,
            fy,
            place_r + MAX_UNIT_RADIUS + collision_pad(),
            |id| {
                let Some(e) = self.entities.get(&id) else {
                    return false;
                };
                if !e.unit || e.hp <= 0.0 || is_air_kind(&e.kind) {
                    return false;
                }
                let ur = unit_radius(&e.kind);
                let dx = e.x - fx;
                let dy = e.y - fy;
                let min_dist = place_r + ur + collision_pad();
                if dx * dx + dy * dy < min_dist * min_dist {
                    unit_blocked = true;
                    return true;
                }
                false
            },
        );
        if unit_blocked {
            return Err("Units in the way");
        }

        // Cannot plant structures inside another commander's base footprint.
        let mut in_enemy_land = false;
        self.grid.for_each_nearby(fx, fy, 14.0 + place_r, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if !e.building || e.team == my_team || e.hp <= 0.0 {
                return false;
            }
            let dx = e.x - fx;
            let dy = e.y - fy;
            let block = Self::enemy_build_block_radius(&e.kind) + place_r;
            if dx * dx + dy * dy < block * block {
                in_enemy_land = true;
                return true;
            }
            false
        });
        if in_enemy_land {
            return Err("Enemy territory");
        }

        if self.water_blocks(fx, fy, place_r) {
            return Err("Cannot build on water");
        }
        if self.mountain_blocks(fx, fy, place_r) {
            return Err("Cannot build on mountain");
        }

        let buildings = self.count_buildings_for(user_id);
        let building_cap = self.building_budget_for(user_id);
        if buildings >= building_cap {
            return Err("Building limit — capture another HQ for more slots");
        }

        let player = self.players.get_mut(&user_id).ok_or("Not in match")?;
        if player.resources.gold < def.cost_gold {
            return Err("Not enough gold");
        }

        let power_after = player.resources.power_used - def.power.min(0);
        if def.power < 0 && power_after > player.resources.power {
            return Err("Not enough power");
        }

        player.resources.gold -= def.cost_gold;
        // Consumers reserve power on place; producers add generation when the build finishes.
        if def.power < 0 {
            player.resources.power_used += -def.power;
        }

        let flag = player.flag.clone();
        let id = Uuid::new_v4();
        let (damage, range, mag) = match def.kind {
            "turret" | "stinger_site" => (PATRIOT_DAMAGE, PATRIOT_RANGE, 0u8),
            "bunker" | "tunnel_network" => (BUNKER_DAMAGE, BUNKER_RANGE, BUNKER_MAG),
            "gatling_cannon" => (GATLING_DAMAGE, GATLING_RANGE, 60u8),
            "firebase" => (FIREBASE_DAMAGE, FIREBASE_RANGE, 0u8),
            _ => (0.0, 0.0, 0u8),
        };
        self.put_entity(Entity {
            id,
            kind: def.kind.into(),
            owner: user_id,
            team: my_team,
            x: fx,
            y: fy,
            hp: def.hp,
            max_hp: def.hp,
            building: true,
            unit: false,
            flag,
            build_remaining_ms: def.build_ms,
            train_queue: VecDeque::new(),
            target: None,
            move_to: None,
            speed: 0.0,
            damage,
            range,
            attack_cooldown_ms: 0,
            mag_ammo: mag,
            dirty: true,
            stuck_frames: 0,
            detour: None,
            detour_ttl: 0,
            last_escape_ang: 0.0,
            prone: false,
            prone_until_tick: 0,
            aim_yaw: 0.0,
            mg_cooldown_ms: 0,
            last_hit_by: None,
            hacked_until_tick: 0,
            ability_cooldown_ms: 0,
        });

        // Redis-stream style delayed job marker.
        self.stream_jobs.push_back(StreamJob {
            due_tick: self.tick + (def.build_ms as u64 / (1000 / TICK_HZ as u64)).max(1),
        });
        self.reveal_vision_for(user_id);

        if let Some(p) = self.players.get_mut(&user_id) {
            // Count on place — construction started / ordered.
            p.stats.buildings_built = p.stats.buildings_built.saturating_add(1);
        }

        Ok(())
    }

    pub fn train_unit(
        &mut self,
        user_id: Uuid,
        building_id: Uuid,
        unit: &str,
    ) -> Result<(), &'static str> {
        if self.ended {
            return Err("Match already ended");
        }
        let def = trainables()
            .iter()
            .find(|u| u.unit == unit)
            .ok_or("Unknown unit")?;

        // mlrs / tank skins are always trainable (fair).
        let player = self.players.get(&user_id).ok_or("Not in match")?;
        if !player.alive {
            return Err("Eliminated");
        }
        if !faction_ok(def.faction, &player.faction) {
            return Err("Wrong faction unit");
        }

        let building = self
            .entities
            .get(&building_id)
            .ok_or("Building not found")?;
        if building.owner != user_id || !building.building || building.build_remaining_ms > 0 {
            return Err("Invalid barracks/factory");
        }
        if building.kind != def.from_building {
            return Err("Wrong building type");
        }

        // Only HQ-scaled army budget (shown on HUD as units/units_cap).
        let budget = self.unit_budget_for(user_id);
        let total = self.count_all_units_with_queue(user_id);
        if total >= budget {
            return Err("Unit limit — capture another HQ for more army slots");
        }

        let player = self.players.get_mut(&user_id).unwrap();
        if !player.resources.has_power() {
            return Err("No power — buildings offline");
        }
        if player.resources.gold < def.cost_gold {
            return Err("Not enough gold");
        }

        player.resources.gold -= def.cost_gold;

        let building = self.entities.get_mut(&building_id).unwrap();
        building.train_queue.push_back(TrainJob {
            unit: def.unit.into(),
            remaining_ms: def.train_ms,
        });
        building.dirty = true;
        Ok(())
    }

    fn count_all_units_with_queue(&self, owner: Uuid) -> usize {
        let living = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.unit && e.hp > 0.0)
            .count();
        let queued = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.building && e.hp > 0.0)
            .map(|e| e.train_queue.len())
            .sum::<usize>();
        living + queued
    }

    fn count_buildings_for(&self, owner: Uuid) -> usize {
        self.entities
            .values()
            .filter(|e| {
                e.owner == owner && e.building && e.hp > 0.0 && e.kind != "hq"
            })
            .count()
    }

    fn hq_count_for(&self, owner: Uuid) -> usize {
        self.entities
            .values()
            .filter(|e| e.owner == owner && e.kind == "hq" && e.hp > 0.0)
            .count()
    }

    /// Home X + X/2 per extra HQ (colony). No HQ → no train rights.
    pub fn unit_budget_for(&self, owner: Uuid) -> usize {
        let hqs = self.hq_count_for(owner);
        if hqs == 0 {
            return 0;
        }
        HOME_UNIT_BUDGET + COLONY_UNIT_BUDGET.saturating_mul(hqs.saturating_sub(1))
    }

    /// Same x + x/2 rule for non-HQ structures.
    pub fn building_budget_for(&self, owner: Uuid) -> usize {
        let hqs = self.hq_count_for(owner);
        if hqs == 0 {
            return 0;
        }
        HOME_BUILDING_BUDGET + COLONY_BUILDING_BUDGET.saturating_mul(hqs.saturating_sub(1))
    }

    pub fn resources_view_for(&self, user_id: Uuid) -> Option<ResourcesView> {
        let player = self.players.get(&user_id)?;
        let bases = self.hq_count_for(user_id);
        Some(ResourcesView {
            gold: player.resources.gold,
            power: player.resources.power,
            power_used: player.resources.power_used,
            units: self.count_all_units_with_queue(user_id) as u32,
            units_cap: self.unit_budget_for(user_id) as u32,
            buildings: self.count_buildings_for(user_id) as u32,
            buildings_cap: self.building_budget_for(user_id) as u32,
            bases: bases as u32,
        })
    }

    /// After HQ loss (or gain), snap queues/construction back under the new x+x/2 caps.
    fn enforce_budgets_for(&mut self, owner: Uuid) {
        self.trim_train_queues_to_budget(owner);
        self.trim_buildings_to_budget(owner);
    }

    fn trim_train_queues_to_budget(&mut self, owner: Uuid) {
        let budget = self.unit_budget_for(owner);
        let living = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.unit && e.hp > 0.0)
            .count();
        let mut room = budget.saturating_sub(living);

        let mut building_ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.building && e.hp > 0.0 && !e.train_queue.is_empty())
            .map(|e| e.id)
            .collect();
        // Drop newest queued jobs first (back of each queue, later buildings last).
        building_ids.sort_by_key(|id| *id);

        let mut refund_gold = 0i32;
        for id in building_ids.iter().rev() {
            let Some(building) = self.entities.get_mut(id) else {
                continue;
            };
            while building.train_queue.len() > room {
                if let Some(job) = building.train_queue.pop_back() {
                    if let Some(def) = trainables().iter().find(|u| u.unit == job.unit) {
                        refund_gold = refund_gold.saturating_add(def.cost_gold);
                    }
                    building.dirty = true;
                } else {
                    break;
                }
            }
            room = room.saturating_sub(building.train_queue.len());
        }

        if refund_gold > 0 {
            if let Some(player) = self.players.get_mut(&owner) {
                player.resources.gold = player.resources.gold.saturating_add(refund_gold);
            }
        }
    }

    fn trim_buildings_to_budget(&mut self, owner: Uuid) {
        let budget = self.building_budget_for(owner);
        let mut count = self.count_buildings_for(owner);
        if count <= budget {
            return;
        }

        // Cancel unfinished builds first, then oldest finished non-HQ if still over.
        let mut constructing: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| {
                e.owner == owner
                    && e.building
                    && e.hp > 0.0
                    && e.kind != "hq"
                    && e.build_remaining_ms > 0
            })
            .map(|e| e.id)
            .collect();
        constructing.sort_by_key(|id| *id);
        for id in constructing.into_iter().rev() {
            if count <= budget {
                break;
            }
            if let Some(e) = self.take_entity(id) {
                self.refund_building_economy(&e);
                // Partial gold refund for cancelled construction.
                if let Some(def) = buildables().iter().find(|b| b.kind == e.kind) {
                    if let Some(player) = self.players.get_mut(&owner) {
                        player.resources.gold =
                            player.resources.gold.saturating_add(def.cost_gold / 2);
                    }
                }
                self.removed.push(id);
                count -= 1;
            }
        }
    }

    pub fn move_units(&mut self, user_id: Uuid, ids: &[Uuid], x: f32, y: f32) {
        let map = self.map_size as f32;
        let tx = x.clamp(0.5, map - 0.5);
        let ty = y.clamp(0.5, map - 0.5);
        let count = ids
            .iter()
            .filter(|id| {
                self.entities.get(id).is_some_and(|e| {
                    e.owner == user_id && e.unit && e.build_remaining_ms == 0
                })
            })
            .count()
            .max(1);
        let mut slot = 0usize;
        for id in ids {
            if let Some(entity) = self.entities.get_mut(id) {
                if entity.owner == user_id && entity.unit && entity.build_remaining_ms == 0 {
                    // Hangared F-16 stays on the pad — only attack orders launch a sortie.
                    if is_f16_kind(&entity.kind)
                        && entity.mag_ammo == 0
                        && entity.move_to.is_none()
                        && entity.target.is_none()
                    {
                        continue;
                    }
                    let (ox, oy) = formation_slot(slot, count, unit_radius(&entity.kind));
                    slot += 1;
                    let gx = (tx + ox).clamp(0.5, map - 0.5);
                    let gy = (ty + oy).clamp(0.5, map - 0.5);
                    entity.move_to = Some((gx, gy));
                    entity.target = None;
                    entity.stuck_frames = 0;
                    entity.detour = None;
                    entity.detour_ttl = 0;
                    entity.dirty = true;
                }
            }
        }
        if let Some(player) = self.players.get_mut(&user_id) {
            player.focus = [tx, ty];
        }
    }

    pub fn attack(&mut self, user_id: Uuid, ids: &[Uuid], target_id: Uuid) -> Result<(), &'static str> {
        let Some(target) = self.entities.get(&target_id) else {
            return Ok(());
        };
        if target.team
            == self
                .players
                .get(&user_id)
                .map(|p| p.team)
                .unwrap_or(255)
        {
            return Ok(());
        }
        // Stealth specialists cannot be ordered as attack targets.
        if is_stealth_specialist(&target.kind) {
            return Err("Hedef görünmez");
        }
        let mut sortie_err: Option<&'static str> = None;
        let mut armed_any = false;
        for id in ids {
            // Unarmed stealth ops — move only; no attack orders.
            if self
                .entities
                .get(id)
                .is_some_and(|e| e.owner == user_id && is_stealth_specialist(&e.kind))
            {
                continue;
            }
            let is_jet = self
                .entities
                .get(id)
                .is_some_and(|e| e.owner == user_id && e.unit && is_f16_kind(&e.kind));
            if is_jet {
                match self.begin_f16_sortie(user_id, *id) {
                    Ok(()) => armed_any = true,
                    Err(e) => {
                        sortie_err = Some(e);
                        continue;
                    }
                }
            }
            if let Some(entity) = self.entities.get_mut(id) {
                if entity.owner == user_id && entity.unit {
                    entity.target = Some(target_id);
                    entity.move_to = None;
                    entity.stuck_frames = 0;
                    entity.detour = None;
                    entity.detour_ttl = 0;
                    entity.dirty = true;
                    if is_f16_kind(&entity.kind) {
                        armed_any = true;
                    }
                }
            }
        }
        if let Some(err) = sortie_err {
            if !armed_any {
                return Err(err);
            }
            // Mixed selection: tanks still attack; surface the jet refusal.
            return Err(err);
        }
        Ok(())
    }

    /// Pay for an F-16 takeoff and load bombs. Already-armed jets skip the fee.
    fn begin_f16_sortie(&mut self, user_id: Uuid, unit_id: Uuid) -> Result<(), &'static str> {
        let Some(e) = self.entities.get(&unit_id) else {
            return Err("Jet not found");
        };
        if !is_f16_kind(&e.kind) || e.owner != user_id {
            return Ok(());
        }
        if e.mag_ammo > 0 {
            return Ok(());
        }
        if e.ability_cooldown_ms > 0 {
            return Err("F-16 mühimmat yeniliyor — sonraki kalkış hazır değil");
        }
        let player = self.players.get_mut(&user_id).ok_or("Not in match")?;
        if player.resources.gold < F16_SORTIE_GOLD {
            return Err("F-16 kalkışı için 6500 gold gerekli");
        }
        player.resources.gold -= F16_SORTIE_GOLD;
        if let Some(e) = self.entities.get_mut(&unit_id) {
            e.mag_ammo = F16_BOMBS;
            // Leave the pad toward the assigned target (attack() sets target next).
            e.move_to = None;
            e.dirty = true;
        }
        Ok(())
    }

    fn nearest_owned_airfield_pad(&self, owner: Uuid, from_x: f32, from_y: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32, f32)> = None;
        for e in self.entities.values() {
            if e.owner != owner || e.hp <= 0.0 || e.kind != "airfield" || e.build_remaining_ms > 0 {
                continue;
            }
            let dx = e.x - from_x;
            let dy = e.y - from_y;
            let d2 = dx * dx + dy * dy;
            if best.map_or(true, |(_, _, d)| d2 < d) {
                best = Some((e.x + 2.4, e.y, d2));
            }
        }
        let map = self.map_size as f32;
        if let Some((x, y, _)) = best {
            return Some((x.clamp(0.5, map - 0.5), y.clamp(0.5, map - 0.5)));
        }
        self.players.get(&owner).and_then(|p| {
            p.home_hq.and_then(|hq| {
                self.entities.get(&hq).map(|e| {
                    ((e.x + 2.0).clamp(0.5, map - 0.5), e.y.clamp(0.5, map - 0.5))
                })
            })
        })
    }

    pub fn set_focus(&mut self, user_id: Uuid, x: f32, y: f32) {
        if let Some(player) = self.players.get_mut(&user_id) {
            player.focus = [x, y];
        }
    }

    /// Completed economy buildings pay out once per second so they matter after the build.
    fn apply_building_income(&mut self) {
        if self.tick % u64::from(TICK_HZ) != 0 {
            return;
        }
        #[derive(Default, Clone, Copy)]
        struct Gain {
            gold: i32,
            pwr: i32,
        }
        let mut by_owner: HashMap<Uuid, Gain> = HashMap::new();
        for e in self.entities.values() {
            if !e.building || e.hp <= 0.0 || e.build_remaining_ms > 0 {
                continue;
            }
            if self.tick < e.hacked_until_tick {
                continue; // Hacker blackout — no income from this structure.
            }
            let g = by_owner.entry(e.owner).or_default();
            match e.kind.as_str() {
                "hq" => {
                    g.gold += 10;
                }
                "supply" | "supply_stash" => {
                    g.gold += 36;
                }
                "black_market" => {
                    g.gold += 48;
                }
                "power_plant" | "nuclear_reactor" if is_power_producer(e.kind.as_str()) => {
                    g.pwr += 15;
                }
                "war_factory" | "arms_dealer" => {
                    g.gold += 14;
                }
                "barracks" => {
                    g.gold += 4;
                }
                "internet_center" | "propaganda_center" | "strategy_center" | "palace" => {
                    g.gold += 16;
                }
                _ => {}
            }
        }
        for (owner, g) in by_owner {
            let Some(player) = self.players.get_mut(&owner) else {
                continue;
            };
            if !player.alive {
                continue;
            }
            // Brownout: only generators still tick; gold income freezes.
            let powered = player.resources.has_power();
            if powered {
                player.resources.gold = player.resources.gold.saturating_add(g.gold);
                if g.gold > 0 {
                    player.stats.gold_earned =
                        player.stats.gold_earned.saturating_add(g.gold as u32);
                }
            }
            player.resources.power = player.resources.power.saturating_add(g.pwr);
            if g.pwr > 0 {
                player.stats.power_earned =
                    player.stats.power_earned.saturating_add(g.pwr as u32);
            }
        }
    }

    fn on_building_finished(&mut self, entity: &Entity) {
        let Some(def) = buildables().iter().find(|b| b.kind == entity.kind) else {
            return;
        };
        if def.power > 0 {
            if let Some(player) = self.players.get_mut(&entity.owner) {
                player.resources.power = player.resources.power.saturating_add(def.power);
            }
        }
        // Radar / any finished building: stamp its vision disc immediately.
        let owner = entity.owner;
        let _ = self.reveal_vision_for(owner);
        if !self.ffa {
            let team = entity.team;
            let allies: Vec<Uuid> = self
                .players
                .values()
                .filter(|p| p.team == team && p.user_id != owner)
                .map(|p| p.user_id)
                .collect();
            for uid in allies {
                let _ = self.reveal_vision_for(uid);
            }
        }
    }

    fn refund_building_economy(&mut self, entity: &Entity) {
        if !entity.building {
            return;
        }
        let Some(def) = buildables().iter().find(|b| b.kind == entity.kind) else {
            return;
        };
        let Some(player) = self.players.get_mut(&entity.owner) else {
            return;
        };
        if def.power < 0 {
            player.resources.power_used = (player.resources.power_used + def.power).max(0);
        } else if entity.build_remaining_ms == 0 && def.power > 0 {
            player.resources.power = (player.resources.power - def.power).max(0);
        }
    }

    /// Player-ordered scrap — frees the slot, refunds scrap gold, plays wreck FX.
    /// Command Centers cannot be demolished.
    pub fn demolish_building(
        &mut self,
        user_id: Uuid,
        building_id: Uuid,
    ) -> Result<(), &'static str> {
        if self.ended {
            return Err("Match already ended");
        }
        let player = self.players.get(&user_id).ok_or("Not in match")?;
        if !player.alive {
            return Err("Eliminated");
        }
        let Some(entity) = self.entities.get(&building_id) else {
            return Err("Building not found");
        };
        if entity.owner != user_id || !entity.building {
            return Err("Bu bina senin değil");
        }
        if entity.kind == "hq" {
            return Err("Komuta merkezi yıkılamaz");
        }

        let unfinished = entity.build_remaining_ms > 0;
        let scrap_gold = buildables()
            .iter()
            .find(|b| b.kind == entity.kind)
            .map(|def| {
                if unfinished {
                    // Cancel construction — half back.
                    def.cost_gold / 2
                } else {
                    // Scrap finished structure — quarter salvage.
                    def.cost_gold / 4
                }
            })
            .unwrap_or(0);

        let queue_refund: i32 = entity
            .train_queue
            .iter()
            .filter_map(|job| {
                trainables()
                    .iter()
                    .find(|u| u.unit == job.unit)
                    .map(|u| u.cost_gold)
            })
            .sum();

        let Some(entity) = self.take_entity(building_id) else {
            return Err("Building not found");
        };
        self.refund_building_economy(&entity);
        if let Some(p) = self.players.get_mut(&user_id) {
            let back = scrap_gold.saturating_add(queue_refund);
            p.resources.gold = p.resources.gold.saturating_add(back);
        }
        // Voluntary scrap — not a combat loss; still emit as death for wreck FX.
        self.removed.push(building_id);
        Ok(())
    }

    pub fn tick_once(&mut self) {
        if self.ended {
            return;
        }
        self.tick += 1;
        let dt_ms = 1000 / TICK_HZ;
        self.grid
            .rebuild(self.entities.values().map(|e| (e.id, e.x, e.y)));

        self.apply_building_income();

        let building_ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.building)
            .map(|e| e.id)
            .collect();
        for id in building_ids {
            let Some(mut entity) = self.take_entity(id) else {
                continue;
            };

            let was_building = entity.build_remaining_ms > 0;
            if entity.build_remaining_ms > 0 {
                entity.build_remaining_ms = entity.build_remaining_ms.saturating_sub(dt_ms);
                entity.dirty = true;
            }
            if was_building && entity.build_remaining_ms == 0 {
                self.on_building_finished(&entity);
                entity.dirty = true;
            }
            // Push a FOW update when hack blackout ends so clients clear the FX.
            if entity.hacked_until_tick > 0 && self.tick == entity.hacked_until_tick {
                entity.dirty = true;
            }

            let powered = self
                .players
                .get(&entity.owner)
                .map(|p| p.resources.has_power())
                .unwrap_or(false);

            // Brownout: factories / barracks freeze production. Power plants still finish.
            // Hacked buildings also freeze (cyber blackout).
            let hacked = self.tick < entity.hacked_until_tick;
            if entity.build_remaining_ms == 0 && powered && !hacked {
                if let Some(job) = entity.train_queue.front_mut() {
                    job.remaining_ms = job.remaining_ms.saturating_sub(dt_ms);
                    entity.dirty = true;
                    if job.remaining_ms == 0 {
                        let unit_kind = entity.train_queue.pop_front().unwrap().unit;
                        entity.dirty = true;
                        let owner = entity.owner;
                        let living = self
                            .entities
                            .values()
                            .filter(|e| e.owner == owner && e.unit && e.hp > 0.0)
                            .count();
                        let budget = self.unit_budget_for(owner);
                        if living >= budget {
                            // HQ loss shrank the cap mid-train — refund, don't spawn.
                            if let Some(def) = trainables().iter().find(|u| u.unit == unit_kind) {
                                if let Some(player) = self.players.get_mut(&owner) {
                                    player.resources.gold =
                                        player.resources.gold.saturating_add(def.cost_gold);
                                }
                            }
                        } else if let Some(def) = trainables().iter().find(|u| u.unit == unit_kind) {
                            let uid = Uuid::new_v4();
                            let (sx, sy) = if is_f16_kind(def.unit) {
                                // Park on the airfield apron — hangared until a sortie.
                                let map = self.map_size as f32;
                                (
                                    (entity.x + 0.15).clamp(0.5, map - 0.5),
                                    entity.y.clamp(0.5, map - 0.5),
                                )
                            } else if is_air_kind(def.unit) {
                                self.find_air_spawn_near(
                                    entity.x,
                                    entity.y,
                                    unit_radius(def.unit),
                                    uid,
                                )
                            } else {
                                self.find_free_spawn_near(
                                    entity.x,
                                    entity.y,
                                    unit_radius(def.unit),
                                    uid,
                                    Some((entity.x, entity.y, building_radius(&entity.kind))),
                                )
                            };
                            let spawn = Entity {
                                id: uid,
                                kind: def.unit.into(),
                                owner: entity.owner,
                                team: entity.team,
                                x: sx,
                                y: sy,
                                hp: def.hp,
                                max_hp: def.hp,
                                building: false,
                                unit: true,
                                flag: None,
                                build_remaining_ms: 0,
                                train_queue: VecDeque::new(),
                                target: None,
                                move_to: None,
                                speed: def.speed,
                                damage: def.damage,
                                range: def.range,
                                attack_cooldown_ms: 0,
                                mag_ammo: if is_rifle_infantry(def.unit) {
                                    RIFLE_MAG
                                } else {
                                    0
                                },
                                dirty: true,
                                stuck_frames: 0,
                                detour: None,
                                detour_ttl: 0,
                                last_escape_ang: 0.0,
                                prone: false,
                                prone_until_tick: 0,
                                aim_yaw: 0.0,
                                mg_cooldown_ms: 0,
                                last_hit_by: None,
                                hacked_until_tick: 0,
                                ability_cooldown_ms: 0,
                            };
                            self.put_entity(spawn);
                            if let Some(p) = self.players.get_mut(&owner) {
                                credit_unit_produced(&mut p.stats, def.unit);
                            }
                        }
                    }
                }
            }

            // Armed buildings go dark without power or while hacked.
            if powered
                && !hacked
                && entity.build_remaining_ms == 0
                && entity.damage > 0.0
                && entity.range > 0.0
            {
                self.tick_armed_building(&mut entity, dt_ms);
            }

            self.put_entity(entity);
        }

        // Units move after buildings are all back in the map (solid obstacles).
        let unit_ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.unit)
            .map(|e| e.id)
            .collect();
        for id in unit_ids {
            let Some(mut entity) = self.take_entity(id) else {
                continue;
            };

            let self_r = unit_radius(&entity.kind);
            let airborne = is_air_kind(&entity.kind);
            if is_f16_kind(&entity.kind) {
                entity.ability_cooldown_ms = entity.ability_cooldown_ms.saturating_sub(dt_ms);
            }
            // If already overlapping a building (e.g. planted on top), shove clear first.
            if !airborne {
                if let Some((bx, by, br)) =
                    self.building_overlap(entity.id, entity.x, entity.y, self_r)
                {
                    let (nx, ny) = self.clear_point_from_building(
                        entity.id,
                        entity.x,
                        entity.y,
                        self_r,
                        bx,
                        by,
                        br,
                    );
                    if (nx - entity.x).abs() > 0.0001 || (ny - entity.y).abs() > 0.0001 {
                        entity.x = nx;
                        entity.y = ny;
                        entity.stuck_frames = 0;
                        entity.dirty = true;
                    }
                }
            }
            // Move orders control pathing only — units may still shoot while marching.
            let obeying_move = entity.move_to.is_some();
            // Squads walk through friendlies toward the click; buildings / enemies still block.
            let pass_allies = obeying_move.then_some(entity.team);

            // Acquire / refresh targets in weapon range (including while moving).
            // F-16: only engage while bombs are loaded (paid sortie) — no free auto-hunting.
            // Stealth specialists are unarmed scouts — never lock a fire target.
            if is_stealth_specialist(&entity.kind) {
                if entity.damage != 0.0 || entity.range != 0.0 {
                    entity.damage = 0.0;
                    entity.range = 0.0;
                    entity.dirty = true;
                }
                if entity.target.is_some() {
                    entity.target = None;
                    entity.dirty = true;
                }
            } else {
            let f16_can_hunt = !is_f16_kind(&entity.kind) || entity.mag_ammo > 0;
            if f16_can_hunt && entity.damage > 0.0 && entity.range > 0.0 && self.tick % 2 == 0 {
                if obeying_move {
                    // On the march: always pick nearest in-range threat (don't stick to someone behind).
                    entity.target =
                        self.find_enemy_in_range(entity.id, entity.team, entity.x, entity.y, entity.range);
                    if entity.target.is_some() {
                        entity.dirty = true;
                    }
                } else if entity.target.is_none() {
                    if let Some(tid) =
                        self.find_enemy_in_range(entity.id, entity.team, entity.x, entity.y, entity.range)
                    {
                        entity.target = Some(tid);
                        entity.dirty = true;
                    }
                }
            } else if is_f16_kind(&entity.kind) && entity.mag_ammo == 0 && entity.target.is_some() {
                // Hangared / RTB — drop stale attack locks.
                entity.target = None;
                entity.dirty = true;
            }
            }

            let mut goal = entity.move_to;
            let mut hold_for_attack = false;
            if let Some(tid) = entity.target {
                if let Some(t) = self.entities.get(&tid) {
                    let tdx = t.x - entity.x;
                    let tdy = t.y - entity.y;
                    let tdist = (tdx * tdx + tdy * tdy).sqrt();
                    let stop_at = (entity.range - 0.35).max(self_r + entity_radius(t) * 0.35);
                    let cover_blocked = if airborne {
                        false
                    } else {
                        self.shot_cover(entity.id, entity.x, entity.y, t).blocked
                    };
                    let bot_cmd = self.players.get(&entity.owner).is_some_and(|p| p.is_bot());
                    // Bots treat a march as attack-move: halt and fight anyone they can shoot.
                    let can_shoot = tdist <= entity.range && !cover_blocked;
                    let in_pocket = tdist <= stop_at && !cover_blocked;
                    if (!obeying_move && in_pocket)
                        || (bot_cmd && can_shoot)
                        || (airborne && can_shoot && !obeying_move)
                    {
                        hold_for_attack = true;
                        goal = None;
                        entity.detour = None;
                        entity.detour_ttl = 0;
                        entity.stuck_frames = 0;
                    } else if !obeying_move {
                        // Chase attack target only when not under a move order.
                        // Fully blocked by a building → walk around for a firing angle.
                        goal = Some((t.x, t.y));
                    }
                    // Player move-to: keep walking to the click and fire if in range.
                } else {
                    entity.target = None;
                }
            }

            if !hold_for_attack {
                if let Some((gx, gy)) = goal {
                    if airborne {
                        // Generals-style air: fly straight over terrain and buildings.
                        let prev_x = entity.x;
                        let prev_y = entity.y;
                        let gdx = gx - entity.x;
                        let gdy = gy - entity.y;
                        let toward_goal = (gdx * gdx + gdy * gdy).sqrt();
                        let step = entity.speed * (dt_ms as f32 / 1000.0);
                        let arrive_r = (entity.range * 0.15).clamp(0.35, 1.2);
                        if toward_goal <= arrive_r {
                            entity.x = gx;
                            entity.y = gy;
                            entity.move_to = None;
                            entity.stuck_frames = 0;
                        } else if toward_goal > 0.001 {
                            let travel = step.min(toward_goal);
                            entity.x += (gdx / toward_goal) * travel;
                            entity.y += (gdy / toward_goal) * travel;
                            entity.stuck_frames = 0;
                        }
                        let map = self.map_size as f32;
                        entity.x = entity.x.clamp(0.5, map - 0.5);
                        entity.y = entity.y.clamp(0.5, map - 0.5);
                        if (entity.x - prev_x).abs() > 0.0001 || (entity.y - prev_y).abs() > 0.0001 {
                            entity.dirty = true;
                        }
                    } else {
                    // Detour is short-lived: expire it so we re-aim at the true goal every few ticks.
                    if entity.detour_ttl > 0 {
                        entity.detour_ttl = entity.detour_ttl.saturating_sub(1);
                        if entity.detour_ttl == 0 {
                            entity.detour = None;
                        }
                    } else {
                        entity.detour = None;
                    }

                    // Arrived at detour waypoint → drop it and chase the real goal again.
                    if let Some((dx, dy)) = entity.detour {
                        let ddx = dx - entity.x;
                        let ddy = dy - entity.y;
                        if ddx * ddx + ddy * ddy < 0.45 * 0.45 {
                            entity.detour = None;
                            entity.detour_ttl = 0;
                            entity.stuck_frames = 0;
                        }
                    }

                    let prev_x = entity.x;
                    let prev_y = entity.y;
                    let gdx = gx - entity.x;
                    let gdy = gy - entity.y;
                    let toward_goal = (gdx * gdx + gdy * gdy).sqrt();
                    let step = entity.speed * (dt_ms as f32 / 1000.0);
                    let ignore = entity.target;
                    let jammed = entity.stuck_frames >= 6;
                    // Allies are skipped via `pass_allies`. Only freeze on enemy/building jams.
                    let solid_units = !jammed;
                    let arrive_r = move_arrive_radius(self_r);
                    let overrun = if entity.kind.contains("tank") {
                        Some(entity.team)
                    } else {
                        None
                    };

                    // Only stop when we are actually on the click — never because
                    // the squad was packed or an enemy was nearby.
                    let near_goal = toward_goal <= arrive_r;
                    if near_goal {
                        if toward_goal <= step
                            && !self.collides_at(entity.id, gx, gy, self_r, None, true)
                        {
                            entity.x = gx;
                            entity.y = gy;
                        }
                        entity.move_to = None;
                        entity.detour = None;
                        entity.detour_ttl = 0;
                        entity.stuck_frames = 0;
                        entity.dirty = true;
                    } else if toward_goal > 0.001 {
                        let gux = gdx / toward_goal;
                        let guy = gdy / toward_goal;

                        // Every tick: try the ordered destination first.
                        let mut moved = self
                            .steer_step_ex(
                                entity.id,
                                entity.x,
                                entity.y,
                                gux,
                                guy,
                                step,
                                self_r,
                                ignore,
                                solid_units,
                                overrun,
                                pass_allies,
                            )
                            .map(|p| (p, entity.last_escape_ang));

                        // Goal path blocked this tick → briefly use detour if we have one.
                        if moved.is_none() {
                            if let Some((tx, ty)) = entity.detour {
                                let ddx = tx - entity.x;
                                let ddy = ty - entity.y;
                                let ddist = (ddx * ddx + ddy * ddy).sqrt();
                                if ddist > 0.001 {
                                    let dux = ddx / ddist;
                                    let duy = ddy / ddist;
                                    moved = self
                                        .steer_step_ex(
                                            entity.id,
                                            entity.x,
                                            entity.y,
                                            dux,
                                            duy,
                                            step,
                                            self_r,
                                            ignore,
                                            solid_units,
                                            overrun,
                                            pass_allies,
                                        )
                                        .map(|p| (p, entity.last_escape_ang));
                                }
                            }
                        }

                        // Still stuck → stronger escape steering.
                        if moved.is_none() && jammed {
                            let (ux, uy) = if let Some((tx, ty)) = entity.detour {
                                let ddx = tx - entity.x;
                                let ddy = ty - entity.y;
                                let ddist = (ddx * ddx + ddy * ddy).sqrt().max(0.001);
                                (ddx / ddist, ddy / ddist)
                            } else {
                                (gux, guy)
                            };
                            moved = self.steer_step_escape_ex(
                                entity.id,
                                entity.x,
                                entity.y,
                                ux,
                                uy,
                                step,
                                self_r,
                                ignore,
                                entity.last_escape_ang,
                                overrun,
                                pass_allies,
                            );
                        }

                        if let Some(((nx, ny), escape_ang)) = moved {
                            entity.x = nx;
                            entity.y = ny;
                            entity.last_escape_ang = escape_ang;
                            entity.dirty = true;
                            let moved_dist =
                                ((nx - prev_x).powi(2) + (ny - prev_y).powi(2)).sqrt();
                            let new_goal_dist = {
                                let ngx = gx - nx;
                                let ngy = gy - ny;
                                (ngx * ngx + ngy * ngy).sqrt()
                            };
                            // Any real progress toward the ordered goal clears the detour.
                            if new_goal_dist < toward_goal - 0.02 {
                                entity.detour = None;
                                entity.detour_ttl = 0;
                                entity.stuck_frames = entity.stuck_frames.saturating_sub(3);
                            } else if moved_dist > step * 0.35 {
                                entity.stuck_frames = entity.stuck_frames.saturating_sub(1);
                            } else {
                                entity.stuck_frames = entity.stuck_frames.saturating_add(1);
                            }
                        } else {
                            entity.stuck_frames = entity.stuck_frames.saturating_add(2);
                        }

                        // Only abandon a move when we are actually on the click — never
                        // because the squad was briefly packed.
                        if entity.stuck_frames >= 8 {
                            if entity.target.is_none() && toward_goal <= arrive_r {
                                entity.move_to = None;
                                entity.detour = None;
                                entity.detour_ttl = 0;
                                entity.stuck_frames = 0;
                                entity.dirty = true;
                            } else {
                                let (waypoint, ang) = self.pick_escape_waypoint(
                                    entity.id,
                                    entity.x,
                                    entity.y,
                                    gx,
                                    gy,
                                    self_r,
                                    entity.last_escape_ang,
                                );
                                entity.detour = Some(waypoint);
                                entity.detour_ttl = 18; // ~0.9s at 20 Hz, then re-check goal
                                entity.last_escape_ang = ang;
                                entity.stuck_frames = 3;
                                entity.dirty = true;
                            }
                        }
                    }
                    } // end ground path (else !airborne)
                } else {
                    entity.detour = None;
                    entity.detour_ttl = 0;
                    entity.stuck_frames = 0;
                }
            } else {
                entity.detour = None;
                entity.detour_ttl = 0;
            }

            if entity.attack_cooldown_ms > 0 {
                entity.attack_cooldown_ms = entity.attack_cooldown_ms.saturating_sub(dt_ms);
            }

            // Shoot any acquired target in range — including while marching to a move order.
            if let Some(tid) = entity.target {
                if self.entities.get(&tid).is_some_and(|t| {
                    is_stealth_specialist(&t.kind) && t.team != entity.team
                }) {
                    entity.target = None;
                    entity.dirty = true;
                }
            }
            if let Some(tid) = entity.target {
                if let Some(target) = self.entities.get(&tid) {
                    let dx = target.x - entity.x;
                    let dy = target.y - entity.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let cover = if is_air_kind(&entity.kind) {
                        ShotCover {
                            blocked: false,
                            exposure: 1.0,
                        }
                    } else {
                        self.shot_cover(entity.id, entity.x, entity.y, target)
                    };
                    let mut aimed = true;
                    if (entity.kind.contains("tank") || entity.kind.contains("mlrs"))
                        && dist > 0.001
                    {
                        let desired = world_aim_yaw(dx, dy);
                        let err = shortest_angle(entity.aim_yaw, desired);
                        let rate = if entity.kind.contains("mlrs") {
                            MLRS_POD_RATE
                        } else {
                            TANK_TURRET_RATE
                        };
                        let align = if entity.kind.contains("mlrs") {
                            MLRS_AIM_ALIGN
                        } else {
                            TANK_AIM_ALIGN
                        };
                        let step = rate * (dt_ms as f32 / 1000.0);
                        entity.aim_yaw += err.clamp(-step, step);
                        aimed = err.abs() <= align;
                    }
                    if cover.blocked {
                        // No shot through walls / hulls. Keep chasing, don't burn cooldown.
                    } else if dist <= entity.range
                        && entity.attack_cooldown_ms == 0
                        && aimed
                        && !(is_f16_kind(&entity.kind) && entity.mag_ammo == 0)
                    {
                        let mut f16_spent = false;
                        if is_rifle_infantry(&entity.kind) {
                            if entity.mag_ammo == 0 {
                                entity.mag_ammo = RIFLE_MAG;
                            }
                            entity.mag_ammo = entity.mag_ammo.saturating_sub(1);
                            if entity.mag_ammo == 0 {
                                // Empty mag — 5s reload, then a fresh 30-round clip.
                                entity.attack_cooldown_ms = RIFLE_RELOAD_MS;
                                entity.mag_ammo = RIFLE_MAG;
                            } else {
                                entity.attack_cooldown_ms = attack_cooldown_for(&entity.kind);
                            }
                        } else if entity.kind.contains("mlrs") {
                            // Ripple fire: one rocket now, brief gap, then next — long reload when pod empty.
                            if entity.mag_ammo == 0 {
                                entity.mag_ammo = MLRS_SALVO;
                            }
                            entity.mag_ammo = entity.mag_ammo.saturating_sub(1);
                            if entity.mag_ammo == 0 {
                                entity.attack_cooldown_ms = MLRS_RELOAD_MS;
                            } else {
                                entity.attack_cooldown_ms = MLRS_RIPPLE_MS;
                            }
                        } else if is_f16_kind(&entity.kind) {
                            entity.mag_ammo = entity.mag_ammo.saturating_sub(1);
                            entity.attack_cooldown_ms = attack_cooldown_for(&entity.kind);
                            f16_spent = entity.mag_ammo == 0;
                        } else {
                            entity.attack_cooldown_ms = attack_cooldown_for(&entity.kind);
                        }
                        entity.dirty = true;
                        let dmg = hit_damage(&entity.kind, target, entity.damage);
                        let tx = target.x;
                        let ty = target.y;
                        let fx = entity.x;
                        let fy = entity.y;
                        let kind = entity.kind.clone();
                        let from_id = entity.id;
                        let team = entity.team;
                        let attacker_owner = entity.owner;
                        let hit_p = shot_hit_chance(&kind, target, dist, entity.range, cover.exposure);
                        let mut rng = rand::thread_rng();
                        let hit = if is_f16_kind(&kind) {
                            // Strike package — bomb leaves the jet; impact is the sortie.
                            true
                        } else {
                            rng.gen_range(0.0..1.0) < hit_p
                        };
                        // MLRS: every rocket lands in a tight beaten zone (even "hits" scatter a bit).
                        let (ix, iy) = if kind.contains("mlrs") {
                            let j = if hit { 0.32 } else { 0.58 };
                            let jx = (tx + rng.gen_range(-j..j))
                                .clamp(0.5, self.map_size as f32 - 0.5);
                            let jy = (ty + rng.gen_range(-j..j))
                                .clamp(0.5, self.map_size as f32 - 0.5);
                            (jx, jy)
                        } else if is_f16_kind(&kind) {
                            let j = if hit { 0.22 } else { 0.85 };
                            let jx = (tx + rng.gen_range(-j..j))
                                .clamp(0.5, self.map_size as f32 - 0.5);
                            let jy = (ty + rng.gen_range(-j..j))
                                .clamp(0.5, self.map_size as f32 - 0.5);
                            (jx, jy)
                        } else if hit {
                            (tx, ty)
                        } else {
                            miss_impact(&mut rng, target, fx, fy)
                        };

                        // Return fire when shot at (muzzle flash), hit or miss.
                        if let Some(t) = self.entities.get_mut(&tid) {
                            if hit {
                                t.hp -= dmg;
                                t.last_hit_by = Some(attacker_owner);
                                t.dirty = true;
                            }
                            if t.unit && is_soft_unit(&t.kind) {
                                t.prone_until_tick = t.prone_until_tick.max(self.tick + 30);
                            }
                            if t.unit
                                && t.damage > 0.0
                                && t.move_to.is_none()
                                && t.target.is_none()
                            {
                                let rdx = fx - t.x;
                                let rdy = fy - t.y;
                                let rdist = (rdx * rdx + rdy * rdy).sqrt();
                                if rdist <= t.range {
                                    t.target = Some(from_id);
                                }
                            }
                        }
                        self.shots.push(ShotEvent {
                            from: from_id,
                            to: tid,
                            x0: fx,
                            y0: fy,
                            x1: ix,
                            y1: iy,
                            kind: kind.clone(),
                            hit,
                        });
                        if hit && kind.contains("mlrs") {
                            self.apply_mlrs_blast(team, attacker_owner, fx, fy, ix, iy, tid);
                        } else if hit && is_f16_kind(&kind) {
                            self.apply_f16_bomb_blast(team, attacker_owner, fx, fy, ix, iy, tid);
                        } else if hit && is_air_bomb_kind(&kind) {
                            self.apply_air_bomb_blast(team, attacker_owner, fx, fy, ix, iy, tid);
                        } else if hit && kind.contains("tank") && !kind.contains("mg") {
                            self.apply_shell_blast(team, attacker_owner, fx, fy, tx, ty, tid);
                        } else if hit && kind.contains("mortar") {
                            self.apply_mortar_blast(team, attacker_owner, fx, fy, tx, ty, tid);
                        }
                        if f16_spent {
                            entity.target = None;
                            entity.ability_cooldown_ms = F16_REARM_MS;
                            if let Some(pad) =
                                self.nearest_owned_airfield_pad(attacker_owner, fx, fy)
                            {
                                entity.move_to = Some(pad);
                            }
                            entity.dirty = true;
                        }
                    }
                } else {
                    entity.target = None;
                }
            }

            if entity.kind.contains("tank") {
                if entity.mg_cooldown_ms > 0 {
                    entity.mg_cooldown_ms = entity.mg_cooldown_ms.saturating_sub(dt_ms);
                }
                if entity.mg_cooldown_ms == 0 {
                    if let Some(tid) =
                        self.find_mg_target(entity.id, entity.team, entity.x, entity.y)
                    {
                        self.fire_tank_mg(&mut entity, tid);
                    }
                }
            }

            // Infantry hits the dirt while shooting / being shot at; stand up after the fight.
            if entity.unit && is_soft_unit(&entity.kind) {
                let mut fighting = false;
                if let Some(tid) = entity.target {
                    if let Some(t) = self.entities.get(&tid) {
                        let dx = t.x - entity.x;
                        let dy = t.y - entity.y;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist <= entity.range {
                            let cover = self.shot_cover(entity.id, entity.x, entity.y, t);
                            if !cover.blocked {
                                fighting = true;
                            }
                        }
                    }
                }
                if fighting {
                    entity.prone_until_tick = entity.prone_until_tick.max(self.tick + 28);
                }
                let want = self.tick < entity.prone_until_tick;
                if entity.prone != want {
                    entity.prone = want;
                    entity.dirty = true;
                }
            }

            self.put_entity(entity);
        }

        self.separate_units(dt_ms);
        self.eject_units_from_buildings();
        self.crush_infantry_under_tanks();
        self.clamp_entities_to_map();
        self.tick_special_ops(dt_ms);

        // Remove dead.
        let dead: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.hp <= 0.0)
            .map(|e| e.id)
            .collect();
        for id in dead {
            if let Some(entity) = self.take_entity(id) {
                // Combat credits — killer vs owner losses.
                if let Some(killer) = entity.last_hit_by {
                    if killer != entity.owner {
                        if let Some(p) = self.players.get_mut(&killer) {
                            if entity.building {
                                p.stats.buildings_destroyed =
                                    p.stats.buildings_destroyed.saturating_add(1);
                            } else if entity.unit {
                                credit_unit_kill(&mut p.stats, &entity.kind);
                            }
                        }
                    }
                }
                if let Some(p) = self.players.get_mut(&entity.owner) {
                    if entity.building {
                        p.stats.buildings_lost = p.stats.buildings_lost.saturating_add(1);
                    } else if entity.unit {
                        p.stats.units_lost = p.stats.units_lost.saturating_add(1);
                    }
                }
                self.refund_building_economy(&entity);
                self.removed.push(id);
                if entity.kind == "hq" {
                    self.on_hq_destroyed(entity);
                }
            }
        }

        bots::tick_bots(self);
        self.check_victory();
    }

    /// HQ falls → transfer local garrison to the conqueror (or wipe if none), spawn colony HQ.
    fn on_hq_destroyed(&mut self, hq: Entity) {
        let hx = hq.x;
        let hy = hq.y;
        let former = hq.owner;
        let claim_r2 = CITY_CLAIM_RADIUS * CITY_CLAIM_RADIUS;
        let conqueror = self.resolve_conqueror(&hq);

        let site_buildings: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| {
                e.owner == former
                    && e.building
                    && e.hp > 0.0
                    && e.kind != "hq"
                    && {
                        let dx = e.x - hx;
                        let dy = e.y - hy;
                        dx * dx + dy * dy <= claim_r2
                    }
            })
            .map(|e| e.id)
            .collect();

        if let Some(cid) = conqueror {
            // Real capture: keep the base footprint under the new owner (not a scorched empty pad).
            let (team, flag) = self
                .players
                .get(&cid)
                .map(|p| (p.team, p.flag.clone()))
                .unwrap_or((hq.team, None));
            for bid in site_buildings {
                self.reassign_building(bid, cid, team, flag.clone());
            }
            self.grant_colony_hq(cid, hx, hy);
            self.enforce_budgets_for(cid);
        } else {
            for wid in site_buildings {
                if let Some(e) = self.take_entity(wid) {
                    self.refund_building_economy(&e);
                    self.removed.push(wid);
                }
            }
        }

        if self.players.get(&former).and_then(|p| p.home_hq) == Some(hq.id) {
            if let Some(player) = self.players.get_mut(&former) {
                player.home_hq = None;
            }
            // Promote another living HQ to "home" so H-key still works.
            let fallback = self
                .entities
                .values()
                .find(|e| e.owner == former && e.kind == "hq" && e.hp > 0.0)
                .map(|e| e.id);
            if let Some(player) = self.players.get_mut(&former) {
                player.home_hq = fallback;
            }
        }

        let remaining_hq = self
            .entities
            .values()
            .filter(|e| e.owner == former && e.kind == "hq" && e.hp > 0.0)
            .count();
        if remaining_hq == 0 {
            if let Some(player) = self.players.get_mut(&former) {
                player.alive = false;
                player.colonies = 0;
                player.home_hq = None;
            }
        } else if let Some(player) = self.players.get_mut(&former) {
            player.colonies = remaining_hq.saturating_sub(1) as u32;
        }

        // Cap shrinks with lost HQs — drop queued trains / builds that no longer fit.
        self.enforce_budgets_for(former);
    }

    /// Move a finished / constructing building to a new commander (colony capture).
    fn reassign_building(
        &mut self,
        id: Uuid,
        new_owner: Uuid,
        new_team: u8,
        flag: Option<String>,
    ) {
        let Some(entity) = self.entities.get(&id).cloned() else {
            return;
        };
        if entity.owner == new_owner {
            return;
        }
        self.refund_building_economy(&entity);
        if let Some(e) = self.entities.get_mut(&id) {
            e.owner = new_owner;
            e.team = new_team;
            e.flag = flag;
            e.train_queue.clear();
            e.target = None;
            e.last_hit_by = None;
            e.dirty = true;
        }
        // Credit power economy to the new owner for finished structures.
        if entity.build_remaining_ms == 0 {
            if let Some(def) = buildables().iter().find(|b| b.kind == entity.kind) {
                if let Some(player) = self.players.get_mut(&new_owner) {
                    if def.power > 0 {
                        player.resources.power =
                            player.resources.power.saturating_add(def.power);
                    } else if def.power < 0 {
                        player.resources.power_used =
                            player.resources.power_used.saturating_add(-def.power);
                    }
                }
            }
        }
    }

    fn is_enemy_of(&self, a: Uuid, b_owner: Uuid, b_team: u8) -> bool {
        if a == b_owner {
            return false;
        }
        let Some(ap) = self.players.get(&a) else {
            return false;
        };
        if self.ffa {
            return true;
        }
        ap.team != b_team
    }

    /// Spy sabotage / Hacker cyber / Terrorist revolt — pulsed abilities.
    fn tick_special_ops(&mut self, dt_ms: u32) {
        let ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| {
                e.unit
                    && e.hp > 0.0
                    && matches!(e.kind.as_str(), "spy" | "hacker" | "terrorist")
            })
            .map(|e| e.id)
            .collect();
        for id in ids {
            let Some(mut agent) = self.take_entity(id) else {
                continue;
            };
            agent.ability_cooldown_ms = agent.ability_cooldown_ms.saturating_sub(dt_ms);
            if agent.ability_cooldown_ms > 0 {
                self.put_entity(agent);
                continue;
            }
            let owner = agent.owner;
            let ax = agent.x;
            let ay = agent.y;
            let kind = agent.kind.clone();

            match kind.as_str() {
                "spy" => {
                    // Sabotage nearest finished enemy building in reach.
                    let mut best: Option<(Uuid, f32)> = None;
                    for e in self.entities.values() {
                        if !e.building || e.hp <= 0.0 || e.build_remaining_ms > 0 {
                            continue;
                        }
                        if !self.is_enemy_of(owner, e.owner, e.team) {
                            continue;
                        }
                        let dx = e.x - ax;
                        let dy = e.y - ay;
                        let d2 = dx * dx + dy * dy;
                        if d2 > 2.6 * 2.6 {
                            continue;
                        }
                        if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                            best = Some((e.id, d2));
                        }
                    }
                    if let Some((bid, _)) = best {
                        if let Some(b) = self.entities.get_mut(&bid) {
                            let dmg = if b.kind == "hq" { 35.0 } else { 75.0 };
                            b.hp = (b.hp - dmg).max(0.0);
                            b.last_hit_by = Some(owner);
                            b.dirty = true;
                            agent.ability_cooldown_ms = 2_400;
                            agent.dirty = true;
                        }
                    }
                }
                "hacker" => {
                    // Near own HQ → siphon gold home (passive income while staging).
                    let near_home = self.entities.values().any(|e| {
                        e.owner == owner
                            && e.kind == "hq"
                            && e.hp > 0.0
                            && {
                                let dx = e.x - ax;
                                let dy = e.y - ay;
                                dx * dx + dy * dy <= 6.0 * 6.0
                            }
                    });
                    if near_home {
                        if let Some(p) = self.players.get_mut(&owner) {
                            p.resources.gold = p.resources.gold.saturating_add(10);
                            p.stats.gold_earned = p.stats.gold_earned.saturating_add(10);
                        }
                        agent.ability_cooldown_ms = 1_000;
                        agent.dirty = true;
                    } else {
                        // Inside enemy footprint → hack building + drain power + steal gold.
                        let mut best: Option<(Uuid, Uuid, f32)> = None;
                        for e in self.entities.values() {
                            if !e.building || e.hp <= 0.0 || e.build_remaining_ms > 0 {
                                continue;
                            }
                            if e.kind == "hq" {
                                continue; // CC immune to full blackout
                            }
                            if !self.is_enemy_of(owner, e.owner, e.team) {
                                continue;
                            }
                            let dx = e.x - ax;
                            let dy = e.y - ay;
                            let d2 = dx * dx + dy * dy;
                            if d2 > 2.8 * 2.8 {
                                continue;
                            }
                            if best.map(|(_, _, bd)| d2 < bd).unwrap_or(true) {
                                best = Some((e.id, e.owner, d2));
                            }
                        }
                        if let Some((bid, victim, _)) = best {
                            let steal;
                            if let Some(vp) = self.players.get_mut(&victim) {
                                let take = (vp.resources.gold / 12).clamp(20, 90);
                                steal = take.min(vp.resources.gold.max(0));
                                vp.resources.gold = vp.resources.gold.saturating_sub(steal);
                                vp.resources.power = (vp.resources.power - 14).max(0);
                            } else {
                                steal = 0;
                            }
                            if steal > 0 {
                                if let Some(ap) = self.players.get_mut(&owner) {
                                    ap.resources.gold = ap.resources.gold.saturating_add(steal);
                                    ap.stats.gold_earned =
                                        ap.stats.gold_earned.saturating_add(steal as u32);
                                }
                            }
                            if let Some(b) = self.entities.get_mut(&bid) {
                                b.hacked_until_tick = self.tick + 100; // ~5s
                                b.dirty = true;
                            }
                            agent.ability_cooldown_ms = 2_200;
                            agent.dirty = true;
                        }
                    }
                }
                "terrorist" => {
                    // Convert nearby enemy soft infantry (insurgency / revolt).
                    let mut candidates: Vec<(Uuid, f32)> = Vec::new();
                    for e in self.entities.values() {
                        if !e.unit || e.hp <= 0.0 || !is_soft_unit(&e.kind) {
                            continue;
                        }
                        if e.kind == "terrorist" || e.kind == "spy" || e.kind == "hacker" {
                            continue;
                        }
                        if !self.is_enemy_of(owner, e.owner, e.team) {
                            continue;
                        }
                        let dx = e.x - ax;
                        let dy = e.y - ay;
                        let d2 = dx * dx + dy * dy;
                        if d2 > 3.4 * 3.4 {
                            continue;
                        }
                        candidates.push((e.id, d2));
                    }
                    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                    let (team, flag) = self
                        .players
                        .get(&owner)
                        .map(|p| (p.team, p.flag.clone()))
                        .unwrap_or((agent.team, None));
                    let mut flipped = 0u32;
                    for (cid, _) in candidates.into_iter().take(2) {
                        if let Some(c) = self.entities.get_mut(&cid) {
                            c.owner = owner;
                            c.team = team;
                            c.flag = flag.clone();
                            c.target = None;
                            c.move_to = None;
                            c.last_hit_by = None;
                            c.dirty = true;
                            flipped += 1;
                        }
                    }
                    if flipped > 0 {
                        agent.ability_cooldown_ms = 5_500;
                        agent.dirty = true;
                    }
                }
                _ => {}
            }
            self.put_entity(agent);
        }
    }

    fn is_hostile_commander(&self, attacker: Uuid, victim_owner: Uuid, victim_team: u8) -> bool {
        if attacker == victim_owner {
            return false;
        }
        let Some(ap) = self.players.get(&attacker) else {
            return false;
        };
        // Eliminated commanders may still hold a field army — they must be able
        // to reclaim a Command Center and return to the match.
        if self.ffa {
            return true;
        }
        ap.team != victim_team
    }

    fn resolve_conqueror(&self, hq: &Entity) -> Option<Uuid> {
        if let Some(uid) = hq.last_hit_by {
            if self.is_hostile_commander(uid, hq.owner, hq.team) {
                return Some(uid);
            }
        }
        let claim_r2 = CITY_CLAIM_RADIUS * CITY_CLAIM_RADIUS;
        let mut best: Option<(Uuid, f32)> = None;
        for e in self.entities.values() {
            if e.hp <= 0.0 || !(e.unit || e.building) {
                continue;
            }
            if !self.is_hostile_commander(e.owner, hq.owner, hq.team) {
                continue;
            }
            let dx = e.x - hq.x;
            let dy = e.y - hq.y;
            let d2 = dx * dx + dy * dy;
            if d2 > claim_r2 {
                continue;
            }
            if best.map(|(_, bd)| d2 < bd).unwrap_or(true) {
                best = Some((e.owner, d2));
            }
        }
        best.map(|(o, _)| o)
    }

    /// Spawn a ready HQ for the conqueror on the captured site (colony / forward base).
    /// If the conqueror was eliminated but still has an army, reclaiming an HQ revives them.
    fn grant_colony_hq(&mut self, owner: Uuid, x: f32, y: f32) {
        let Some(player) = self.players.get(&owner) else {
            return;
        };
        let team = player.team;
        let flag = player.flag.clone();
        let was_eliminated = !player.alive;
        let map = self.map_size as f32;
        let fx = x.clamp(1.5, map - 1.5);
        let fy = y.clamp(1.5, map - 1.5);
        let hq_id = Uuid::new_v4();
        self.put_entity(Entity {
            id: hq_id,
            kind: "hq".into(),
            owner,
            team,
            x: fx,
            y: fy,
            hp: 12_000.0,
            max_hp: 12_000.0,
            building: true,
            unit: false,
            flag,
            build_remaining_ms: 0,
            train_queue: VecDeque::new(),
            target: None,
            move_to: None,
            speed: 0.0,
            damage: 0.0,
            range: 0.0,
            attack_cooldown_ms: 0,
            mag_ammo: 0,
            dirty: true,
            stuck_frames: 0,
            detour: None,
            detour_ttl: 0,
            last_escape_ang: 0.0,
            prone: false,
            prone_until_tick: 0,
            aim_yaw: 0.0,
            mg_cooldown_ms: 0,
            last_hit_by: None,
            hacked_until_tick: 0,
            ability_cooldown_ms: 0,
        });
        let hqs = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.kind == "hq" && e.hp > 0.0)
            .count();
        if let Some(player) = self.players.get_mut(&owner) {
            player.alive = true;
            player.colonies = hqs.saturating_sub(1) as u32;
            player.stats.bases_captured = player.stats.bases_captured.saturating_add(1);
            if player.home_hq.is_none() || was_eliminated {
                player.home_hq = Some(hq_id);
            }
            // Captured HQ brings its own base power online.
            player.resources.power = player.resources.power.saturating_add(40);
        }
        self.reveal_vision_for(owner);
    }

    /// Air strike splash — jets drop bombs, attack helos fire rockets.
    fn apply_air_bomb_blast(
        &mut self,
        team: u8,
        attacker: Uuid,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 1.55;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            if e.team == team && e.id != primary {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, e.unit && is_soft_unit(&e.kind)));
            false
        });

        let inx = x - from_x;
        let iny = y - from_y;
        let in_len = (inx * inx + iny * iny).sqrt().max(0.001);
        let iux = inx / in_len;
        let iuy = iny / in_len;

        for (id, dist, is_infantry) in victims {
            if id == primary {
                continue;
            }
            let Some(victim) = self.entities.get(&id) else {
                continue;
            };
            if !is_infantry && self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = 1.0 - (dist / RADIUS).clamp(0.0, 1.0);
            let dmg = if victim.building {
                95.0 + 140.0 * falloff
            } else if is_infantry {
                55.0 + 90.0 * falloff
            } else if is_air_kind(&victim.kind) {
                35.0 * falloff
            } else {
                70.0 + 110.0 * falloff
            };
            if let Some(v) = self.entities.get_mut(&id) {
                v.hp -= dmg;
                v.last_hit_by = Some(attacker);
                v.dirty = true;
                if v.unit && is_soft_unit(&v.kind) {
                    v.prone_until_tick = v.prone_until_tick.max(self.tick + 36);
                }
            }
        }
    }

    /// F-16 Mk84-class strike — large beaten zone sized to erase a tank column.
    fn apply_f16_bomb_blast(
        &mut self,
        team: u8,
        attacker: Uuid,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 3.15;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            if e.team == team && e.id != primary {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, e.unit && is_soft_unit(&e.kind)));
            false
        });

        let inx = x - from_x;
        let iny = y - from_y;
        let in_len = (inx * inx + iny * iny).sqrt().max(0.001);
        let iux = inx / in_len;
        let iuy = iny / in_len;

        for (id, dist, is_infantry) in victims {
            if id == primary {
                continue;
            }
            let Some(victim) = self.entities.get(&id) else {
                continue;
            };
            // Airburst HE — armor in the open gets no cover credit.
            if victim.building && self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = 1.0 - (dist / RADIUS).clamp(0.0, 1.0);
            let t = falloff * falloff;
            let dmg = if victim.building {
                420.0 + 1_650.0 * t
            } else if is_infantry {
                10_000.0
            } else if is_air_kind(&victim.kind) {
                180.0 * falloff
            } else if victim.kind.contains("mlrs") {
                5_200.0 + 4_800.0 * t
            } else {
                // MBT seat: near-center kills; fringe still mission-kills / cripples.
                3_800.0 + 5_400.0 * t
            };
            if let Some(v) = self.entities.get_mut(&id) {
                v.hp -= dmg;
                v.last_hit_by = Some(attacker);
                v.dirty = true;
                if v.unit && is_soft_unit(&v.kind) {
                    v.prone_until_tick = v.prone_until_tick.max(self.tick + 48);
                }
            }
        }
    }

    /// Mortar bomb splash — smaller than tank HE, lethal to nearby infantry.
    fn apply_mortar_blast(
        &mut self,
        team: u8,
        attacker: Uuid,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 0.95;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            let infantry = e.unit && is_soft_unit(&e.kind);
            if !infantry && e.team == team {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, infantry));
            false
        });

        let inx = x - from_x;
        let iny = y - from_y;
        let in_len = (inx * inx + iny * iny).sqrt().max(0.001);
        let iux = inx / in_len;
        let iuy = iny / in_len;

        for (id, dist, is_infantry) in victims {
            if id == primary {
                continue;
            }
            let Some(victim) = self.entities.get(&id) else {
                continue;
            };
            if !is_infantry && self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = (1.0 - dist / RADIUS).clamp(0.0, 1.0);
            let dmg = if is_infantry {
                280.0 * falloff
            } else if self
                .entities
                .get(&id)
                .is_some_and(|e| is_vehicle_kind(&e.kind))
            {
                42.0 * falloff
            } else {
                95.0 * falloff
            };
            if dmg < 1.0 {
                continue;
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.hp -= dmg;
                e.last_hit_by = Some(attacker);
                e.dirty = true;
                if is_infantry {
                    e.prone_until_tick = e.prone_until_tick.max(self.tick + 36);
                }
            }
        }
    }

    /// M270 rocket ripple saturation — wide beaten zone, shreds soft targets, chips armor.
    fn apply_mlrs_blast(
        &mut self,
        team: u8,
        attacker: Uuid,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 1.75;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            let infantry = e.unit && is_soft_unit(&e.kind);
            if !infantry && e.team == team {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, infantry));
            false
        });

        let inx = x - from_x;
        let iny = y - from_y;
        let in_len = (inx * inx + iny * iny).sqrt().max(0.001);
        let iux = inx / in_len;
        let iuy = iny / in_len;

        for (id, dist, is_infantry) in victims {
            if id == primary {
                continue;
            }
            let Some(victim) = self.entities.get(&id) else {
                continue;
            };
            if !is_infantry && self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = (1.0 - dist / RADIUS).clamp(0.0, 1.0);
            let dmg = if is_infantry {
                // DPICM / HE — lethal in the beaten zone.
                10_000.0
            } else if self
                .entities
                .get(&id)
                .is_some_and(|e| e.kind.contains("tank"))
            {
                95.0 * falloff
            } else if self
                .entities
                .get(&id)
                .is_some_and(|e| e.kind.contains("mlrs"))
            {
                220.0 * falloff
            } else {
                // Buildings soak multiple rockets but each still hurts.
                200.0 * falloff
            };
            if dmg < 1.0 {
                continue;
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.hp -= dmg;
                e.last_hit_by = Some(attacker);
                e.dirty = true;
                if is_infantry {
                    e.prone_until_tick = e.prone_until_tick.max(self.tick + 40);
                }
            }
        }
    }

    /// HE blast around a tank shell impact. Primary target already took direct damage.
    /// Anyone close enough to be thrown (matches client knock radius) dies.
    fn apply_shell_blast(
        &mut self,
        team: u8,
        attacker: Uuid,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 1.25;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            let infantry = e.unit && is_soft_unit(&e.kind);
            // Overpressure kills soldiers of every team; armor/buildings stay friendly-fire safe.
            if !infantry && e.team == team {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, infantry));
            false
        });

        let inx = x - from_x;
        let iny = y - from_y;
        let in_len = (inx * inx + iny * iny).sqrt().max(0.001);
        let iux = inx / in_len;
        let iuy = iny / in_len;

        for (id, dist, is_infantry) in victims {
            if id == primary {
                continue; // direct hit already applied
            }
            let Some(victim) = self.entities.get(&id) else {
                continue;
            };
            // Hull can shield a tank/building; soldiers in the burst still die.
            if !is_infantry && self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = (1.0 - dist / RADIUS).clamp(0.0, 1.0);
            let dmg = if is_infantry {
                10_000.0
            } else if self
                .entities
                .get(&id)
                .is_some_and(|e| is_vehicle_kind(&e.kind))
            {
                70.0 * falloff
            } else {
                // Buildings take modest splash
                85.0 * falloff
            };
            if dmg < 1.0 {
                continue;
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.hp -= dmg;
                e.last_hit_by = Some(attacker);
                e.dirty = true;
            }
        }
    }

    /// True if this splash victim is behind the struck tank or a solid wall.
    fn blast_blocked(
        &self,
        primary: Uuid,
        impact_x: f32,
        impact_y: f32,
        iux: f32,
        iuy: f32,
        victim: &Entity,
    ) -> bool {
        let vx = victim.x - impact_x;
        let vy = victim.y - impact_y;
        let vlen = (vx * vx + vy * vy).sqrt().max(0.001);
        // Behind the impact relative to incoming fire (same heading as the shell).
        let behind = (vx / vlen) * iux + (vy / vlen) * iuy > 0.25;
        if behind {
            if let Some(p) = self.entities.get(&primary) {
                if p.kind.contains("tank") || p.building {
                    // Aligns with the hull: the tank/building is a shield.
                    let lateral = (vx * -iuy + vy * iux).abs();
                    let shield = occluder_radius(p).unwrap_or(0.2);
                    if lateral < shield + 0.18 {
                        return true;
                    }
                }
            }
        }
        // Another building between impact and victim.
        let mut wall = false;
        self.grid.for_each_in_aabb(
            impact_x.min(victim.x) - MAX_ENTITY_RADIUS,
            impact_y.min(victim.y) - MAX_ENTITY_RADIUS,
            impact_x.max(victim.x) + MAX_ENTITY_RADIUS,
            impact_y.max(victim.y) + MAX_ENTITY_RADIUS,
            |id| {
                let Some(other) = self.entities.get(&id) else {
                    return false;
                };
                if other.id == primary || other.id == victim.id || other.hp <= 0.0 {
                    return false;
                }
                let Some(r) = occluder_radius(other) else {
                    return false;
                };
                if !other.building {
                    return false;
                }
                let (gap, t) = segment_point_gap(
                    impact_x,
                    impact_y,
                    victim.x,
                    victim.y,
                    other.x,
                    other.y,
                );
                if t > 0.08 && t < 0.92 && gap < r {
                    wall = true;
                    return true;
                }
                false
            },
        );
        wall
    }

    /// Line of fire from (ax,ay) to `target`. Buildings/tanks block; hugging a corner is a peek.
    fn shot_cover(&self, from_id: Uuid, ax: f32, ay: f32, target: &Entity) -> ShotCover {
        let tx = target.x;
        let ty = target.y;
        let dx = tx - ax;
        let dy = ty - ay;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 0.02 {
            return ShotCover {
                blocked: false,
                exposure: 1.0,
            };
        }
        let ux = dx / dist;
        let uy = dy / dist;

        let mut blocked = false;
        let mut exposure = 1.0;
        let mut used_cover = false;

        self.grid.for_each_in_aabb(
            ax.min(tx) - MAX_ENTITY_RADIUS,
            ay.min(ty) - MAX_ENTITY_RADIUS,
            ax.max(tx) + MAX_ENTITY_RADIUS,
            ay.max(ty) + MAX_ENTITY_RADIUS,
            |id| {
                let Some(other) = self.entities.get(&id) else {
                    return false;
                };
                if other.id == from_id || other.id == target.id || other.hp <= 0.0 {
                    return false;
                }
                let Some(r) = occluder_radius(other) else {
                    return false;
                };

                let ocx = other.x - ax;
                let ocy = other.y - ay;
                // Cover behind the shooter does not sit on the firing line.
                if ocx * ux + ocy * uy < -0.05 {
                    return false;
                }

                let (gap, t) = segment_point_gap(ax, ay, tx, ty, other.x, other.y);

                // Shooter peeking around this same wall — don't eat their own muzzle.
                let shooter_hug = {
                    let sdx = other.x - ax;
                    let sdy = other.y - ay;
                    (sdx * sdx + sdy * sdy).sqrt() < r + 0.55
                };
                if t < 0.14 && shooter_hug {
                    return false;
                }

                let tdx = other.x - tx;
                let tdy = other.y - ty;
                let hug = (tdx * tdx + tdy * tdy).sqrt();
                let hugging_target = !target.building && hug < r + 0.5;

                if t > 0.06 && t < 0.97 && gap < r {
                    if hugging_target && gap > r * 0.62 {
                        // Ray clips the edge — peeking a corner, not fully behind.
                        exposure *= 0.22;
                        used_cover = true;
                    } else {
                        blocked = true;
                        return true;
                    }
                } else if hugging_target {
                    // Cover sits in front of the target even if the ray just misses the hull.
                    let to_cover_x = other.x - tx;
                    let to_cover_y = other.y - ty;
                    let clen = (to_cover_x * to_cover_x + to_cover_y * to_cover_y)
                        .sqrt()
                        .max(0.001);
                    // From the target, is this cover toward the shooter?
                    let toward_shooter = (-ux) * (to_cover_x / clen) + (-uy) * (to_cover_y / clen);
                    if toward_shooter > 0.2 && gap < r + 0.28 {
                        let peek = ((gap - r).max(0.0) / 0.28).clamp(0.0, 1.0);
                        // peek=0 almost behind; peek=1 just using nearby cover.
                        exposure *= 0.18 + peek * 0.42;
                        used_cover = true;
                    }
                }
                false
            },
        );

        if blocked {
            return ShotCover {
                blocked: true,
                exposure: 0.0,
            };
        }
        if !used_cover && !target.building {
            // Open ground — full silhouette, easier to hit.
            exposure = 1.28;
        }
        ShotCover {
            blocked: false,
            exposure,
        }
    }

    /// Armed buildings (Patriot / bunker / gatling / stinger) engage while finished.
    fn tick_armed_building(&mut self, entity: &mut Entity, dt_ms: u32) {
        match entity.kind.as_str() {
            "bunker" | "tunnel_network" | "gatling_cannon" | "firebase" => {
                self.tick_bunker(entity, dt_ms);
            }
            "turret" | "stinger_site" => self.tick_patriot(entity, dt_ms),
            _ => {
                if entity.damage > 0.0 && entity.range > 0.0 {
                    self.tick_patriot(entity, dt_ms);
                }
            }
        }
    }

    /// Pillbox: idle MG sweep, slew onto contact, burst fire when aimed.
    fn tick_bunker(&mut self, entity: &mut Entity, dt_ms: u32) {
        if entity.attack_cooldown_ms > 0 {
            entity.attack_cooldown_ms = entity.attack_cooldown_ms.saturating_sub(dt_ms);
        }
        let dt = dt_ms as f32 / 1000.0;

        if self.tick % 2 == 0 {
            let stale = match entity.target {
                None => true,
                Some(tid) => match self.entities.get(&tid) {
                    None => true,
                    Some(t) => {
                        if t.hp <= 0.0 || t.team == entity.team {
                            true
                        } else {
                            let dx = t.x - entity.x;
                            let dy = t.y - entity.y;
                            (dx * dx + dy * dy).sqrt() > entity.range
                        }
                    }
                },
            };
            if stale {
                entity.target = self.find_enemy_in_range(
                    entity.id,
                    entity.team,
                    entity.x,
                    entity.y,
                    entity.range,
                );
            }
        }

        let Some(tid) = entity.target else {
            entity.aim_yaw += BUNKER_SCAN_RATE * dt;
            if entity.aim_yaw > std::f32::consts::PI {
                entity.aim_yaw -= std::f32::consts::TAU;
            }
            if self.tick % 4 == 0 {
                entity.dirty = true;
            }
            return;
        };
        let Some(target) = self.entities.get(&tid) else {
            entity.target = None;
            return;
        };
        let dx = target.x - entity.x;
        let dy = target.y - entity.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > entity.range {
            entity.target = None;
            return;
        }

        let mut aimed = true;
        if dist > 0.001 {
            let desired = world_aim_yaw(dx, dy);
            let err = shortest_angle(entity.aim_yaw, desired);
            let step = BUNKER_SLEW_RATE * dt;
            entity.aim_yaw += err.clamp(-step, step);
            entity.dirty = true;
            aimed = err.abs() <= BUNKER_AIM_ALIGN;
        }

        if !aimed || entity.attack_cooldown_ms > 0 {
            return;
        }

        let cover = self.shot_cover(entity.id, entity.x, entity.y, target);
        if cover.blocked {
            return;
        }

        let kind = entity.kind.as_str();
        let uses_mag = matches!(kind, "bunker" | "tunnel_network" | "gatling_cannon");
        if uses_mag {
            let mag_size = if kind == "gatling_cannon" { 60u8 } else { BUNKER_MAG };
            if entity.mag_ammo == 0 {
                entity.mag_ammo = mag_size;
            }
            entity.mag_ammo = entity.mag_ammo.saturating_sub(1);
            if entity.mag_ammo == 0 {
                entity.attack_cooldown_ms = if kind == "gatling_cannon" {
                    700
                } else {
                    BUNKER_RELOAD_MS
                };
                entity.mag_ammo = mag_size;
            } else {
                entity.attack_cooldown_ms = attack_cooldown_for(kind);
            }
        } else {
            entity.attack_cooldown_ms = attack_cooldown_for(kind);
        }
        entity.dirty = true;

        let dmg = hit_damage(kind, target, entity.damage);
        let tx = target.x;
        let ty = target.y;
        let fx = entity.x;
        let fy = entity.y;
        let from_id = entity.id;
        let attacker_owner = entity.owner;
        let hit_p = shot_hit_chance(kind, target, dist, entity.range, cover.exposure);
        let mut rng = rand::thread_rng();
        let hit_floor = if kind == "firebase" { 0.45 } else { 0.35 };
        let hit_ceil = if kind == "firebase" { 0.88 } else { 0.82 };
        let hit = rng.gen_range(0.0..1.0) < hit_p.max(hit_floor).min(hit_ceil);
        let (ix, iy) = if hit {
            (tx, ty)
        } else {
            miss_impact(&mut rng, target, fx, fy)
        };

        if let Some(t) = self.entities.get_mut(&tid) {
            if hit {
                t.hp -= dmg;
                t.last_hit_by = Some(attacker_owner);
                t.dirty = true;
            }
            if t.unit && is_soft_unit(&t.kind) {
                t.prone_until_tick = t.prone_until_tick.max(self.tick + 18);
            }
        }

        let shot_kind = match kind {
            "firebase" => "firebase_shell",
            "gatling_cannon" => "gatling",
            _ => "bunker_mg",
        };
        self.shots.push(ShotEvent {
            from: from_id,
            to: tid,
            x0: fx,
            y0: fy,
            x1: ix,
            y1: iy,
            kind: shot_kind.into(),
            hit,
        });
    }

    /// Patriot Battery: idle radar/launcher scan; slew onto contact; fire only when aimed.
    fn tick_patriot(&mut self, entity: &mut Entity, dt_ms: u32) {
        if entity.attack_cooldown_ms > 0 {
            entity.attack_cooldown_ms = entity.attack_cooldown_ms.saturating_sub(dt_ms);
        }
        let dt = dt_ms as f32 / 1000.0;

        if self.tick % 2 == 0 {
            let stale = match entity.target {
                None => true,
                Some(tid) => match self.entities.get(&tid) {
                    None => true,
                    Some(t) => {
                        if t.hp <= 0.0 || t.team == entity.team {
                            true
                        } else {
                            let dx = t.x - entity.x;
                            let dy = t.y - entity.y;
                            (dx * dx + dy * dy).sqrt() > entity.range
                        }
                    }
                },
            };
            if stale {
                entity.target =
                    self.find_enemy_in_range(entity.id, entity.team, entity.x, entity.y, entity.range);
            }
        }

        // No contact — keep the launcher sweeping like a search radar.
        let Some(tid) = entity.target else {
            entity.aim_yaw += PATRIOT_SCAN_RATE * dt;
            if entity.aim_yaw > std::f32::consts::PI {
                entity.aim_yaw -= std::f32::consts::TAU;
            }
            // ~5 Hz dirty so clients see the sweep without flooding every tick.
            if self.tick % 4 == 0 {
                entity.dirty = true;
            }
            return;
        };
        let Some(target) = self.entities.get(&tid) else {
            entity.target = None;
            return;
        };
        let dx = target.x - entity.x;
        let dy = target.y - entity.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > entity.range {
            entity.target = None;
            return;
        }

        let mut aimed = true;
        if dist > 0.001 {
            let desired = world_aim_yaw(dx, dy);
            let err = shortest_angle(entity.aim_yaw, desired);
            let step = PATRIOT_SLEW_RATE * dt;
            entity.aim_yaw += err.clamp(-step, step);
            entity.dirty = true;
            aimed = err.abs() <= PATRIOT_AIM_ALIGN;
        }

        // Tank-style: do not commit a missile until the tubes face the contact.
        if !aimed || entity.attack_cooldown_ms > 0 {
            return;
        }

        let cover = self.shot_cover(entity.id, entity.x, entity.y, target);
        if cover.blocked {
            return;
        }

        entity.attack_cooldown_ms = attack_cooldown_for(&entity.kind);
        entity.dirty = true;
        let dmg = hit_damage(&entity.kind, target, entity.damage);
        let tx = target.x;
        let ty = target.y;
        let fx = entity.x;
        let fy = entity.y;
        let from_id = entity.id;
        let attacker_owner = entity.owner;
        let hit_p = shot_hit_chance(&entity.kind, target, dist, entity.range, cover.exposure);
        let mut rng = rand::thread_rng();
        let hit = rng.gen_range(0.0..1.0) < hit_p.max(0.72);
        let (ix, iy) = if hit {
            (tx, ty)
        } else {
            miss_impact(&mut rng, target, fx, fy)
        };

        if let Some(t) = self.entities.get_mut(&tid) {
            if hit {
                t.hp -= dmg;
                t.last_hit_by = Some(attacker_owner);
                t.dirty = true;
            }
            if t.unit && is_soft_unit(&t.kind) {
                t.prone_until_tick = t.prone_until_tick.max(self.tick + 24);
            }
        }

        self.shots.push(ShotEvent {
            from: from_id,
            to: tid,
            x0: fx,
            y0: fy,
            x1: ix,
            y1: iy,
            kind: "patriot_missile".into(),
            hit,
        });
    }

    /// Nearest living enemy unit or building inside `range` of (x, y).
    /// Prefers combat units that actually have a firing angle; skips targets fully behind walls.
    fn find_enemy_in_range(
        &self,
        from_id: Uuid,
        team: u8,
        x: f32,
        y: f32,
        range: f32,
    ) -> Option<Uuid> {
        let mut best_unit: Option<(Uuid, f32)> = None;
        let mut best_building: Option<(Uuid, f32)> = None;
        let mut candidates: Vec<Uuid> = Vec::new();
        self.grid.for_each_nearby(x, y, range + MAX_ENTITY_RADIUS, |id| {
            candidates.push(id);
            false
        });
        for id in candidates {
            let Some(other) = self.entities.get(&id) else {
                continue;
            };
            if other.team == team || other.hp <= 0.0 {
                continue;
            }
            if is_stealth_specialist(&other.kind) {
                continue;
            }
            let dx = other.x - x;
            let dy = other.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if other.unit {
                if other.damage <= 0.0 {
                    continue;
                }
                if dist > range {
                    continue;
                }
                let cover = self.shot_cover(from_id, x, y, other);
                if cover.blocked {
                    continue;
                }
                // Prefer nearer + more exposed (don't lock a peeker when an open target exists).
                let score = dist / (0.28 + cover.exposure);
                if best_unit.map(|(_, s)| score < s).unwrap_or(true) {
                    best_unit = Some((other.id, score));
                }
            } else if other.building {
                // Reach the building footprint, not only its center.
                let edge = (dist - building_radius(&other.kind) * 0.55).max(0.0);
                if edge <= range && best_building.map(|(_, d)| dist < d).unwrap_or(true) {
                    let cover = self.shot_cover(from_id, x, y, other);
                    if cover.blocked {
                        continue;
                    }
                    best_building = Some((other.id, dist));
                }
            }
        }
        best_unit.or(best_building).map(|(id, _)| id)
    }

    /// Cupola MG: prefer nearby infantry, then tanks. Independent of the main-gun target.
    fn find_mg_target(&self, from_id: Uuid, team: u8, x: f32, y: f32) -> Option<Uuid> {
        let mut best_inf: Option<(Uuid, f32)> = None;
        let mut best_tank: Option<(Uuid, f32)> = None;
        let mut ids: Vec<Uuid> = Vec::new();
        self.grid
            .for_each_nearby(x, y, TANK_MG_RANGE + MAX_ENTITY_RADIUS, |id| {
                ids.push(id);
                false
            });
        for id in ids {
            if id == from_id {
                continue;
            }
            let Some(other) = self.entities.get(&id) else {
                continue;
            };
            if other.team == team || other.hp <= 0.0 || !other.unit {
                continue;
            }
            if is_stealth_specialist(&other.kind) {
                continue;
            }
            let dx = other.x - x;
            let dy = other.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > TANK_MG_RANGE {
                continue;
            }
            let cover = self.shot_cover(from_id, x, y, other);
            if cover.blocked {
                continue;
            }
            let score = dist / (0.22 + cover.exposure);
            if other.kind.contains("tank") {
                if best_tank.map(|(_, s)| score < s).unwrap_or(true) {
                    best_tank = Some((other.id, score));
                }
            } else if best_inf.map(|(_, s)| score < s).unwrap_or(true) {
                best_inf = Some((other.id, score));
            }
        }
        best_inf.or(best_tank).map(|(id, _)| id)
    }

    fn fire_tank_mg(&mut self, entity: &mut Entity, tid: Uuid) {
        let Some(target) = self.entities.get(&tid) else {
            return;
        };
        let dx = target.x - entity.x;
        let dy = target.y - entity.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let cover = self.shot_cover(entity.id, entity.x, entity.y, target);
        if cover.blocked || dist > TANK_MG_RANGE {
            return;
        }
        let infantry = target.unit && is_soft_unit(&target.kind);
        let tank = is_vehicle_kind(&target.kind);
        let dmg = if infantry {
            48.0
        } else if tank {
            22.0
        } else {
            10.0
        };
        let hit_p = (0.86 / (1.0 + (dist / (TANK_MG_RANGE * 0.45)).powi(2))
            * cover.exposure.clamp(0.2, 1.35)
            * if tank { 0.72 } else { 1.05 })
        .clamp(0.08, 0.92);
        let mut rng = rand::thread_rng();
        let hit = rng.gen_range(0.0..1.0) < hit_p;
        let fx = entity.x;
        let fy = entity.y;
        let (ix, iy) = if hit {
            (target.x, target.y)
        } else {
            miss_impact(&mut rng, target, fx, fy)
        };
        if hit {
            if let Some(t) = self.entities.get_mut(&tid) {
                t.hp -= dmg;
                t.last_hit_by = Some(entity.owner);
                t.dirty = true;
                if t.unit && is_soft_unit(&t.kind) {
                    t.prone_until_tick = t.prone_until_tick.max(self.tick + 18);
                }
            }
        }
        entity.mg_cooldown_ms = TANK_MG_COOLDOWN_MS;
        entity.dirty = true;
        self.shots.push(ShotEvent {
            from: entity.id,
            to: tid,
            x0: fx,
            y0: fy,
            x1: ix,
            y1: iy,
            kind: "tank_mg".into(),
            hit,
        });
    }

    /// How close an enemy structure may sit before the tile is "their territory".
    fn enemy_build_block_radius(kind: &str) -> f32 {
        match kind {
            "hq" => 14.0,
            "war_factory" | "barracks" => 10.0,
            _ => 8.0,
        }
    }

    fn water_blocks(&self, x: f32, y: f32, radius: f32) -> bool {
        for p in &self.ponds {
            let dx = p.x - x;
            let dy = p.y - y;
            let min = p.r + radius;
            if dx * dx + dy * dy < min * min {
                return true;
            }
        }
        false
    }

    fn mountain_blocks(&self, x: f32, y: f32, radius: f32) -> bool {
        for m in &self.mountains {
            let dx = m.x - x;
            let dy = m.y - y;
            let min = m.r + radius;
            if dx * dx + dy * dy < min * min {
                return true;
            }
        }
        false
    }

    /// Lakes + mountains — ground movement and placement.
    fn ground_blocks(&self, x: f32, y: f32, radius: f32) -> bool {
        self.water_blocks(x, y, radius) || self.mountain_blocks(x, y, radius)
    }

    fn collides_at(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        self_r: f32,
        ignore: Option<Uuid>,
        solid_units: bool,
    ) -> bool {
        self.collides_at_ex(self_id, x, y, self_r, ignore, solid_units, None, None)
    }

    /// `overrun_team`: tank of this team ignores enemy infantry (runs them over).
    /// `pass_allies`: skip same-team units (marching squads must not block themselves).
    fn collides_at_ex(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        self_r: f32,
        ignore: Option<Uuid>,
        solid_units: bool,
        overrun_team: Option<u8>,
        pass_allies: Option<u8>,
    ) -> bool {
        if self.ground_blocks(x, y, self_r) {
            return true;
        }
        let mut hit = false;
        self.grid.for_each_nearby(
            x,
            y,
            self_r + MAX_ENTITY_RADIUS + collision_pad(),
            |id| {
                let Some(other) = self.entities.get(&id) else {
                    return false;
                };
                if other.id == self_id || Some(other.id) == ignore {
                    return false;
                }
                if !other.building && !other.unit {
                    return false;
                }
                if other.unit {
                    if pass_allies == Some(other.team) {
                        return false;
                    }
                    if let Some(team) = overrun_team {
                        // Tanks drive through enemy infantry; still blocked by enemy tanks.
                        if other.team != team && !other.kind.contains("tank") {
                            return false;
                        }
                    }
                    if !solid_units {
                        return false;
                    }
                }
                // Under-construction buildings still block.
                let other_r = entity_radius(other);
                let min_d = self_r + other_r + collision_pad();
                let dx = other.x - x;
                let dy = other.y - y;
                if dx * dx + dy * dy < min_d * min_d {
                    hit = true;
                    return true;
                }
                false
            },
        );
        hit
    }

    fn steer_step_ex(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        ux: f32,
        uy: f32,
        step: f32,
        self_r: f32,
        ignore: Option<Uuid>,
        solid_units: bool,
        overrun_team: Option<u8>,
        pass_allies: Option<u8>,
    ) -> Option<(f32, f32)> {
        // Try forward, then fan left/right to walk around obstacles.
        const ANGLES: &[f32] = &[
            0.0, 0.4, -0.4, 0.85, -0.85, 1.3, -1.3, 1.75, -1.75, 2.3, -2.3, 2.8, -2.8,
        ];
        for &ang in ANGLES {
            let (s, c) = ang.sin_cos();
            let dx = ux * c - uy * s;
            let dy = ux * s + uy * c;
            let nx = x + dx * step;
            let ny = y + dy * step;
            if !self.collides_at_ex(
                self_id,
                nx,
                ny,
                self_r,
                ignore,
                solid_units,
                overrun_team,
                pass_allies,
            ) {
                return Some((nx, ny));
            }
        }
        None
    }

    fn steer_step_escape_ex(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        ux: f32,
        uy: f32,
        step: f32,
        self_r: f32,
        ignore: Option<Uuid>,
        last_escape_ang: f32,
        overrun_team: Option<u8>,
        pass_allies: Option<u8>,
    ) -> Option<((f32, f32), f32)> {
        let mut rng = rand::thread_rng();
        // Soft on units so crowds don't freeze everyone.
        if let Some(p) = self.steer_step_ex(
            self_id,
            x,
            y,
            ux,
            uy,
            step,
            self_r,
            ignore,
            false,
            overrun_team,
            pass_allies,
        ) {
            return Some((p, last_escape_ang));
        }

        // Random absolute headings — skip near the last failed escape.
        for _ in 0..14 {
            let ang = rng.gen_range(0.0..std::f32::consts::TAU);
            let delta = (ang - last_escape_ang).rem_euclid(std::f32::consts::TAU);
            let mirrored = std::f32::consts::TAU - delta;
            if delta.min(mirrored) < 0.55 {
                continue;
            }
            let nx = x + ang.cos() * step;
            let ny = y + ang.sin() * step;
            if !self.collides_at_ex(self_id, nx, ny, self_r, ignore, false, overrun_team, pass_allies)
            {
                return Some(((nx, ny), ang));
            }
            // Also try a longer probe step for escaping pockets.
            let nx2 = x + ang.cos() * step * 1.6;
            let ny2 = y + ang.sin() * step * 1.6;
            if !self.collides_at_ex(
                self_id,
                nx2,
                ny2,
                self_r,
                ignore,
                false,
                overrun_team,
                pass_allies,
            ) {
                return Some(((nx2, ny2), ang));
            }
        }

        // Last resort: tiny lateral slides.
        for side in [-1.0f32, 1.0] {
            let nx = x - uy * side * step * 0.85;
            let ny = y + ux * side * step * 0.85;
            if !self.collides_at_ex(self_id, nx, ny, self_r, ignore, false, overrun_team, pass_allies)
            {
                return Some(((nx, ny), last_escape_ang + side));
            }
        }
        // Last resort: any free micro-step, even repeating angles.
        for k in 0..12 {
            let ang = (k as f32) * (std::f32::consts::TAU / 12.0) + rng.gen_range(0.0..0.3);
            let nx = x + ang.cos() * step;
            let ny = y + ang.sin() * step;
            if !self.collides_at_ex(self_id, nx, ny, self_r, ignore, false, overrun_team, pass_allies)
            {
                return Some(((nx, ny), ang));
            }
        }
        None
    }

    fn pick_escape_waypoint(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        goal_x: f32,
        goal_y: f32,
        self_r: f32,
        last_escape_ang: f32,
    ) -> ((f32, f32), f32) {
        let mut rng = rand::thread_rng();
        let map = self.map_size as f32;
        let to_goal = (goal_y - y).atan2(goal_x - x);

        for _ in 0..20 {
            // Prefer side/back detours relative to the goal, not the same heading again.
            let side = if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
            let ang = to_goal
                + side * rng.gen_range(0.9..2.4)
                + rng.gen_range(-0.35..0.35);
            let delta = (ang - last_escape_ang).rem_euclid(std::f32::consts::TAU);
            let mirrored = std::f32::consts::TAU - delta;
            if delta.min(mirrored) < 0.7 && rng.gen_bool(0.7) {
                continue;
            }
            let dist = rng.gen_range(2.5..7.5);
            let wx = (x + ang.cos() * dist).clamp(0.5, map - 0.5);
            let wy = (y + ang.sin() * dist).clamp(0.5, map - 0.5);
            // Prefer waypoints that aren't inside buildings.
            if !self.collides_at(self_id, wx, wy, self_r, None, false) {
                return ((wx, wy), ang);
            }
        }

        let ang = last_escape_ang + std::f32::consts::FRAC_PI_2 + rng.gen_range(-0.4..0.4);
        let wx = (x + ang.cos() * 4.0).clamp(0.5, map - 0.5);
        let wy = (y + ang.sin() * 4.0).clamp(0.5, map - 0.5);
        ((wx, wy), ang)
    }

    fn find_free_spawn_near(
        &self,
        bx: f32,
        by: f32,
        radius: f32,
        self_id: Uuid,
        extra_solid: Option<(f32, f32, f32)>,
    ) -> (f32, f32) {
        let map = self.map_size as f32;
        let base = extra_solid
            .map(|(_, _, er)| er + radius + collision_pad() + 0.12)
            .unwrap_or(radius + 0.45);
        for k in 0..240 {
            let ang = k as f32 * 0.53;
            let dist = base + (k as f32) * 0.1;
            let x = (bx + ang.cos() * dist).clamp(0.5, map - 0.5);
            let y = (by + ang.sin() * dist).clamp(0.5, map - 0.5);
            if let Some((ex, ey, er)) = extra_solid {
                let dx = ex - x;
                let dy = ey - y;
                let min_d = radius + er + collision_pad() + 0.04;
                if dx * dx + dy * dy < min_d * min_d {
                    continue;
                }
            }
            if !self.collides_at(self_id, x, y, radius, None, true) {
                return (x, y);
            }
        }
        // Never return an unchecked fallback — walk outward until clear.
        self.resolve_clear_spawn(bx, by, radius, self_id, extra_solid)
    }

    /// Guaranteed clear pad near (bx,by); expands until a free ring is found.
    fn resolve_clear_spawn(
        &self,
        bx: f32,
        by: f32,
        radius: f32,
        self_id: Uuid,
        extra_solid: Option<(f32, f32, f32)>,
    ) -> (f32, f32) {
        let map = self.map_size as f32;
        for ring in 0..48 {
            let dist = 0.6 + ring as f32 * 0.35;
            let spokes = 10 + ring * 2;
            for s in 0..spokes {
                let ang = (s as f32) * (std::f32::consts::TAU / spokes as f32);
                let x = (bx + ang.cos() * dist).clamp(0.5, map - 0.5);
                let y = (by + ang.sin() * dist).clamp(0.5, map - 0.5);
                if let Some((ex, ey, er)) = extra_solid {
                    let dx = ex - x;
                    let dy = ey - y;
                    let min_d = radius + er + collision_pad() + 0.04;
                    if dx * dx + dy * dy < min_d * min_d {
                        continue;
                    }
                }
                if !self.collides_at(self_id, x, y, radius, None, true) {
                    return (x, y);
                }
            }
        }
        (
            (bx + 4.0).clamp(0.5, map - 0.5),
            (by + 4.0).clamp(0.5, map - 0.5),
        )
    }

    /// Overlapping finished/under-construction building, if any.
    fn building_overlap(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        self_r: f32,
    ) -> Option<(f32, f32, f32)> {
        let mut best: Option<(f32, f32, f32, f32)> = None; // bx,by,br,penetration
        self.grid.for_each_nearby(
            x,
            y,
            self_r + MAX_ENTITY_RADIUS + collision_pad(),
            |id| {
                let Some(e) = self.entities.get(&id) else {
                    return false;
                };
                if e.id == self_id || !e.building || e.hp <= 0.0 {
                    return false;
                }
                let br = building_radius(&e.kind);
                let dx = x - e.x;
                let dy = y - e.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let min_d = self_r + br + collision_pad();
                if dist < min_d {
                    let pen = min_d - dist;
                    if best.map(|(_, _, _, p)| pen > p).unwrap_or(true) {
                        best = Some((e.x, e.y, br, pen));
                    }
                }
                false
            },
        );
        best.map(|(bx, by, br, _)| (bx, by, br))
    }

    fn clear_point_from_building(
        &self,
        self_id: Uuid,
        x: f32,
        y: f32,
        self_r: f32,
        bx: f32,
        by: f32,
        br: f32,
    ) -> (f32, f32) {
        let map = self.map_size as f32;
        let need = br + self_r + collision_pad() + 0.1;
        let dx = x - bx;
        let dy = y - by;
        let dist = (dx * dx + dy * dy).sqrt();
        let (mut ox, mut oy) = if dist < 1e-3 {
            (bx + need, by)
        } else {
            (bx + dx / dist * need, by + dy / dist * need)
        };
        ox = ox.clamp(0.5, map - 0.5);
        oy = oy.clamp(0.5, map - 0.5);
        if !self.collides_at(self_id, ox, oy, self_r, None, true) {
            return (ox, oy);
        }
        // Fan around the building rim until free.
        for k in 0..24 {
            let ang = k as f32 * (std::f32::consts::TAU / 24.0);
            let px = (bx + ang.cos() * need).clamp(0.5, map - 0.5);
            let py = (by + ang.sin() * need).clamp(0.5, map - 0.5);
            if !self.collides_at(self_id, px, py, self_r, None, true) {
                return (px, py);
            }
        }
        self.resolve_clear_spawn(bx, by, self_r, self_id, Some((bx, by, br)))
    }

    /// Units planted under a new building (or bad spawn) get shoved to the rim.
    fn eject_units_from_buildings(&mut self) {
        let trapped: Vec<(Uuid, f32)> = self
            .entities
            .values()
            .filter(|e| e.unit && e.hp > 0.0 && !is_air_kind(&e.kind))
            .filter(|e| {
                self.building_overlap(e.id, e.x, e.y, unit_radius(&e.kind))
                    .is_some()
            })
            .map(|e| (e.id, unit_radius(&e.kind)))
            .collect();
        for (id, self_r) in trapped {
            let Some(entity) = self.entities.get(&id) else {
                continue;
            };
            let x = entity.x;
            let y = entity.y;
            let Some((bx, by, br)) = self.building_overlap(id, x, y, self_r) else {
                continue;
            };
            let (nx, ny) = self.clear_point_from_building(id, x, y, self_r, bx, by, br);
            if let Some(entity) = self.entities.get_mut(&id) {
                entity.x = nx;
                entity.y = ny;
                entity.stuck_frames = 0;
                entity.dirty = true;
            }
            self.grid.upsert(id, nx, ny);
        }
    }

    /// Pad beside an airfield — ignores ground buildings (unit is airborne on spawn).
    fn find_air_spawn_near(
        &self,
        bx: f32,
        by: f32,
        radius: f32,
        self_id: Uuid,
    ) -> (f32, f32) {
        let map = self.map_size as f32;
        for k in 0..64 {
            let ang = k as f32 * 0.65;
            let dist = 1.8 + (k as f32) * 0.11;
            let x = (bx + ang.cos() * dist).clamp(0.5, map - 0.5);
            let y = (by + ang.sin() * dist).clamp(0.5, map - 0.5);
            let mut blocked = false;
            self.grid.for_each_nearby(x, y, radius + 0.35, |id| {
                let Some(e) = self.entities.get(&id) else {
                    return false;
                };
                if e.id == self_id || e.hp <= 0.0 || !e.unit || !is_air_kind(&e.kind) {
                    return false;
                }
                let dx = e.x - x;
                let dy = e.y - y;
                let min_d = radius + unit_radius(&e.kind) + 0.15;
                if dx * dx + dy * dy < min_d * min_d {
                    blocked = true;
                    return true;
                }
                false
            });
            if !blocked {
                return (x, y);
            }
        }
        (
            (bx + 2.2).clamp(0.5, map - 0.5),
            by.clamp(0.5, map - 0.5),
        )
    }

    /// Tanks flatten enemy infantry they drive over.
    fn crush_infantry_under_tanks(&mut self) {
        let tanks: Vec<(Uuid, u8, f32, f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.unit && e.hp > 0.0 && e.kind.contains("tank"))
            .map(|e| (e.id, e.team, e.x, e.y, unit_radius(&e.kind)))
            .collect();

        let mut crushed: Vec<Uuid> = Vec::new();
        for &(_tid, team, tx, ty, tr) in &tanks {
            let query_r = tr * 0.92 + MAX_UNIT_RADIUS * 0.25;
            self.grid.for_each_nearby(tx, ty, query_r, |id| {
                let Some(other) = self.entities.get(&id) else {
                    return false;
                };
                if !other.unit
                    || other.hp <= 0.0
                    || other.team == team
                    || is_vehicle_kind(&other.kind)
                {
                    return false;
                }
                let dx = other.x - tx;
                let dy = other.y - ty;
                // Must be under the hull footprint, not just nearby.
                let crush_r = tr * 0.92 + unit_radius(&other.kind) * 0.25;
                if dx * dx + dy * dy <= crush_r * crush_r {
                    crushed.push(other.id);
                }
                false
            });
        }
        crushed.sort_unstable();
        crushed.dedup();
        for id in crushed {
            if let Some(e) = self.entities.get_mut(&id) {
                e.hp = 0.0;
                e.dirty = true;
                e.move_to = None;
                e.target = None;
            }
        }
    }

    fn separate_units(&mut self, dt_ms: u32) {
        // Neighbor-only separation; every other tick is enough visually.
        if self.tick % 2 == 1 {
            return;
        }
        let ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.unit && !is_air_kind(&e.kind))
            .map(|e| e.id)
            .collect();
        if ids.len() < 2 {
            return;
        }
        // Compensate for half-rate so push strength stays similar.
        let strength = 2.8 * (dt_ms as f32 / 1000.0) * 2.0;
        let mut pushes: HashMap<Uuid, (f32, f32)> = HashMap::new();

        for &a_id in &ids {
            let Some(a) = self.entities.get(&a_id) else {
                continue;
            };
            let ar = unit_radius(&a.kind);
            let ax = a.x;
            let ay = a.y;
            let a_tank = is_vehicle_kind(&a.kind);
            let a_team = a.team;
            self.grid.for_each_nearby(
                ax,
                ay,
                ar + MAX_UNIT_RADIUS + collision_pad(),
                |b_id| {
                    if b_id <= a_id {
                        return false;
                    }
                    let Some(b) = self.entities.get(&b_id) else {
                        return false;
                    };
                    if !b.unit {
                        return false;
                    }
                    // Don't push tanks off the infantry they are crushing.
                    let b_tank = is_vehicle_kind(&b.kind);
                    if a_tank && !b_tank && a_team != b.team {
                        return false;
                    }
                    if b_tank && !a_tank && a_team != b.team {
                        return false;
                    }
                    let br = unit_radius(&b.kind);
                    let dx = ax - b.x;
                    let dy = ay - b.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let min_d = ar + br + collision_pad();
                    if dist >= min_d || dist < 1e-4 {
                        return false;
                    }
                    let push = (min_d - dist) * 0.5;
                    let nx = dx / dist;
                    let ny = dy / dist;
                    let pa = pushes.entry(a_id).or_insert((0.0, 0.0));
                    pa.0 += nx * push;
                    pa.1 += ny * push;
                    let pb = pushes.entry(b_id).or_insert((0.0, 0.0));
                    pb.0 -= nx * push;
                    pb.1 -= ny * push;
                    false
                },
            );
        }

        for (id, (px, py)) in pushes {
            let Some(entity) = self.entities.get(&id) else {
                continue;
            };
            // Don't yank a marching unit back into the blob after it took a step.
            if entity.move_to.is_some() {
                continue;
            }
            let r = unit_radius(&entity.kind);
            let nx = entity.x + px * strength.clamp(0.0, 1.2);
            let ny = entity.y + py * strength.clamp(0.0, 1.2);
            // Don't separate into buildings.
            if self.collides_at(id, nx, ny, r, None, false) {
                continue;
            }
            if let Some(entity) = self.entities.get_mut(&id) {
                entity.x = nx;
                entity.y = ny;
                entity.dirty = true;
            }
            self.grid.upsert(id, nx, ny);
        }
    }

    fn clamp_entities_to_map(&mut self) {
        let map = self.map_size as f32;
        let mut moved: Vec<(Uuid, f32, f32)> = Vec::new();
        for entity in self.entities.values_mut() {
            if !entity.unit {
                continue;
            }
            let nx = entity.x.clamp(0.5, map - 0.5);
            let ny = entity.y.clamp(0.5, map - 0.5);
            if (nx - entity.x).abs() > 0.001 || (ny - entity.y).abs() > 0.001 {
                entity.x = nx;
                entity.y = ny;
                entity.dirty = true;
                moved.push((entity.id, nx, ny));
            }
        }
        for (id, x, y) in moved {
            self.grid.upsert(id, x, y);
        }
    }

    /// Own units always; enemies only if they sit inside a friendly vision disc.
    /// Allied (!FFA): teammates and their vision discs are shared.
    /// Dev M-key (`debug_omniscient`) reveals everything for that viewer only.
    fn visible_ids_for(&self, viewer: Uuid) -> HashSet<Uuid> {
        if self.player_has_global_vision(viewer) {
            return self.entities.keys().copied().collect();
        }
        let viewer_team = self.players.get(&viewer).map(|p| p.team);
        let share = !self.ffa;
        let mut visible = HashSet::with_capacity(64);
        let mut sources: Vec<(f32, f32, f32)> = Vec::new();
        for entity in self.entities.values() {
            let friendly =
                entity.owner == viewer || (share && viewer_team == Some(entity.team));
            if !friendly {
                continue;
            }
            visible.insert(entity.id);
            if aoi::entity_provides_vision(entity) {
                let radius = aoi::vision_radius(entity);
                if radius > 0.0 {
                    sources.push((entity.x, entity.y, radius));
                }
            }
        }
        for (sx, sy, radius) in sources {
            self.grid.for_each_nearby(sx, sy, radius, |id| {
                if visible.contains(&id) {
                    return false;
                }
                let Some(entity) = self.entities.get(&id) else {
                    return false;
                };
                let dx = entity.x - sx;
                let dy = entity.y - sy;
                if dx * dx + dy * dy <= radius * radius {
                    // Enemy stealth specialists stay off FOW / client entirely.
                    if is_stealth_specialist(&entity.kind) {
                        return false;
                    }
                    visible.insert(id);
                }
                false
            });
        }
        visible
    }

    /// Teams that still own at least one living Command Center.
    fn teams_with_command_centers(&self) -> HashMap<u8, u32> {
        let mut teams: HashMap<u8, u32> = HashMap::new();
        for entity in self.entities.values() {
            if entity.hp <= 0.0 || entity.kind != "hq" {
                continue;
            }
            let Some(player) = self.players.get(&entity.owner) else {
                continue;
            };
            *teams.entry(player.team).or_default() += 1;
        }
        teams
    }

    fn check_victory(&mut self) {
        if self.ended {
            return;
        }
        // No time limit — fight until only one side still holds a Command Center.
        // Drop-in: don't end while only one commander has joined yet.
        let hq_teams = self.teams_with_command_centers();
        if self.players.len() >= 2 && hq_teams.len() <= 1 {
            self.ended = true;
            self.winner_team = hq_teams.into_keys().next();
            self.end_reason = "Last Command Center".into();
        }
    }

    pub fn delta_for(
        &mut self,
        user_id: Uuid,
    ) -> (
        Vec<EntityView>,
        Vec<Uuid>,
        Vec<Uuid>,
        Option<ResourcesView>,
        Vec<u16>,
        Vec<ShotEvent>,
    ) {
        // Fog: only stamp / stream this commander's vision discs (not the whole map).
        let mut explored_new = self.reveal_vision_for(user_id);
        // Cap payload — first seconds can flood thousands of cell indices.
        const MAX_EXPLORED_NEW: usize = 180;
        if explored_new.len() > MAX_EXPLORED_NEW {
            explored_new.truncate(MAX_EXPLORED_NEW);
        }

        let resources = self.resources_view_for(user_id);
        let Some(player) = self.players.get_mut(&user_id) else {
            return (vec![], vec![], vec![], None, explored_new, vec![]);
        };
        let previously_known = std::mem::take(&mut player.aoi_known);

        let visible_ids = self.visible_ids_for(user_id);

        let mut entities = Vec::new();
        for id in &visible_ids {
            let Some(entity) = self.entities.get(id) else {
                continue;
            };
            let entered_vision = !previously_known.contains(id);
            if entity.dirty
                || entity.move_to.is_some()
                || entity.build_remaining_ms > 0
                || !entity.train_queue.is_empty()
                || entered_vision
            {
                entities.push(self.entity_view(entity));
            }
        }

        // Deaths vs FOW leave must stay distinct — client wrecks only real kills.
        let death_set: HashSet<Uuid> = self.removed.iter().copied().collect();
        let mut died = Vec::new();
        for id in &self.removed {
            if previously_known.contains(id) {
                died.push(*id);
            }
        }
        let mut removed = Vec::new();
        for id in previously_known.difference(&visible_ids) {
            // Keep own units in AOI set even if somehow filtered — never ghost-drop them.
            if self.entities.get(id).is_some_and(|e| e.owner == user_id) {
                continue;
            }
            if death_set.contains(id) {
                continue;
            }
            removed.push(*id);
        }

        // Shots only if either end is in this viewer's fog window.
        let shots: Vec<ShotEvent> = self
            .shots
            .iter()
            .filter(|s| visible_ids.contains(&s.from) || visible_ids.contains(&s.to))
            .cloned()
            .collect();

        if let Some(player) = self.players.get_mut(&user_id) {
            // Re-insert own entities that we skipped removing so AOI stays consistent.
            let mut known = visible_ids;
            for id in &previously_known {
                if let Some(e) = self.entities.get(id) {
                    if e.owner == user_id {
                        known.insert(*id);
                    }
                }
            }
            player.aoi_known = known;
        }

        (entities, removed, died, resources, explored_new, shots)
    }

    /// After reconnect, force the next deltas to re-send everything currently visible.
    pub fn force_aoi_resync(&mut self, user_id: Uuid) {
        if let Some(player) = self.players.get_mut(&user_id) {
            player.aoi_known.clear();
        }
    }

    pub fn clear_frame_flags(&mut self) {
        for entity in self.entities.values_mut() {
            entity.dirty = false;
        }
        self.removed.clear();
        self.shots.clear();
    }

    fn entity_view(&self, entity: &Entity) -> EntityView {
        let (owner_name, colors) = self
            .players
            .get(&entity.owner)
            .map(|p| {
                let name = if p.is_bot() {
                    format!("{} [BOT]", p.name)
                } else {
                    p.name.clone()
                };
                (name, p.colors)
            })
            .unwrap_or_else(|| ("Unknown".into(), [0x888888, 0x555555, 0x333333]));

        let progress = if entity.build_remaining_ms > 0 {
            let total = buildables()
                .iter()
                .find(|b| b.kind == entity.kind)
                .map(|b| b.build_ms as f32)
                .unwrap_or(15_000.0);
            Some(1.0 - (entity.build_remaining_ms as f32 / total).min(1.0))
        } else {
            None
        };

        let train_progress = if entity.build_remaining_ms == 0 {
            entity.train_queue.front().map(|job| {
                let total = trainables()
                    .iter()
                    .find(|u| u.unit == job.unit)
                    .map(|u| u.train_ms as f32)
                    .unwrap_or(5_000.0)
                    .max(1.0);
                1.0 - (job.remaining_ms as f32 / total).min(1.0)
            })
        } else {
            None
        };

        EntityView {
            id: entity.id,
            kind: entity.kind.clone(),
            owner: entity.owner,
            owner_name,
            colors,
            team: entity.team,
            x: entity.x,
            y: entity.y,
            hp: entity.hp,
            max_hp: entity.max_hp,
            building: entity.building,
            unit: entity.unit,
            flag: entity.flag.clone(),
            progress,
            train_progress,
            train_queue: entity.train_queue.len().min(255) as u8,
            prone: entity.prone,
            hacked: entity.building && self.tick < entity.hacked_until_tick,
            aim_at: if entity.kind.contains("tank")
                || entity.kind.contains("mlrs")
                || entity.kind == "turret"
                || entity.kind == "bunker"
                || entity.kind == "firebase"
                || entity.kind == "gatling_cannon"
            {
                entity.target
            } else {
                None
            },
            aim_yaw: if entity.kind.contains("tank")
                || entity.kind.contains("mlrs")
                || entity.kind == "turret"
                || entity.kind == "bunker"
                || entity.kind == "firebase"
                || entity.kind == "gatling_cannon"
            {
                Some(entity.aim_yaw)
            } else {
                None
            },
            airborne: is_f16_kind(&entity.kind)
                && (entity.mag_ammo > 0 || entity.move_to.is_some() || entity.target.is_some()),
        }
    }
}
