//! Opening computer commanders: five nations that attack and defend on their own.

use rand::Rng;
use uuid::Uuid;

use super::grid::MAX_ENTITY_RADIUS;
use super::match_sim::{building_radius, MatchSim};

pub const OPENING_BOT_COUNT: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotStyle {
    /// Pushes the nearest enemy HQ early and keeps the pressure on.
    Aggressive,
    /// All-in rush; barely guards home.
    Reckless,
    /// Splits the army: raid + garrison.
    Balanced,
    /// Holds the base, turrets, intercepts, small counter-raids.
    Defensive,
    /// Sits tight until threatened, then dumps the army on the attacker.
    Counter,
}

#[derive(Clone, Debug)]
pub struct BotMind {
    pub style: BotStyle,
    pub war_owner: Option<Uuid>,
    pub next_wave: u64,
    pub last_move: u64,
}

struct BotProfile {
    name: &'static str,
    country: &'static str,
    faction: &'static str,
    style: BotStyle,
}

const PROFILES: [BotProfile; OPENING_BOT_COUNT] = [
    BotProfile {
        name: "Reeves",
        country: "USA",
        faction: "usa",
        style: BotStyle::Aggressive,
    },
    BotProfile {
        name: "Wei Feng",
        country: "China",
        faction: "china",
        style: BotStyle::Reckless,
    },
    BotProfile {
        name: "Rashid",
        country: "GLA",
        faction: "gla",
        style: BotStyle::Balanced,
    },
    BotProfile {
        name: "Hawke",
        country: "Britain",
        faction: "usa",
        style: BotStyle::Defensive,
    },
    BotProfile {
        name: "Volkova",
        country: "Russia",
        faction: "china",
        style: BotStyle::Counter,
    },
];

impl BotStyle {
    fn first_wave_tick(self) -> u64 {
        match self {
            BotStyle::Reckless => 40,
            BotStyle::Aggressive => 80,
            BotStyle::Balanced => 160,
            BotStyle::Counter => 280,
            BotStyle::Defensive => 420,
        }
    }

    fn rest_ticks(self) -> u64 {
        match self {
            BotStyle::Reckless => 50,
            BotStyle::Aggressive => 70,
            BotStyle::Balanced => 110,
            BotStyle::Counter => 90,
            BotStyle::Defensive => 180,
        }
    }

    /// Fraction of living units sent on the attack (rest garrison).
    fn assault_ratio(self, threatened: bool, hq_hurt: bool) -> f32 {
        match self {
            BotStyle::Reckless => {
                if hq_hurt {
                    0.55
                } else {
                    0.95
                }
            }
            BotStyle::Aggressive => {
                if threatened {
                    0.62
                } else {
                    0.88
                }
            }
            BotStyle::Balanced => {
                if threatened {
                    0.4
                } else {
                    0.58
                }
            }
            BotStyle::Defensive => {
                if threatened {
                    0.12
                } else {
                    0.28
                }
            }
            BotStyle::Counter => {
                if threatened || hq_hurt {
                    0.82
                } else {
                    0.18
                }
            }
        }
    }

    fn ranger_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 90,
            BotStyle::Aggressive => 75,
            BotStyle::Balanced => 65,
            BotStyle::Defensive => 48,
            BotStyle::Counter => 60,
        }
    }

    fn missile_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 0,
            BotStyle::Aggressive => 4,
            BotStyle::Balanced => 6,
            BotStyle::Defensive => 12,
            BotStyle::Counter => 8,
        }
    }

    fn tank_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 2,
            BotStyle::Aggressive => 4,
            BotStyle::Balanced => 4,
            BotStyle::Defensive => 2,
            BotStyle::Counter => 3,
        }
    }

    fn turrets(self) -> u32 {
        match self {
            BotStyle::Defensive => 3,
            BotStyle::Counter => 2,
            BotStyle::Balanced => 1,
            _ => 0,
        }
    }
}

