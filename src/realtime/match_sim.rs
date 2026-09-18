use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use rand::Rng;
use uuid::Uuid;

use super::aoi;
use super::bots::{self, BotMind};
use super::generals_roster::{self, faction_ok};
use super::grid::{SpatialGrid, MAX_ENTITY_RADIUS, MAX_UNIT_RADIUS};
use super::protocol::{
    BuildableInfo, EntityView, MatchSnapshot, PondView, ResourcesView, ScoreboardRow, ShotEvent,
    TrainableInfo,
};

pub const TICK_HZ: u32 = 20;
pub const BROADCAST_EVERY: u32 = 2; // 10 Hz to clients
pub const MAX_PLAYERS: u8 = 32;

/// Total living+queued units with a single home HQ.
pub const HOME_UNIT_BUDGET: usize = 36;
/// Extra units per captured colony HQ (= half of home → x + x/2 + x/2 …).
pub const COLONY_UNIT_BUDGET: usize = HOME_UNIT_BUDGET / 2;
/// Non-HQ structures allowed at the home base.
pub const HOME_BUILDING_BUDGET: usize = 12;
/// Extra structures unlocked per captured colony HQ (= half of home).
pub const COLONY_BUILDING_BUDGET: usize = HOME_BUILDING_BUDGET / 2;
/// Wipe / claim radius around a fallen HQ (city footprint).
pub const CITY_CLAIM_RADIUS: f32 = 14.0;

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
            gold: 10_000,
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
/// Generals Patriot: strong vs vehicles/air, weak vs infantry. Range ~225 logic ≈ 9.0 wu.
const PATRIOT_RANGE: f32 = 9.0;
const PATRIOT_DAMAGE: f32 = 520.0;
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
        || kind.contains("comanche")
        || kind.contains("helix")
        || kind.contains("chinook")
}

#[inline]
fn is_air_kind(kind: &str) -> bool {
    kind.contains("raptor")
        || kind.contains("mig")
        || kind.contains("comanche")
        || kind.contains("helix")
        || kind.contains("chinook")
}

