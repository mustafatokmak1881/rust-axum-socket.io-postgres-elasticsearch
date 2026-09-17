use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use rand::Rng;
use uuid::Uuid;

use super::aoi;
use super::bots::{self, BotMind};
use super::grid::{SpatialGrid, MAX_ENTITY_RADIUS, MAX_UNIT_RADIUS};
use super::protocol::{
    BuildableInfo, EntityView, MatchSnapshot, ResourcesView, ShotEvent, TrainableInfo,
};

pub const TICK_HZ: u32 = 20;
pub const BROADCAST_EVERY: u32 = 2; // 10 Hz to clients
pub const MAX_PLAYERS: u8 = 100;

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
    pub supplies: i32,
    pub fuel: i32,
    pub munitions: i32,
    pub power: i32,
    pub power_used: i32,
}

impl Resources {
    pub fn starter() -> Self {
        Self {
            supplies: 50_000,
            fuel: 50_000,
            munitions: 50_000,
            power: 1_000,
            power_used: 0,
        }
    }

    pub fn view(&self) -> ResourcesView {
        ResourcesView {
            supplies: self.supplies,
            fuel: self.fuel,
            munitions: self.munitions,
            power: self.power,
            power_used: self.power_used,
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
    pub cost_supplies: i32,
    pub cost_fuel: i32,
    pub cost_munitions: i32,
    pub build_ms: u32,
    pub power: i32,
    pub hp: f32,
}

#[derive(Clone, Debug)]
pub struct UnitDef {
    pub unit: &'static str,
    pub name: &'static str,
    pub from_building: &'static str,
    pub cost_supplies: i32,
    pub cost_fuel: i32,
    pub cost_munitions: i32,
    pub train_ms: u32,
    pub hp: f32,
    pub damage: f32,
    pub speed: f32,
    pub range: f32,
    /// Time between shots (realistic reload / burst spacing).
    pub attack_ms: u32,
}

pub fn buildables() -> &'static [BuildDef] {
    &[
        BuildDef {
            kind: "power_plant",
            name: "Cold Fusion Reactor",
            cost_supplies: 800,
            cost_fuel: 200,
            cost_munitions: 0,
            build_ms: 8_000,
            power: 100,
            hp: 1800.0,
        },
        BuildDef {
            kind: "barracks",
            name: "Barracks",
            cost_supplies: 600,
            cost_fuel: 0,
            cost_munitions: 200,
            build_ms: 10_000,
            power: -20,
            hp: 2200.0,
        },
        BuildDef {
            kind: "war_factory",
            name: "War Factory",
            cost_supplies: 1200,
            cost_fuel: 400,
            cost_munitions: 400,
            build_ms: 14_000,
            power: -30,
            hp: 2800.0,
        },
        BuildDef {
            kind: "supply",
            name: "Supply Center",
            cost_supplies: 500,
            cost_fuel: 0,
            cost_munitions: 0,
            build_ms: 7_000,
            power: -10,
            hp: 1500.0,
        },
        BuildDef {
            kind: "turret",
            name: "Patriot Battery",
            cost_supplies: 700,
            cost_fuel: 0,
            cost_munitions: 500,
            build_ms: 9_000,
            power: -15,
            hp: 1400.0,
        },
    ]
}

pub fn trainables() -> &'static [UnitDef] {
    // Scale: HQ visual ~2.15 wu ≈ 22–28 m → 1 wu ≈ 12–13 m.
    // Combat: frequent fire, low hit chance, high damage on connect (realistic lethality).
    &[
        UnitDef {
            unit: "ranger",
            name: "Ranger",
            from_building: "barracks",
            cost_supplies: 120,
            cost_fuel: 0,
            cost_munitions: 40,
            train_ms: 3_500,
            // 3× longer infantry fights: hits still hurt, one round never drops a man.
            hp: 525.0,
            damage: 50.0,
            // Combat jog ~5–6 km/h → well below tank cross-country pace.
            speed: 0.20,
            range: 4.5,
            // Semi-auto under fire — not a spray; mag dump then a real reload.
            attack_ms: 850,
        },
        UnitDef {
            unit: "missile_defender",
            name: "Missile Defender",
            from_building: "barracks",
            cost_supplies: 280,
            cost_fuel: 40,
            cost_munitions: 160,
            train_ms: 8_000,
            hp: 495.0,
            // Guided punch vs armor; infantry is wounded over several hits (see hit_damage).
            damage: 280.0,
            // Laden AT team — slower than rifle infantry.
            speed: 0.16,
            range: 7.5,
            attack_ms: 4_800,
        },
        UnitDef {
            unit: "tank",
            name: "Crusader Tank",
            from_building: "war_factory",
            cost_supplies: 2_200,
            cost_fuel: 900,
            cost_munitions: 700,
            train_ms: 48_000,
            hp: 7_200.0,
            // 3× TTK vs armor; infantry takes several shells via hit_damage().
            damage: 400.0,
            // Cross-country combat pace ~3× infantry jog.
            speed: 0.58,
            range: 7.5,
            attack_ms: 5_200,
        },
        UnitDef {
            unit: "tank_desert",
            name: "Desert Crusader",
            from_building: "war_factory",
            cost_supplies: 2_200,
            cost_fuel: 900,
            cost_munitions: 700,
            train_ms: 48_000,
            hp: 7_200.0,
            damage: 400.0,
            speed: 0.58,
            range: 7.5,
            attack_ms: 5_200,
        },
    ]
}

