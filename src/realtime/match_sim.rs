use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use rand::Rng;
use uuid::Uuid;

use super::aoi;
use super::protocol::{
    BuildableInfo, EntityView, MatchSnapshot, ResourcesView, TrainableInfo,
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
}

impl PlayerState {
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.faction)
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
    pub dirty: bool,
    /// Frames with little/no progress toward the goal.
    pub stuck_frames: u16,
    /// Temporary escape waypoint when pathing is jammed.
    pub detour: Option<(f32, f32)>,
    /// Remaining ticks to honor the detour before forcing a re-aim at the true goal.
    pub detour_ttl: u16,
    /// Last escape heading (radians) — avoid picking the same jam twice.
    pub last_escape_ang: f32,
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
            hp: 1200.0,
        },
        BuildDef {
            kind: "barracks",
            name: "Barracks",
            cost_supplies: 600,
            cost_fuel: 0,
            cost_munitions: 200,
            build_ms: 10_000,
            power: -20,
            hp: 1500.0,
        },
        BuildDef {
            kind: "war_factory",
            name: "War Factory",
            cost_supplies: 1200,
            cost_fuel: 400,
            cost_munitions: 400,
            build_ms: 14_000,
            power: -30,
            hp: 2000.0,
        },
        BuildDef {
            kind: "supply",
            name: "Supply Center",
            cost_supplies: 500,
            cost_fuel: 0,
            cost_munitions: 0,
            build_ms: 7_000,
            power: -10,
            hp: 1000.0,
        },
        BuildDef {
            kind: "turret",
            name: "Patriot Battery",
            cost_supplies: 700,
            cost_fuel: 0,
            cost_munitions: 500,
            build_ms: 9_000,
            power: -15,
            hp: 900.0,
        },
    ]
}

pub fn trainables() -> &'static [UnitDef] {
    // Scale: HQ visual ~1.4 world units ≈ ~20 m → 1 wu ≈ 14 m.
    // Speeds are world-units / second (applied each tick as speed * dt).
    &[
        UnitDef {
            unit: "ranger",
            name: "Ranger",
            from_building: "barracks",
            cost_supplies: 150,
            cost_fuel: 0,
            cost_munitions: 50,
            train_ms: 4_000,
            hp: 120.0,
            damage: 12.0,
            // ~5 m/s jog → ≈ 0.35 wu/s
            speed: 0.35,
            range: 4.0,
        },
        UnitDef {
            unit: "missile_defender",
            name: "Missile Defender",
            from_building: "barracks",
            cost_supplies: 200,
            cost_fuel: 0,
            cost_munitions: 100,
            train_ms: 5_000,
            hp: 100.0,
            damage: 18.0,
            // heavier infantry ~4 m/s
            speed: 0.28,
            range: 7.0,
        },
        UnitDef {
            unit: "tank",
            name: "Crusader Tank",
            from_building: "war_factory",
            cost_supplies: 700,
            cost_fuel: 200,
            cost_munitions: 200,
            train_ms: 10_000,
            hp: 500.0,
            damage: 40.0,
            // combat pace ~8 m/s
            speed: 0.55,
            range: 5.5,
        },
        UnitDef {
            unit: "tank_desert",
            name: "Desert Crusader",
            from_building: "war_factory",
            cost_supplies: 700,
            cost_fuel: 200,
            cost_munitions: 200,
            train_ms: 10_000,
            hp: 500.0,
            damage: 40.0,
            speed: 0.55,
            range: 5.5,
        },
    ]
}

/// Client `BUILDING_MODELS[].target` — max visual dimension after fit.
fn building_visual_size(kind: &str) -> f32 {
    match kind {
        "hq" => 1.4,
        "war_factory" => 1.35,
        "barracks" => 0.85,
        "power_plant" | "supply" => 1.1,
        "turret" => 0.9,
        _ => 1.0,
    }
}

/// Horizontal collision radius from visual size (not a fat generic circle).
pub fn building_radius(kind: &str) -> f32 {
    // Footprint is roughly square; half-extent ≈ 0.40–0.45 of fitted max dim.
    building_visual_size(kind) * 0.42
}