#[inline]
fn is_air_bomb_kind(kind: &str) -> bool {
    kind.contains("raptor")
        || kind.contains("mig")
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

/// Visible traverse (~66°/s) — the gun waits until this finishes.
const TANK_TURRET_RATE: f32 = 1.15;
const TANK_AIM_ALIGN: f32 = 0.07;
const TANK_MG_RANGE: f32 = 5.4;
const TANK_MG_COOLDOWN_MS: u32 = 130;
/// M270 pod slew — heavier than a tank turret, still waits for bearing.
const MLRS_POD_RATE: f32 = 0.72;
const MLRS_AIM_ALIGN: f32 = 0.10;
/// Rockets in one ripple before the long reload.
const MLRS_SALVO: u8 = 6;
/// Gap between rockets in a ripple (~real M270 spacing, shortened for game pace).
const MLRS_RIPPLE_MS: u32 = 140;
const MLRS_RELOAD_MS: u32 = 9_200;
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
    } else if armored && attacker_kind.contains("tank") && soft_vehicle {
        // Tank gun vs soft launcher — catastrophic.
        (base * 2.1).max(base)
    } else if target.building
        && (attacker_kind.contains("abrams")
            || attacker_kind.contains("paladin")
            || attacker_kind.contains("marauder")
            || attacker_kind.contains("overlord"))
    {
        (base * 1.15).max(base)
    } else if target.building && (attacker_kind == "turret" || attacker_kind == "stinger_site") {
        (base * 0.75).max(280.0)
    } else if target.building && attacker_kind == "firebase" {
        (base * 1.05).max(base)
    } else if target.building && attacker_kind.contains("mlrs") {
        (base * 1.1).max(base)
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
        range * 0.45
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
        0.86
    } else if attacker_kind == "turret" || attacker_kind.contains("missile") {
        0.84
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

/// Dense opening-army pack — kept for potential future spawn layouts.
#[allow(dead_code)]
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
        "airfield" => 2.0,
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
    let count = 7 + (seed % 5) as usize; // 7–11 ponds
    let mut ponds: Vec<PondView> = Vec::with_capacity(count);
    let mut s = seed;
    let mut attempts = 0;
    while ponds.len() < count && attempts < count * 40 {
        attempts += 1;
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(attempts as u64 + 1);
        let x = 12.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 24.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(17);
        let y = 12.0 + ((s % 10_000) as f32 / 10_000.0) * (map - 24.0).max(8.0);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(31);
        let r = 2.8 + ((s % 50) as f32) * 0.07; // ~2.8–6.3
        // Prefer mid-map lakes; keep clear of west/east spawn bands.
        if x < map * 0.18 || x > map * 0.82 {
            continue;
        }
        let mut overlap = false;
        for p in &ponds {
            let dx = p.x - x;
            let dy = p.y - y;
            let min = p.r + r + 3.0;
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

pub struct MatchSim {
    pub id: Uuid,
    pub map_size: u16,
    pub ffa: bool,
    pub tick: u64,
    pub players: HashMap<Uuid, PlayerState>,
    pub entities: HashMap<Uuid, Entity>,
    /// Impassable water discs — blocks ground units and building placement.
    pub ponds: Vec<PondView>,
    /// Spatial hash of `entities` — rebuilt/kept in sync for neighbor queries.
    pub(crate) grid: SpatialGrid,
    pub removed: Vec<Uuid>,
    /// Shots fired since last client broadcast (cleared in clear_frame_flags).
    pub shots: Vec<ShotEvent>,
    pub ended: bool,
    pub winner_team: Option<u8>,
    pub end_reason: String,
    pub created_at: Instant,
    pub max_duration: Duration,
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
        target_players: u8,
    ) -> Self {
        let map_size = map_size.clamp(64, 256);
        let ponds = generate_ponds(id, map_size);
        let mut sim = Self {
            id,
            map_size,
            ffa,
            tick: 0,
            players: HashMap::new(),
            entities: HashMap::new(),
            ponds,
            grid: SpatialGrid::new(),
            removed: Vec::new(),
            shots: Vec::new(),
            ended: false,
            winner_team: None,
            end_reason: String::new(),
            created_at: Instant::now(),
            max_duration: Duration::from_secs(30 * 60),
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

        let target = (target_players as usize).clamp(2, MAX_PLAYERS as usize);
        bots::seed_opening_bots(&mut sim, target);
        if !sim.ffa {
            sim.rebalance_allied_teams();
        }
        sim
    }

    /// Ally skirmish: split commanders ~50/50 by HQ position — west Team 0, east Team 1.
    /// Alone (ffa): each commander is their own team (set at spawn).
    fn rebalance_allied_teams(&mut self) {
        if self.ffa {
            return;
        }
        let mut marks: Vec<(Uuid, f32)> = Vec::new();
        for p in self.players.values() {
            let hx = self
                .entities
                .values()
                .find(|e| e.owner == p.user_id && e.kind == "hq" && e.hp > 0.0)
                .map(|e| e.x)
                .unwrap_or(p.focus[0]);
            marks.push((p.user_id, hx));
        }
        if marks.len() < 2 {
            return;
        }
        marks.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mid = marks.len().div_ceil(2);
        for (i, (id, _)) in marks.iter().enumerate() {
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
            hp: 7500.0,
            max_hp: 7500.0,
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
        });
        self.spawn_starting_force(user_id, team, x, y);
        self.reveal_vision_for(user_id);
    }

    /// Place new HQs on a wide ring. Ally mode biases Team 0 west / Team 1 east.
    fn allocate_spawn_xy(&self, team: u8) -> (f32, f32) {
        let map = self.map_size as f32;
        let hq_positions: Vec<(f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.kind == "hq")
            .map(|e| (e.x, e.y))
            .collect();

        let (bx, by) = if !self.ffa {
            // Two fronts: west (team 0) vs east (team 1).
            let x = if team == 0 { map * 0.22 } else { map * 0.78 };
            let y = map * 0.50;
            (x, y)
        } else if hq_positions.is_empty() {
            (map * 0.42, map * 0.50)
        } else {
            let n = hq_positions.len() as f32;
            let sx: f32 = hq_positions.iter().map(|(x, _)| *x).sum();
            let sy: f32 = hq_positions.iter().map(|(_, y)| *y).sum();
            (sx / n, sy / n)
        };

        const GOLDEN: f32 = 2.399_963;
        // Pack denser when the lobby is large so 32 HQs still fit.
        let min_sep = if self.players.len() >= 20 {
            11.0
        } else if self.players.len() >= 12 {
            14.0
        } else {
            18.0
        };

        for k in 0..220 {
            let r = if hq_positions.is_empty() && self.ffa {
                0.0
            } else if hq_positions.is_empty() {
                (k as f32).sqrt() * 4.0
            } else {
                min_sep + (k as f32).sqrt() * 5.5
            };
            let angle = k as f32 * GOLDEN;
            let mut x = (bx + angle.cos() * r).clamp(4.0, map - 5.0);
            let y = (by + angle.sin() * r).clamp(4.0, map - 5.0);
            if !self.ffa {
                // Keep allies on their half of the map.
                if team == 0 {
                    x = x.clamp(4.0, map * 0.45);
                } else {
                    x = x.clamp(map * 0.55, map - 5.0);
                }
            }
            let fx = x.floor() as f32 + 0.5;
            let fy = y.floor() as f32 + 0.5;
            let sep = min_sep * 0.85;
            let mut blocked = false;
            self.grid.for_each_nearby(fx, fy, sep + MAX_ENTITY_RADIUS, |id| {
                let Some(e) = self.entities.get(&id) else {
                    return false;
                };
                if !e.building {
                    return false;
                }
                let dx = e.x - fx;
                let dy = e.y - fy;
                if dx * dx + dy * dy < sep * sep {
                    blocked = true;
                    return true;
                }
                false
            });
            if !blocked {
                if self.water_blocks(fx, fy, building_radius("hq") + 0.5) {
                    continue;
                }
                return (fx, fy);
            }
        }

        let fallback_x = if !self.ffa {
            if team == 0 {
                (map * 0.22).clamp(4.0, map - 5.0)
            } else {
                (map * 0.78).clamp(4.0, map - 5.0)
            }
        } else {
            bx.clamp(4.0, map - 5.0)
        };
        let fy = by.clamp(4.0, map - 5.0);
        let hq_r = building_radius("hq") + 0.5;
        if !self.water_blocks(fallback_x, fy, hq_r) {
            return (fallback_x, fy);
        }
        // Nudge off water if the naive fallback landed in a pond.
        for k in 0..48 {
            let ang = k as f32 * 0.7;
            let dist = 2.0 + k as f32 * 0.35;
            let x = (fallback_x + ang.cos() * dist).clamp(4.0, map - 5.0);
            let y = (fy + ang.sin() * dist).clamp(4.0, map - 5.0);
            if !self.water_blocks(x, y, hq_r) {
                return (x, y);
            }
        }
        (fallback_x, fy)
    }

    /// Mid-match join: spawn HQ on the same wide ring as the opening cities.
    pub fn add_player(
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

        let index = self.players.len();
        let team = if self.ffa {
            index as u8
        } else {
            self.pick_allied_team()
        };

        self.spawn_commander(user_id, name, faction, team, flag, true, None);
        Ok(())
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn human_count(&self) -> usize {
        self.players.values().filter(|p| !p.is_bot()).count()
    }

    /// Faction opening base + army (same package for humans and bots).
    /// Buildings match that faction's roster (USA / China / GLA).
    fn spawn_starting_force(&mut self, user_id: Uuid, team: u8, hx: f32, hy: f32) {
        let faction = self
            .players
            .get(&user_id)
            .map(|p| p.faction.as_str())
            .unwrap_or("usa")
            .to_string();

        // HQ provides a small base load; plants/reactors add the rest.
        if let Some(player) = self.players.get_mut(&user_id) {
            player.resources.power = player.resources.power.saturating_add(40);
        }

        let (power_kind, supply_kind, factory_kind, tank_unit, infantry_unit) =
            match faction.as_str() {
                "china" => (
                    Some("nuclear_reactor"),
                    "supply",
                    "war_factory",
                    "battlemaster",
                    "red_guard",
                ),
                "gla" => (None, "supply_stash", "arms_dealer", "scorpion_tank", "rebel"),
                _ => (
                    Some("power_plant"),
                    "supply",
                    "war_factory",
                    "tank",
                    "ranger",
                ),
            };

        // Ring of finished starter structures around the Command Center.
        let mut slots: Vec<(&str, f32, f32)> = Vec::new();
        if let Some(pk) = power_kind {
            slots.push((pk, 3.2, 0.4));
        }
        slots.push((supply_kind, 2.6, 2.0));
        slots.push(("barracks", 0.2, 3.4));
        slots.push((factory_kind, -2.8, 2.2));

        for (kind, ox, oy) in slots {
            self.spawn_finished_building(user_id, team, &faction, kind, hx + ox, hy + oy);
        }

        let tank = trainables()
            .iter()
            .find(|u| u.unit == tank_unit && faction_ok(u.faction, &faction))
            .or_else(|| trainables().iter().find(|u| u.unit == "tank"));
        let infantry = trainables()
            .iter()
            .find(|u| u.unit == infantry_unit && faction_ok(u.faction, &faction));

        let hq_r = building_radius("hq");
        let unit_offsets: &[(f32, f32)] = &[
            (4.0, 0.0),
            (4.2, 1.1),
            (3.6, -1.0),
            (5.0, 0.5),
            (4.6, -0.8),
        ];
        for (i, &(ox, oy)) in unit_offsets.iter().enumerate() {
            let def = if i == 0 {
                tank
            } else {
                infantry.or(tank)
            };
            let Some(def) = def else { continue };
            let tr = unit_radius(def.unit);
            let map = self.map_size as f32;
            let pack_x = (hx + ox).clamp(0.5, map - 0.5);
            let pack_y = (hy + oy).clamp(0.5, map - 0.5);
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
            self.insert_unit(user_id, team, def, sx, sy);
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
        })
    }

    /// Tab scoreboard: every commander, army size, and economy.
    pub fn scoreboard_for(&self, viewer: Uuid) -> Vec<ScoreboardRow> {
        let mut rows: Vec<ScoreboardRow> = self
            .players
            .values()
            .map(|p| {
                let mut infantry = 0u32;
                let mut tanks = 0u32;
                let mut buildings = 0u32;
                let mut bases = 0u32;
                let mut hq_pos: Option<(f32, f32)> = None;
                for e in self.entities.values() {
                    if e.owner != p.user_id || e.hp <= 0.0 {
                        continue;
                    }
                    if e.kind == "hq" {
                        bases += 1;
                        if hq_pos.is_none() {
                            hq_pos = Some((e.x, e.y));
                        }
                    }
                    if e.building {
                        buildings += 1;
                    } else if e.unit {
                        if e.kind.contains("tank") || e.kind.contains("mlrs") {
                            tanks += 1;
                        } else {
                            infantry += 1;
                        }
                    }
                }
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
        rows.sort_by(|a, b| {
            b.alive
                .cmp(&a.alive)
                .then_with(|| b.bases.cmp(&a.bases))
                .then_with(|| (b.infantry + b.tanks).cmp(&(a.infantry + a.tanks)))
                .then_with(|| b.gold.cmp(&a.gold))
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
        });

        // Redis-stream style delayed job marker.
        self.stream_jobs.push_back(StreamJob {
            due_tick: self.tick + (def.build_ms as u64 / (1000 / TICK_HZ as u64)).max(1),
        });
        self.reveal_vision_for(user_id);

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

    pub fn attack(&mut self, user_id: Uuid, ids: &[Uuid], target_id: Uuid) {
        let Some(target) = self.entities.get(&target_id) else {
            return;
        };
        if target.team
            == self
                .players
                .get(&user_id)
                .map(|p| p.team)
                .unwrap_or(255)
        {
            return;
        }
        for id in ids {
            if let Some(entity) = self.entities.get_mut(id) {
                if entity.owner == user_id && entity.unit {
                    entity.target = Some(target_id);
                    entity.move_to = None;
                    entity.stuck_frames = 0;
                    entity.detour = None;
                    entity.detour_ttl = 0;
                    entity.dirty = true;
                }
            }
        }
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
            let g = by_owner.entry(e.owner).or_default();
            match e.kind.as_str() {
                "hq" => {
                    g.gold += 14;
                }
                "supply" | "supply_stash" => {
                    g.gold += 50;
                }
                "black_market" => {
                    g.gold += 64;
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
            }
            player.resources.power = player.resources.power.saturating_add(g.pwr);
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
            }

            let powered = self
                .players
                .get(&entity.owner)
                .map(|p| p.resources.has_power())
                .unwrap_or(false);

            // Brownout: factories / barracks freeze production. Power plants still finish.
            if entity.build_remaining_ms == 0 && powered {
                if let Some(job) = entity.train_queue.front_mut() {
                    job.remaining_ms = job.remaining_ms.saturating_sub(dt_ms);
                    entity.dirty = true;
                    if job.remaining_ms == 0 {
                        let unit_kind = entity.train_queue.pop_front().unwrap().unit;
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
                            let (sx, sy) = if is_air_kind(def.unit) {
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
                            };
                            self.put_entity(spawn);
                        }
                    }
                }
            }

            // Armed buildings go dark without power.
            if powered
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
            if entity.damage > 0.0 && entity.range > 0.0 && self.tick % 2 == 0 {
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
                    } else if dist <= entity.range && entity.attack_cooldown_ms == 0 && aimed {
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
                        let hit = rng.gen_range(0.0..1.0) < hit_p;
                        // MLRS: every rocket lands in a tight beaten zone (even "hits" scatter a bit).
                        let (ix, iy) = if kind.contains("mlrs") {
                            let j = if hit { 0.32 } else { 0.58 };
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
                        } else if hit && is_air_bomb_kind(&kind) {
                            self.apply_air_bomb_blast(team, attacker_owner, fx, fy, ix, iy, tid);
                        } else if hit && kind.contains("tank") && !kind.contains("mg") {
                            self.apply_shell_blast(team, attacker_owner, fx, fy, tx, ty, tid);
                        } else if hit && kind.contains("mortar") {
                            self.apply_mortar_blast(team, attacker_owner, fx, fy, tx, ty, tid);
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

        // Remove dead.
        let dead: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.hp <= 0.0)
            .map(|e| e.id)
            .collect();
        for id in dead {
            if let Some(entity) = self.take_entity(id) {
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

    /// HQ falls → wipe local garrison buildings, grant site to the killer as a colony HQ.
    fn on_hq_destroyed(&mut self, hq: Entity) {
        let hx = hq.x;
        let hy = hq.y;
        let former = hq.owner;
        let claim_r2 = CITY_CLAIM_RADIUS * CITY_CLAIM_RADIUS;

        let wipe: Vec<Uuid> = self
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
        for wid in wipe {
            if let Some(e) = self.take_entity(wid) {
                self.refund_building_economy(&e);
                self.removed.push(wid);
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
            }
        } else if let Some(player) = self.players.get_mut(&former) {
            player.colonies = remaining_hq.saturating_sub(1) as u32;
        }

        // Cap shrinks with lost HQs — drop queued trains / builds that no longer fit.
        self.enforce_budgets_for(former);

        if let Some(conqueror) = self.resolve_conqueror(&hq) {
            self.grant_colony_hq(conqueror, hx, hy);
        }
    }

    fn is_hostile_commander(&self, attacker: Uuid, victim_owner: Uuid, victim_team: u8) -> bool {
        if attacker == victim_owner {
            return false;
        }
        let Some(ap) = self.players.get(&attacker) else {
            return false;
        };
        if !ap.alive {
            return false;
        }
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
    fn grant_colony_hq(&mut self, owner: Uuid, x: f32, y: f32) {
        let Some(player) = self.players.get(&owner) else {
            return;
        };
        if !player.alive {
            return;
        }
        let team = player.team;
        let flag = player.flag.clone();
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
            hp: 7500.0,
            max_hp: 7500.0,
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
        });
        let hqs = self
            .entities
            .values()
            .filter(|e| e.owner == owner && e.kind == "hq" && e.hp > 0.0)
            .count();
        if let Some(player) = self.players.get_mut(&owner) {
            player.colonies = hqs.saturating_sub(1) as u32;
            player.focus = [fx, fy];
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
        if self.water_blocks(x, y, self_r) {
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
                    visible.insert(id);
                }
                false
            });
        }
        visible
    }

    /// Teams that still have anything on the map (units or buildings).
    /// HQ loss marks a player DEAD for scoreboard/build, but leftover army keeps them in the fight.
    fn teams_with_forces(&self) -> HashMap<u8, f32> {
        let mut teams: HashMap<u8, f32> = HashMap::new();
        for entity in self.entities.values() {
            if entity.hp <= 0.0 {
                continue;
            }
            let Some(player) = self.players.get(&entity.owner) else {
                continue;
            };
            *teams.entry(player.team).or_default() += entity.hp.max(0.0);
        }
        teams
    }

    fn check_victory(&mut self) {
        if self.ended {
            return;
        }
        let force_teams = self.teams_with_forces();

        if self.created_at.elapsed() >= self.max_duration {
            self.ended = true;
            self.end_reason = "Time limit".into();
            self.winner_team = force_teams
                .into_iter()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(t, _)| t);
            return;
        }

        // Don't end on HQ death alone — only when one side has nothing left on the field.
        // Drop-in matches: don't end while only one commander has joined yet.
        if self.players.len() >= 2 && force_teams.len() <= 1 {
            self.ended = true;
            self.winner_team = force_teams.into_keys().next();
            self.end_reason = "Last force standing".into();
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
            prone: entity.prone,
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
        }
    }
}