pub fn seed_opening_bots(sim: &mut MatchSim) {
    for profile in &PROFILES {
        if sim.players.len() >= 100 {
            break;
        }
        let id = Uuid::new_v4();
        let team = 80 + sim.players.values().filter(|p| p.is_bot()).count() as u8;
        let name = format!("{} · {}", profile.name, profile.country);
        sim.spawn_commander(
            id,
            name,
            profile.faction.into(),
            team,
            None,
            false,
            Some(BotMind {
                style: profile.style,
                war_owner: None,
                next_wave: profile.style.first_wave_tick(),
                last_move: 0,
            }),
        );
    }
}

pub fn tick_bots(sim: &mut MatchSim) {
    if sim.ended {
        return;
    }
    let bots: Vec<Uuid> = sim
        .players
        .values()
        .filter(|p| p.alive && p.is_bot())
        .map(|p| p.user_id)
        .collect();
    for id in bots {
        let slot = (id.as_u128() % 10) as u64;
        if sim.tick % 10 != slot {
            continue;
        }
        think(sim, id);
    }
}

fn think(sim: &mut MatchSim, bot_id: Uuid) {
    let Some(player) = sim.players.get(&bot_id) else {
        return;
    };
    if !player.alive {
        return;
    }
    let style = player.bot.as_ref().map(|b| b.style).unwrap_or(BotStyle::Balanced);
    let team = player.team;
    let next_wave = player.bot.as_ref().map(|b| b.next_wave).unwrap_or(0);
    let last_move = player.bot.as_ref().map(|b| b.last_move).unwrap_or(0);

    let Some((hq_id, hx, hy, hq_hp, hq_max)) = own_hq(sim, bot_id) else {
        return;
    };
    let hq_hurt = hq_hp < hq_max * 0.82;
    let threatened = enemy_near(sim, team, hx, hy, 14.0);

    expand_base(sim, bot_id, style, hx, hy);
    train_army(sim, bot_id, style);

    let war = pick_war_target(sim, bot_id, team, style, hx, hy, threatened);
    if let Some(player) = sim.players.get_mut(&bot_id) {
        if let Some(mind) = player.bot.as_mut() {
            mind.war_owner = war.as_ref().map(|w| w.owner);
        }
    }

    let ready_to_wave = sim.tick >= next_wave;
    if !ready_to_wave && !threatened && !hq_hurt {
        if sim.tick.saturating_sub(last_move) >= 40 {
            hold_garrison(sim, bot_id, hx, hy, 1.0);
            if let Some(player) = sim.players.get_mut(&bot_id) {
                if let Some(mind) = player.bot.as_mut() {
                    mind.last_move = sim.tick;
                }
            }
        }
        return;
    }

    if sim.tick.saturating_sub(last_move) < 28 && !threatened {
        return;
    }

    let ratio = style.assault_ratio(threatened, hq_hurt);
    let mut units: Vec<Uuid> = sim
        .entities
        .values()
        .filter(|e| e.owner == bot_id && e.unit && e.hp > 0.0)
        .map(|e| e.id)
        .collect();
    units.sort_unstable();
    if units.is_empty() {
        return;
    }

    let assault_n = ((units.len() as f32) * ratio).round() as usize;
    let assault_n = assault_n.min(units.len());
    let (assault, garrison) = units.split_at(assault_n);

    if !garrison.is_empty() {
        hold_garrison_ids(sim, bot_id, hx, hy, garrison);
    }

    if !assault.is_empty() {
        if let Some(target) = war {
            sim.attack(bot_id, assault, target.id);
        } else {
            sim.move_units(bot_id, assault, hx + 4.0, hy);
        }
    }

    if let Some(player) = sim.players.get_mut(&bot_id) {
        if let Some(mind) = player.bot.as_mut() {
            mind.last_move = sim.tick;
            if ready_to_wave {
                mind.next_wave = sim.tick + style.rest_ticks();
            }
        }
    }
    let _ = hq_id;
}

#[derive(Clone)]
struct HqMark {
    id: Uuid,
    owner: Uuid,
    x: f32,
    y: f32,
    hp: f32,
}