/// Matches client `unitDims` half-extent on the ground plane.
pub fn unit_radius(kind: &str) -> f32 {
    if kind.contains("tank") {
        // box ~0.20 × 0.26 → ~0.13
        0.13
    } else if kind.contains("missile") {
        0.05
    } else {
        // ranger / infantry box ~0.07 × 0.07
        0.045
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
fn collision_pad() -> f32 {
    0.04
}

pub struct MatchSim {
    pub id: Uuid,
    pub map_size: u16,
    pub ffa: bool,
    pub tick: u64,
    pub players: HashMap<Uuid, PlayerState>,
    pub entities: HashMap<Uuid, Entity>,
    pub removed: Vec<Uuid>,
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
            removed: Vec::new(),
            ended: false,
            winner_team: None,
            end_reason: String::new(),
            created_at: Instant::now(),
            max_duration: Duration::from_secs(30 * 60),
            stream_jobs: VecDeque::new(),
        };

        for (user_id, name, faction, team, flag) in roster.into_iter() {
            let (x, y) = sim.allocate_spawn_xy();
            let slot = sim.players.len();
            let colors = color_scheme_for_slot(slot);

            sim.players.insert(
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
                    connected: true,
                    aoi_known: HashSet::new(),
                    explored: aoi::ExploredMap::new(map_size),
                },
            );

            let hq_id = Uuid::new_v4();
            sim.entities.insert(
                hq_id,
                Entity {
                    id: hq_id,
                    kind: "hq".into(),
                    owner: user_id,
                    team,
                    x,
                    y,
                    hp: 5000.0,
                    max_hp: 5000.0,
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
                    dirty: true,
                    stuck_frames: 0,
                    detour: None,
                    detour_ttl: 0,
                    last_escape_ang: 0.0,
                },
            );
            sim.reveal_vision_for(user_id);
        }

        sim
    }

    /// Place new HQs in a tight cluster near existing players (not spread across the map).
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

        // ~12 tile ring spacing so bases sit close but don't stack.
        const MIN_SEP: f32 = 12.0;
        const GOLDEN: f32 = 2.399_963;

        for k in 0..96 {
            let r = if hq_positions.is_empty() {
                0.0
            } else {
                MIN_SEP + (k as f32).sqrt() * 3.5
            };
            let angle = k as f32 * GOLDEN;
            let x = (bx + angle.cos() * r).clamp(4.0, map - 5.0);
            let y = (by + angle.sin() * r).clamp(4.0, map - 5.0);
            let ix = x.floor() as i32;
            let iy = y.floor() as i32;

            let blocked = self.entities.values().any(|e| {
                if !e.building {
                    return false;
                }
                let dx = e.x - (ix as f32 + 0.5);
                let dy = e.y - (iy as f32 + 0.5);
                dx * dx + dy * dy < (MIN_SEP * 0.85) * (MIN_SEP * 0.85)
            });
            if !blocked {
                return (ix as f32 + 0.5, iy as f32 + 0.5);
            }
        }

        (
            bx.clamp(4.0, map - 5.0),
            by.clamp(4.0, map - 5.0),
        )
    }

    /// Mid-match join: spawn HQ near the existing player cluster.
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

        let (x, y) = self.allocate_spawn_xy();
        let colors = color_scheme_for_slot(index);

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
                connected: true,
                aoi_known: HashSet::new(),
                explored: aoi::ExploredMap::new(self.map_size),
            },
        );

        let hq_id = Uuid::new_v4();
        self.entities.insert(
            hq_id,
            Entity {
                id: hq_id,
                kind: "hq".into(),
                owner: user_id,
                team,
                x,
                y,
                hp: 5000.0,
                max_hp: 5000.0,
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
                dirty: true,
                stuck_frames: 0,
                detour: None,
                detour_ttl: 0,
                last_escape_ang: 0.0,
            },
        );
        self.reveal_vision_for(user_id);

        Ok(())
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
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
        let entities = aoi::visible_entities(self.entities.values(), user_id)
            .into_iter()
            .map(|e| self.entity_view(e))
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
        let player = self.players.get_mut(&user_id).ok_or("Not in match")?;
        if !player.alive {
            return Err("Eliminated");
        }

        let fx = x as f32 + 0.5;
        let fy = y as f32 + 0.5;
        if x < 0 || y < 0 || x >= self.map_size as i32 || y >= self.map_size as i32 {
            return Err("Out of bounds");
        }

        let place_r = building_radius(kind);
        let blocked = self.entities.values().any(|e| {
            if !e.building {
                return false;
            }
            let other_r = building_radius(&e.kind);
            let dx = e.x - fx;
            let dy = e.y - fy;
            let min_dist = place_r + other_r + collision_pad();
            dx * dx + dy * dy < min_dist * min_dist
        });
        if blocked {
            return Err("Tile occupied");
        }

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

        let id = Uuid::new_v4();
        self.entities.insert(
            id,
            Entity {
                id,
                kind: def.kind.into(),
                owner: user_id,
                team: player.team,
                x: fx,
                y: fy,
                hp: def.hp,
                max_hp: def.hp,
                building: true,
                unit: false,
                flag: player.flag.clone(),
                build_remaining_ms: def.build_ms,
                train_queue: VecDeque::new(),
                target: None,
                move_to: None,
                speed: 0.0,
                damage: if def.kind == "turret" { 25.0 } else { 0.0 },
                range: if def.kind == "turret" { 10.0 } else { 0.0 },
                attack_cooldown_ms: 0,
                dirty: true,
                stuck_frames: 0,
                detour: None,
                detour_ttl: 0,
                last_escape_ang: 0.0,
            },
        );

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
        for id in ids {
            if let Some(entity) = self.entities.get_mut(id) {
                if entity.owner == user_id && entity.unit && entity.build_remaining_ms == 0 {
                    entity.move_to = Some((tx, ty));
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
            let Some(mut entity) = self.entities.remove(&id) else {
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
                                dirty: true,
                                stuck_frames: 0,
                                detour: None,
                                detour_ttl: 0,
                                last_escape_ang: 0.0,
                            };
                            self.entities.insert(uid, spawn);
                        }
                    }
                }
            }

            // Turrets auto-acquire.
            if entity.kind == "turret" && entity.build_remaining_ms == 0 {
                if entity.attack_cooldown_ms > 0 {
                    entity.attack_cooldown_ms =
                        entity.attack_cooldown_ms.saturating_sub(dt_ms);
                } else {
                    let team = entity.team;
                    let range = entity.range;
                    let ex = entity.x;
                    let ey = entity.y;
                    let dmg = entity.damage;
                    let mut best: Option<(Uuid, f32)> = None;
                    for other in self.entities.values() {
                        if other.team == team || !other.unit {
                            continue;
                        }
                        let dx = other.x - ex;
                        let dy = other.y - ey;
                        let dist = (dx * dx + dy * dy).sqrt();
                        if dist <= range {
                            if best.map(|(_, d)| dist < d).unwrap_or(true) {
                                best = Some((other.id, dist));
                            }
                        }
                    }
                    if let Some((tid, _)) = best {
                        if let Some(t) = self.entities.get_mut(&tid) {
                            t.hp -= dmg;
                            t.dirty = true;
                        }
                        entity.attack_cooldown_ms = 700;
                        entity.dirty = true;
                    }
                }
            }

            self.entities.insert(id, entity);
        }

        // Units move after buildings are all back in the map (solid obstacles).
        let unit_ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.unit)
            .map(|e| e.id)
            .collect();
        for id in unit_ids {
            let Some(mut entity) = self.entities.remove(&id) else {
                continue;
            };

            let self_r = unit_radius(&entity.kind);
            let mut goal = entity.move_to;
            let mut hold_for_attack = false;
            if let Some(tid) = entity.target {
                if let Some(t) = self.entities.get(&tid) {
                    let tdx = t.x - entity.x;
                    let tdy = t.y - entity.y;
                    let tdist = (tdx * tdx + tdy * tdy).sqrt();
                    let stop_at = (entity.range - 0.35).max(self_r + entity_radius(t) * 0.35);
                    if tdist <= stop_at {
                        hold_for_attack = true;
                        goal = None;
                        entity.detour = None;
                        entity.detour_ttl = 0;
                        entity.stuck_frames = 0;
                    } else {
                        goal = Some((t.x, t.y));
                    }
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
                    let solid_units = !jammed;

                    // True goal arrival (never "arrive" at a detour as the final destination).
                    if entity.target.is_none() && (toward_goal <= step || toward_goal < 0.15) {
                        if !self.collides_at(entity.id, gx, gy, self_r, None, true) {
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
                            .steer_step(
                                entity.id,
                                entity.x,
                                entity.y,
                                gux,
                                guy,
                                step,
                                self_r,
                                ignore,
                                solid_units,
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
                                        .steer_step(
                                            entity.id,
                                            entity.x,
                                            entity.y,
                                            dux,
                                            duy,
                                            step,
                                            self_r,
                                            ignore,
                                            solid_units,
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
                            moved = self.steer_step_escape(
                                entity.id,
                                entity.x,
                                entity.y,
                                ux,
                                uy,
                                step,
                                self_r,
                                ignore,
                                entity.last_escape_ang,
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

                        // Brief detour only when jammed — TTL forces us back to the true goal.
                        if entity.stuck_frames >= 8 {
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

            if let Some(tid) = entity.target {
                if let Some(target) = self.entities.get(&tid) {
                    let dx = target.x - entity.x;
                    let dy = target.y - entity.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist <= entity.range && entity.attack_cooldown_ms == 0 {
                        entity.attack_cooldown_ms = 800;
                        entity.dirty = true;
                        let dmg = entity.damage;
                        if let Some(t) = self.entities.get_mut(&tid) {
                            t.hp -= dmg;
                            t.dirty = true;
                        }
                    }
                }
            }

            self.entities.insert(id, entity);
        }

        self.separate_units(dt_ms);
        self.clamp_entities_to_map();

        // Remove dead.
        let dead: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.hp <= 0.0)
            .map(|e| e.id)
            .collect();
        for id in dead {
            if let Some(entity) = self.entities.remove(&id) {
                self.removed.push(id);
                if entity.kind == "hq" {
                    if let Some(player) = self.players.get_mut(&entity.owner) {
                        player.alive = false;
                    }
                }
            }
        }

        self.check_victory();
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
        for other in self.entities.values() {
            if other.id == self_id || Some(other.id) == ignore {
                continue;
            }
            if !other.building && !other.unit {
                continue;
            }
            if other.unit && !solid_units {
                continue;
            }
            // Under-construction buildings still block.
            let other_r = entity_radius(other);
            let min_d = self_r + other_r + collision_pad();
            let dx = other.x - x;
            let dy = other.y - y;
            if dx * dx + dy * dy < min_d * min_d {
                return true;
            }
        }
        false
    }

    fn steer_step(
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
            if !self.collides_at(self_id, nx, ny, self_r, ignore, solid_units) {
                return Some((nx, ny));
            }
        }
        None
    }

    /// When jammed: biased random headings, prefer anything that isn't the last failed angle.
    fn steer_step_escape(
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
    ) -> Option<((f32, f32), f32)> {
        let mut rng = rand::thread_rng();
        // Soft on units so crowds don't freeze everyone.
        if let Some(p) = self.steer_step(self_id, x, y, ux, uy, step, self_r, ignore, false) {
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
            if !self.collides_at(self_id, nx, ny, self_r, ignore, false) {
                return Some(((nx, ny), ang));
            }
            // Also try a longer probe step for escaping pockets.
            let nx2 = x + ang.cos() * step * 1.6;
            let ny2 = y + ang.sin() * step * 1.6;
            if !self.collides_at(self_id, nx2, ny2, self_r, ignore, false) {
                return Some(((nx2, ny2), ang));
            }
        }

        // Last resort: any free micro-step, even repeating angles.
        for k in 0..12 {
            let ang = (k as f32) * (std::f32::consts::TAU / 12.0) + rng.gen_range(0.0..0.3);
            let nx = x + ang.cos() * step;
            let ny = y + ang.sin() * step;
            if !self.collides_at(self_id, nx, ny, self_r, ignore, false) {
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

    fn separate_units(&mut self, dt_ms: u32) {
        let ids: Vec<Uuid> = self
            .entities
            .values()
            .filter(|e| e.unit)
            .map(|e| e.id)
            .collect();
        let strength = 2.8 * (dt_ms as f32 / 1000.0);
        let mut pushes: HashMap<Uuid, (f32, f32)> = HashMap::new();

        for (i, &a_id) in ids.iter().enumerate() {
            let Some(a) = self.entities.get(&a_id) else {
                continue;
            };
            let ar = unit_radius(&a.kind);
            for &b_id in ids.iter().skip(i + 1) {
                let Some(b) = self.entities.get(&b_id) else {
                    continue;
                };
                let br = unit_radius(&b.kind);
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                let dist = (dx * dx + dy * dy).sqrt();
                let min_d = ar + br + collision_pad();
                if dist >= min_d || dist < 1e-4 {
                    continue;
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
            }
        }

        for (id, (px, py)) in pushes {
            let Some(entity) = self.entities.get(&id) else {
                continue;
            };
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
        }
    }

    fn clamp_entities_to_map(&mut self) {
        let map = self.map_size as f32;
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
            }
        }
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
    ) {
        let explored_new = self.reveal_vision_for(user_id);

        let Some(player) = self.players.get(&user_id) else {
            return (vec![], vec![], None, explored_new);
        };
        let resources = Some(player.resources.view());
        let previously_known = player.aoi_known.clone();

        let visible_ids: HashSet<Uuid> = aoi::visible_entities(self.entities.values(), user_id)
            .into_iter()
            .map(|e| e.id)
            .collect();

        let mut entities = Vec::new();
        for id in &visible_ids {
            let Some(entity) = self.entities.get(id) else {
                continue;
            };
            let entered_vision = !previously_known.contains(id);
            if entity.dirty
                || entity.unit
                || entity.build_remaining_ms > 0
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

        if let Some(player) = self.players.get_mut(&user_id) {
            player.aoi_known = visible_ids;
        }

        (entities, removed, resources, explored_new)
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
    }

    fn entity_view(&self, entity: &Entity) -> EntityView {
        let (owner_name, colors) = self
            .players
            .get(&entity.owner)
            .map(|p| (p.name.clone(), p.colors))
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
        }
    }
}