fn attack_cooldown_for(kind: &str) -> u32 {
    trainables()
        .iter()
        .find(|u| u.unit == kind)
        .map(|u| u.attack_ms)
        .unwrap_or(1_000)
}

const RIFLE_MAG: u8 = 30;
const RIFLE_RELOAD_MS: u32 = 5_000;

fn is_rifle_infantry(kind: &str) -> bool {
    !kind.contains("tank") && !kind.contains("missile")
}

/// Infantry fights last ~3× longer: heavy weapons don't delete a soldier in one connect.
fn hit_damage(attacker_kind: &str, target: &Entity, base: f32) -> f32 {
    let infantry = target.unit && !target.kind.contains("tank");
    if infantry && (attacker_kind.contains("tank") || attacker_kind.contains("missile")) {
        (base * 0.28).max(48.0)
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
        range * 0.42
    } else if attacker_kind.contains("missile") {
        range * 0.32
    } else {
        range * 0.22
    };
    let dist_mul = 1.0 / (1.0 + (dist / d0).powi(2));

    // Point-blank connect rate (before size / cover / prone).
    let weapon_near = if attacker_kind.contains("tank") {
        0.84
    } else if attacker_kind.contains("missile") {
        0.72
    } else {
        0.70
    };

    let size_mul = if target.building {
        1.55
    } else if target.kind.contains("tank") {
        1.40
    } else {
        1.0
    };

    let vis = exposure.clamp(0.12, 1.35);
    let prone_mul = if target.prone && target.unit && !target.kind.contains("tank") {
        0.38
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
    } else if entity.unit && entity.kind.contains("tank") {
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
    } else if target.kind.contains("tank") {
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

/// Soft arrival: close enough to stop even if the exact point is occupied.
fn move_arrive_radius(self_r: f32) -> f32 {
    (self_r * 2.2 + 0.04).clamp(0.06, 0.18)
}

/// Client `BUILDING_MODELS[].target` — max visual dimension after fit.
fn building_visual_size(kind: &str) -> f32 {
    match kind {
        "hq" => 2.15,
        "war_factory" => 2.1,
        "barracks" => 1.35,
        "power_plant" | "supply" => 1.7,
        "turret" => 1.4,
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
    if kind.contains("tank") {
        0.1
    } else if kind.contains("missile") {
        0.02
    } else {
        // ranger ~1/3 previous size
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

pub struct MatchSim {
    pub id: Uuid,
    pub map_size: u16,
    pub ffa: bool,
    pub tick: u64,
    pub players: HashMap<Uuid, PlayerState>,
    pub entities: HashMap<Uuid, Entity>,
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
    ) -> Self {
        let map_size = map_size.clamp(64, 256);
        let mut sim = Self {
            id,
            map_size,
            ffa,
            tick: 0,
            players: HashMap::new(),
            entities: HashMap::new(),
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

        bots::seed_opening_bots(&mut sim);
        sim
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
        let (x, y) = self.allocate_spawn_xy();
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
        });
        self.spawn_starting_force(user_id, team, x, y);
        self.reveal_vision_for(user_id);
    }

    /// Place new HQs on a wide ring so cities sit ~3× farther apart than the old cluster.
    fn allocate_spawn_xy(&self) -> (f32, f32) {
        let map = self.map_size as f32;
        let hq_positions: Vec<(f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.kind == "hq")
            .map(|e| (e.x, e.y))
            .collect();

        let (bx, by) = if hq_positions.is_empty() {
            // First player: cluster anchor slightly off map center.
            (map * 0.42, map * 0.50)
        } else {
            let n = hq_positions.len() as f32;
            let sx: f32 = hq_positions.iter().map(|(x, _)| *x).sum();
            let sy: f32 = hq_positions.iter().map(|(_, y)| *y).sum();
            (sx / n, sy / n)
        };

        // Was 12 tiles (~one base length). Triple so commanders start a real march apart.
        const MIN_SEP: f32 = 36.0;
        const GOLDEN: f32 = 2.399_963;

        for k in 0..160 {
            let r = if hq_positions.is_empty() {
                0.0
            } else {
                MIN_SEP + (k as f32).sqrt() * 10.5
            };
            let angle = k as f32 * GOLDEN;
            let x = (bx + angle.cos() * r).clamp(4.0, map - 5.0);
            let y = (by + angle.sin() * r).clamp(4.0, map - 5.0);
            let ix = x.floor() as i32;
            let iy = y.floor() as i32;

            let fx = ix as f32 + 0.5;
            let fy = iy as f32 + 0.5;
            let sep = MIN_SEP * 0.85;
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
                return (fx, fy);
            }
        }

        (
            bx.clamp(4.0, map - 5.0),
            by.clamp(4.0, map - 5.0),
        )
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
            (index % 2) as u8
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

    /// Opening army: 50 rangers + 1 tank at the commander's HQ.
    fn spawn_starting_force(&mut self, user_id: Uuid, team: u8, hx: f32, hy: f32) {
        let ranger = trainables()
            .iter()
            .find(|u| u.unit == "ranger")
            .expect("ranger def");
        let tank = trainables()
            .iter()
            .find(|u| u.unit == "tank")
            .expect("tank def");
        let hq_r = building_radius("hq");
        for _ in 0..50 {
            let r = unit_radius(ranger.unit);
            let (sx, sy) = self.find_free_spawn_near(
                hx,
                hy,
                r,
                Uuid::nil(),
                Some((hx, hy, hq_r)),
            );
            self.insert_unit(user_id, team, ranger, sx, sy);
        }
        let tr = unit_radius(tank.unit);
        let (sx, sy) = self.find_free_spawn_near(hx, hy, tr, Uuid::nil(), Some((hx, hy, hq_r)));
        self.insert_unit(user_id, team, tank, sx, sy);
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

    pub fn buildable_info() -> Vec<BuildableInfo> {
        buildables()
            .iter()
            .map(|b| BuildableInfo {
                kind: b.kind.into(),
                name: b.name.into(),
                cost_supplies: b.cost_supplies,
                cost_fuel: b.cost_fuel,
                cost_munitions: b.cost_munitions,
                build_ms: b.build_ms,
                power: b.power,
            })
            .collect()
    }

    pub fn trainable_info() -> Vec<TrainableInfo> {
        trainables()
            .iter()
            .map(|u| TrainableInfo {
                unit: u.unit.into(),
                name: u.name.into(),
                from_building: u.from_building.into(),
                cost_supplies: u.cost_supplies,
                cost_fuel: u.cost_fuel,
                cost_munitions: u.cost_munitions,
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
            team: player.team,
            ffa: self.ffa,
            aoi_radius: aoi::AOI_RADIUS,
            focus,
            explored: player.explored.to_bytes(),
            resources: player.resources.view(),
            entities,
            buildable: Self::buildable_info(),
            trainable: Self::trainable_info(),
        })
    }

    /// Stamp current unit/building vision into the player's explored map.
    pub fn reveal_vision_for(&mut self, user_id: Uuid) -> Vec<u16> {
        let sources: Vec<(f32, f32, f32)> = self
            .entities
            .values()
            .filter(|e| e.owner == user_id && aoi::entity_provides_vision(e))
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
        let def = buildables()
            .iter()
            .find(|b| b.kind == kind)
            .ok_or("Unknown building")?;
        let (alive, my_team) = {
            let player = self.players.get(&user_id).ok_or("Not in match")?;
            (player.alive, player.team)
        };
        if !alive {
            return Err("Eliminated");
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

        let player = self.players.get_mut(&user_id).ok_or("Not in match")?;
        if player.resources.supplies < def.cost_supplies
            || player.resources.fuel < def.cost_fuel
            || player.resources.munitions < def.cost_munitions
        {
            return Err("Not enough resources");
        }

        let power_after = player.resources.power_used - def.power.min(0);
        if def.power < 0 && power_after > player.resources.power {
            return Err("Not enough power");
        }

        player.resources.supplies -= def.cost_supplies;
        player.resources.fuel -= def.cost_fuel;
        player.resources.munitions -= def.cost_munitions;
        if def.power > 0 {
            player.resources.power += def.power;
        } else {
            player.resources.power_used += -def.power;
        }

        let flag = player.flag.clone();
        let id = Uuid::new_v4();
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

        // desert tank skin is cosmetic-equivalent stats; always trainable (fair).
        let player = self.players.get(&user_id).ok_or("Not in match")?;
        if !player.alive {
            return Err("Eliminated");
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

        let player = self.players.get_mut(&user_id).unwrap();
        if player.resources.supplies < def.cost_supplies
            || player.resources.fuel < def.cost_fuel
            || player.resources.munitions < def.cost_munitions
        {
            return Err("Not enough resources");
        }

        player.resources.supplies -= def.cost_supplies;
        player.resources.fuel -= def.cost_fuel;
        player.resources.munitions -= def.cost_munitions;

        let building = self.entities.get_mut(&building_id).unwrap();
        building.train_queue.push_back(TrainJob {
            unit: def.unit.into(),
            remaining_ms: def.train_ms,
        });
        building.dirty = true;
        Ok(())
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

    pub fn tick_once(&mut self) {
        if self.ended {
            return;
        }
        self.tick += 1;
        let dt_ms = 1000 / TICK_HZ;
        self.grid
            .rebuild(self.entities.values().map(|e| (e.id, e.x, e.y)));

        // Income from supply buildings.
        if self.tick % u64::from(TICK_HZ) == 0 {
            let owners: Vec<Uuid> = self
                .entities
                .values()
                .filter(|e| e.kind == "supply" && e.build_remaining_ms == 0)
                .map(|e| e.owner)
                .collect();
            for owner in owners {
                if let Some(player) = self.players.get_mut(&owner) {
                    player.resources.supplies += 40;
                    player.resources.fuel += 20;
                    player.resources.munitions += 15;
                }
            }
        }

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

            if entity.build_remaining_ms > 0 {
                entity.build_remaining_ms = entity.build_remaining_ms.saturating_sub(dt_ms);
                entity.dirty = true;
            }

            if entity.build_remaining_ms == 0 {
                if let Some(job) = entity.train_queue.front_mut() {
                    job.remaining_ms = job.remaining_ms.saturating_sub(dt_ms);
                    entity.dirty = true;
                    if job.remaining_ms == 0 {
                        let unit_kind = entity.train_queue.pop_front().unwrap().unit;
                        if let Some(def) = trainables().iter().find(|u| u.unit == unit_kind) {
                            let uid = Uuid::new_v4();
                            let (sx, sy) = self.find_free_spawn_near(
                                entity.x,
                                entity.y,
                                unit_radius(def.unit),
                                uid,
                                Some((entity.x, entity.y, building_radius(&entity.kind))),
                            );
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
                            };
                            self.put_entity(spawn);
                        }
                    }
                }
            }

            // Buildings do not fire for now — only units (soldiers/tanks) attack.
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
                    let cover = self.shot_cover(entity.id, entity.x, entity.y, t);
                    if !obeying_move && tdist <= stop_at && !cover.blocked {
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
                    // While move_to is set: keep walking to the click point and fire if in range.
                } else {
                    entity.target = None;
                }
            }

            if !hold_for_attack {
                if let Some((gx, gy)) = goal {
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
                    let cover = self.shot_cover(entity.id, entity.x, entity.y, target);
                    if cover.blocked {
                        // No shot through walls / hulls. Keep chasing, don't burn cooldown.
                    } else if dist <= entity.range && entity.attack_cooldown_ms == 0 {
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
                        let hit_p = shot_hit_chance(&kind, target, dist, entity.range, cover.exposure);
                        let mut rng = rand::thread_rng();
                        let hit = rng.gen_range(0.0..1.0) < hit_p;
                        let (ix, iy) = if hit {
                            (tx, ty)
                        } else {
                            miss_impact(&mut rng, target, fx, fy)
                        };

                        // Return fire when shot at (muzzle flash), hit or miss.
                        if let Some(t) = self.entities.get_mut(&tid) {
                            if hit {
                                t.hp -= dmg;
                                t.dirty = true;
                            }
                            if t.unit && !t.kind.contains("tank") {
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
                        // HE splash only when the shell actually lands on target.
                        if hit && kind.contains("tank") {
                            self.apply_shell_blast(team, fx, fy, tx, ty, tid);
                        }
                    }
                } else {
                    entity.target = None;
                }
            }

            // Infantry hits the dirt while shooting / being shot at; stand up after the fight.
            if entity.unit && !entity.kind.contains("tank") {
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
                self.removed.push(id);
                if entity.kind == "hq" {
                    if let Some(player) = self.players.get_mut(&entity.owner) {
                        player.alive = false;
                    }
                }
            }
        }

        bots::tick_bots(self);
        self.check_victory();
    }

    /// HE blast around a tank shell impact. Primary target already took direct damage.
    /// Infantry behind the struck hull / a wall relative to the incoming shot are shielded.
    fn apply_shell_blast(
        &mut self,
        team: u8,
        from_x: f32,
        from_y: f32,
        x: f32,
        y: f32,
        primary: Uuid,
    ) {
        const RADIUS: f32 = 1.15;
        let mut victims: Vec<(Uuid, f32, bool)> = Vec::new();
        self.grid.for_each_nearby(x, y, RADIUS + MAX_ENTITY_RADIUS, |id| {
            let Some(e) = self.entities.get(&id) else {
                return false;
            };
            if e.team == team || e.hp <= 0.0 || !(e.unit || e.building) {
                return false;
            }
            let dx = e.x - x;
            let dy = e.y - y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > RADIUS {
                return false;
            }
            victims.push((e.id, dist, e.unit && !e.kind.contains("tank")));
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
            // Hull / wall between blast and victim: no through-shot splash.
            if self.blast_blocked(primary, x, y, iux, iuy, victim) {
                continue;
            }
            let falloff = (1.0 - dist / RADIUS).clamp(0.0, 1.0);
            let dmg = if is_infantry {
                // Center = lethal; outer ring = wound.
                70.0 * falloff.powf(0.75)
            } else if self
                .entities
                .get(&id)
                .is_some_and(|e| e.kind.contains("tank"))
            {
                45.0 * falloff
            } else {
                // Buildings take modest splash
                55.0 * falloff
            };
            if dmg < 1.0 {
                continue;
            }
            if let Some(e) = self.entities.get_mut(&id) {
                e.hp -= dmg;
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

    /// How close an enemy structure may sit before the tile is "their territory".
    fn enemy_build_block_radius(kind: &str) -> f32 {
        match kind {
            "hq" => 14.0,
            "war_factory" | "barracks" => 10.0,
            _ => 8.0,
        }
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
            .map(|(_, _, er)| er + radius + collision_pad() + 0.15)
            .unwrap_or(radius + 0.8);
        for k in 0..48 {
            let ang = k as f32 * 0.7;
            let dist = base + (k as f32) * 0.18;
            let x = (bx + ang.cos() * dist).clamp(0.5, map - 0.5);
            let y = (by + ang.sin() * dist).clamp(0.5, map - 0.5);
            if let Some((ex, ey, er)) = extra_solid {
                let dx = ex - x;
                let dy = ey - y;
                let min_d = radius + er + collision_pad();
                if dx * dx + dy * dy < min_d * min_d {
                    continue;
                }
            }
            if !self.collides_at(self_id, x, y, radius, None, true) {
                return (x, y);
            }
        }
        (
            (bx + base + 0.4).clamp(0.5, map - 0.5),
            (by + base + 0.4).clamp(0.5, map - 0.5),
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
                    || other.kind.contains("tank")
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
            .filter(|e| e.unit)
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
            let a_tank = a.kind.contains("tank");
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
                    let b_tank = b.kind.contains("tank");
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
    fn visible_ids_for(&self, viewer: Uuid) -> HashSet<Uuid> {
        let mut visible = HashSet::with_capacity(64);
        let mut sources: Vec<(f32, f32, f32)> = Vec::new();
        for entity in self.entities.values() {
            if entity.owner != viewer {
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

    fn check_victory(&mut self) {
        if self.ended {
            return;
        }
        if self.created_at.elapsed() >= self.max_duration {
            self.ended = true;
            self.end_reason = "Time limit".into();
            // Highest remaining HQ hp team wins loosely: most alive players' team.
            let mut team_alive: HashMap<u8, u32> = HashMap::new();
            for player in self.players.values().filter(|p| p.alive) {
                *team_alive.entry(player.team).or_default() += 1;
            }
            self.winner_team = team_alive
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(t, _)| t);
            return;
        }

        let alive_teams: HashSet<u8> = self
            .players
            .values()
            .filter(|p| p.alive)
            .map(|p| p.team)
            .collect();

        // Drop-in matches: don't end while only one commander has joined yet.
        if self.players.len() >= 2 && alive_teams.len() <= 1 {
            self.ended = true;
            self.winner_team = alive_teams.into_iter().next();
            self.end_reason = "Last command standing".into();
        }
    }

    pub fn delta_for(
        &mut self,
        user_id: Uuid,
    ) -> (
        Vec<EntityView>,
        Vec<Uuid>,
        Option<ResourcesView>,
        Vec<u16>,
        Vec<ShotEvent>,
    ) {
        if self.tick % 4 == 0 {
            let _ = self.reveal_vision_for(user_id);
        }
        let explored_new = Vec::new();

        let Some(player) = self.players.get_mut(&user_id) else {
            return (vec![], vec![], None, explored_new, vec![]);
        };
        let resources = Some(player.resources.view());
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

        let mut removed = self.removed.clone();
        for id in previously_known.difference(&visible_ids) {
            if self.entities.get(id).is_some_and(|e| e.owner == user_id) {
                continue;
            }
            removed.push(*id);
        }

        // Show shots involving you, or either end currently in vision.
        let shots: Vec<ShotEvent> = self
            .shots
            .iter()
            .filter(|s| {
                if visible_ids.contains(&s.from) || visible_ids.contains(&s.to) {
                    return true;
                }
                let from_mine = self.entities.get(&s.from).is_some_and(|e| e.owner == user_id);
                let to_mine = self.entities.get(&s.to).is_some_and(|e| e.owner == user_id);
                from_mine || to_mine
            })
            .cloned()
            .collect();

        if let Some(player) = self.players.get_mut(&user_id) {
            player.aoi_known = visible_ids;
        }

        (entities, removed, resources, explored_new, shots)
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
        }
    }
}