fn own_hq(sim: &MatchSim, owner: Uuid) -> Option<(Uuid, f32, f32, f32, f32)> {
    sim.entities.values().find_map(|e| {
        if e.owner == owner && e.kind == "hq" && e.hp > 0.0 {
            Some((e.id, e.x, e.y, e.hp, e.max_hp))
        } else {
            None
        }
    })
}

fn pick_war_target(
    sim: &MatchSim,
    _bot_id: Uuid,
    team: u8,
    style: BotStyle,
    hx: f32,
    hy: f32,
    threatened: bool,
) -> Option<HqMark> {
    let mut hqs: Vec<HqMark> = sim
        .entities
        .values()
        .filter(|e| e.kind == "hq" && e.team != team && e.hp > 0.0)
        .map(|e| HqMark {
            id: e.id,
            owner: e.owner,
            x: e.x,
            y: e.y,
            hp: e.hp,
        })
        .collect();
    if hqs.is_empty() {
        return None;
    }

    if threatened {
        if let Some(attacker) = nearest_enemy_unit(sim, team, hx, hy) {
            if let Some(hq) = hqs.iter().find(|h| h.owner == attacker).cloned() {
                return Some(hq);
            }
        }
    }

    match style {
        BotStyle::Reckless | BotStyle::Aggressive => {
            hqs.sort_by(|a, b| a.hp.partial_cmp(&b.hp).unwrap_or(std::cmp::Ordering::Equal));
            hqs.into_iter().next()
        }
        BotStyle::Counter if threatened => {
            hqs.sort_by(|a, b| {
                dist2(hx, hy, a.x, a.y)
                    .partial_cmp(&dist2(hx, hy, b.x, b.y))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hqs.into_iter().next()
        }
        _ => {
            hqs.sort_by(|a, b| {
                dist2(hx, hy, a.x, a.y)
                    .partial_cmp(&dist2(hx, hy, b.x, b.y))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            hqs.into_iter().next()
        }
    }
}

fn nearest_enemy_unit(sim: &MatchSim, team: u8, x: f32, y: f32) -> Option<Uuid> {
    let mut best: Option<(Uuid, f32)> = None;
    sim.grid.for_each_nearby(x, y, 16.0, |id| {
        let Some(e) = sim.entities.get(&id) else {
            return false;
        };
        if e.team == team || e.hp <= 0.0 || !e.unit {
            return false;
        }
        let d = dist2(x, y, e.x, e.y);
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((e.owner, d));
        }
        false
    });
    best.map(|(owner, _)| owner)
}

fn enemy_near(sim: &MatchSim, team: u8, x: f32, y: f32, radius: f32) -> bool {
    let mut hit = false;
    sim.grid.for_each_nearby(x, y, radius + MAX_ENTITY_RADIUS, |id| {
        let Some(e) = sim.entities.get(&id) else {
            return false;
        };
        if e.team == team || e.hp <= 0.0 || !e.unit {
            return false;
        }
        if dist2(x, y, e.x, e.y) <= radius * radius {
            hit = true;
            return true;
        }
        false
    });
    hit
}

fn dist2(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

fn count_kind(sim: &MatchSim, owner: Uuid, kind: &str) -> u32 {
    sim.entities
        .values()
        .filter(|e| e.owner == owner && e.kind == kind && e.hp > 0.0)
        .count() as u32
}

fn count_units(sim: &MatchSim, owner: Uuid, pred: impl Fn(&str) -> bool) -> usize {
    sim.entities
        .values()
        .filter(|e| e.owner == owner && e.unit && e.hp > 0.0 && pred(&e.kind))
        .count()
}

fn expand_base(sim: &mut MatchSim, bot_id: Uuid, style: BotStyle, hx: f32, hy: f32) {
    let plants = count_kind(sim, bot_id, "power_plant");
    let barracks = count_kind(sim, bot_id, "barracks");
    let supply = count_kind(sim, bot_id, "supply");
    let factory = count_kind(sim, bot_id, "war_factory");
    let turrets = count_kind(sim, bot_id, "turret");

    if plants < 1 {
        try_place(sim, bot_id, "power_plant", hx, hy, 2.8);
        return;
    }
    if barracks < 1 {
        try_place(sim, bot_id, "barracks", hx, hy, 3.2);
        return;
    }
    if supply < 1 {
        try_place(sim, bot_id, "supply", hx, hy, 3.4);
        return;
    }
    if factory < 1 && (!matches!(style, BotStyle::Reckless) || sim.tick > 200) {
        try_place(sim, bot_id, "war_factory", hx, hy, 3.8);
        return;
    }
    if plants < 2 && sim.tick > 240 {
        try_place(sim, bot_id, "power_plant", hx, hy, 4.2);
        return;
    }
    if barracks < 2 && matches!(style, BotStyle::Reckless | BotStyle::Aggressive) && sim.tick > 180
    {
        try_place(sim, bot_id, "barracks", hx, hy, 4.0);
        return;
    }
    if turrets < style.turrets() {
        try_place(sim, bot_id, "turret", hx, hy, 5.2);
    }
}

fn try_place(sim: &mut MatchSim, bot_id: Uuid, kind: &str, hx: f32, hy: f32, radius: f32) {
    let mut rng = rand::thread_rng();
    let br = building_radius(kind);
    for k in 0..36 {
        let ang = (k as f32) * 0.7 + rng.gen_range(-0.2..0.2);
        let dist = radius + br + (k as f32) * 0.22;
        let x = (hx + ang.cos() * dist).floor() as i32;
        let y = (hy + ang.sin() * dist).floor() as i32;
        if sim.place_building(bot_id, kind, x, y).is_ok() {
            return;
        }
    }
}

fn train_army(sim: &mut MatchSim, bot_id: Uuid, style: BotStyle) {
    let rangers = count_units(sim, bot_id, |k| k == "ranger");
    let missiles = count_units(sim, bot_id, |k| k.contains("missile"));
    let tanks = count_units(sim, bot_id, |k| k.contains("tank"));

    let barracks: Vec<Uuid> = sim
        .entities
        .values()
        .filter(|e| {
            e.owner == bot_id
                && e.kind == "barracks"
                && e.build_remaining_ms == 0
                && e.hp > 0.0
                && e.train_queue.len() < 2
        })
        .map(|e| e.id)
        .collect();
    let factories: Vec<Uuid> = sim
        .entities
        .values()
        .filter(|e| {
            e.owner == bot_id
                && e.kind == "war_factory"
                && e.build_remaining_ms == 0
                && e.hp > 0.0
                && e.train_queue.is_empty()
        })
        .map(|e| e.id)
        .collect();

    for id in barracks {
        if missiles < style.missile_cap() && style.missile_cap() > 0 {
            if sim.train_unit(bot_id, id, "missile_defender").is_ok() {
                continue;
            }
        }
        if rangers < style.ranger_cap() {
            let _ = sim.train_unit(bot_id, id, "ranger");
        }
    }
    for id in factories {
        if tanks < style.tank_cap() {
            let _ = sim.train_unit(bot_id, id, "tank");
        }
    }
}

fn hold_garrison(sim: &mut MatchSim, bot_id: Uuid, hx: f32, hy: f32, ratio: f32) {
    let units: Vec<Uuid> = sim
        .entities
        .values()
        .filter(|e| e.owner == bot_id && e.unit && e.hp > 0.0)
        .map(|e| e.id)
        .collect();
    if units.is_empty() {
        return;
    }
    let n = ((units.len() as f32) * ratio).ceil() as usize;
    hold_garrison_ids(sim, bot_id, hx, hy, &units[..n.min(units.len())]);
}

fn hold_garrison_ids(sim: &mut MatchSim, bot_id: Uuid, hx: f32, hy: f32, ids: &[Uuid]) {
    if ids.is_empty() {
        return;
    }
    let mut rng = rand::thread_rng();
    let ang = rng.gen_range(0.0..std::f32::consts::TAU);
    let dist = rng.gen_range(2.8..5.2);
    let x = hx + ang.cos() * dist;
    let y = hy + ang.sin() * dist;
    sim.move_units(bot_id, ids, x, y);
}
