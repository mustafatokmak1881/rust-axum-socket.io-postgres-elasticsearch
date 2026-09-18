//! Opening computer commanders that attack and defend on their own.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::grid::MAX_ENTITY_RADIUS;
use super::match_sim::{building_radius, MatchSim, MAX_PLAYERS};

/// Seed bots up toward a full lobby (human already seated when MatchSim::new runs).
pub const OPENING_BOT_TARGET: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotStyle {
    /// Pushes with combined arms; still fights the army in the way.
    Aggressive,
    /// Attacks early, but intercepts and does not suicide into a bigger force.
    Reckless,
    /// Splits: main fight, building raid, garrison.
    Balanced,
    /// Holds the base, intercepts, small counter-raids.
    Defensive,
    /// Sits until threatened, then smashes the attacker — not a random HQ.
    Counter,
}

#[derive(Clone, Debug)]
pub struct BotMind {
    pub style: BotStyle,
    pub war_owner: Option<Uuid>,
    pub next_wave: u64,
    pub last_move: u64,
    /// Last living count per building kind — used to detect losses.
    pub seen_counts: HashMap<String, u32>,
    /// Don't slap the same building down the tick after it dies.
    pub rebuild_hold: HashMap<String, u64>,
}

struct BotProfile {
    name: &'static str,
    country: &'static str,
    faction: &'static str,
    style: BotStyle,
}

const PROFILES: [BotProfile; 9] = [
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
    BotProfile {
        name: "Okada",
        country: "Japan",
        faction: "usa",
        style: BotStyle::Balanced,
    },
    BotProfile {
        name: "Moreau",
        country: "France",
        faction: "china",
        style: BotStyle::Aggressive,
    },
    BotProfile {
        name: "Hassan",
        country: "GLA",
        faction: "gla",
        style: BotStyle::Defensive,
    },
    BotProfile {
        name: "Novak",
        country: "Serbia",
        faction: "china",
        style: BotStyle::Reckless,
    },
];

impl BotStyle {
    fn first_wave_tick(self) -> u64 {
        match self {
            BotStyle::Reckless => 40,
            BotStyle::Aggressive => 55,
            BotStyle::Balanced => 80,
            BotStyle::Counter => 110,
            BotStyle::Defensive => 140,
        }
    }

    fn rest_ticks(self) -> u64 {
        match self {
            BotStyle::Reckless => 45,
            BotStyle::Aggressive => 55,
            BotStyle::Balanced => 75,
            BotStyle::Counter => 65,
            BotStyle::Defensive => 100,
        }
    }

    fn react_ticks(self) -> u64 {
        match self {
            BotStyle::Reckless | BotStyle::Aggressive => 12,
            BotStyle::Balanced | BotStyle::Counter => 16,
            BotStyle::Defensive => 14,
        }
    }

    /// Field force vs home garrison. Home is never emptied if a fight is on.
    fn assault_ratio(self, threatened: bool, hq_hurt: bool) -> f32 {
        match self {
            BotStyle::Reckless => {
                if threatened || hq_hurt {
                    0.55
                } else {
                    0.85
                }
            }
            BotStyle::Aggressive => {
                if threatened {
                    0.55
                } else {
                    0.78
                }
            }
            BotStyle::Balanced => {
                if threatened {
                    0.45
                } else {
                    0.70
                }
            }
            BotStyle::Defensive => {
                if threatened {
                    0.30
                } else {
                    0.55
                }
            }
            BotStyle::Counter => {
                if threatened || hq_hurt {
                    0.80
                } else {
                    0.45
                }
            }
        }
    }

    fn min_push_power(self) -> f32 {
        match self {
            BotStyle::Reckless => 3.0,
            BotStyle::Aggressive => 3.5,
            BotStyle::Balanced => 4.0,
            BotStyle::Defensive => 3.0,
            BotStyle::Counter => 4.0,
        }
    }

    /// Minimum ready units before a home rally will march.
    fn min_push_squad(self) -> usize {
        match self {
            BotStyle::Reckless | BotStyle::Aggressive => 3,
            BotStyle::Balanced | BotStyle::Defensive => 3,
            BotStyle::Counter => 4,
        }
    }

    fn ranger_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 10,
            BotStyle::Aggressive => 8,
            BotStyle::Balanced => 8,
            BotStyle::Defensive => 10,
            BotStyle::Counter => 8,
        }
    }

    fn mortar_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 2,
            BotStyle::Aggressive => 2,
            BotStyle::Balanced => 3,
            BotStyle::Defensive => 3,
            BotStyle::Counter => 2,
        }
    }

    fn tank_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 16,
            BotStyle::Aggressive => 14,
            BotStyle::Balanced => 14,
            BotStyle::Defensive => 12,
            BotStyle::Counter => 14,
        }
    }

    fn abrams_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 8,
            BotStyle::Aggressive => 8,
            BotStyle::Balanced => 6,
            BotStyle::Defensive => 5,
            BotStyle::Counter => 7,
        }
    }

    fn mlrs_cap(self) -> usize {
        match self {
            BotStyle::Reckless => 4,
            BotStyle::Aggressive => 5,
            BotStyle::Balanced => 5,
            BotStyle::Defensive => 3,
            BotStyle::Counter => 4,
        }
    }

    fn turrets(self) -> u32 {
        match self {
            BotStyle::Defensive => 3,
            BotStyle::Counter => 2,
            BotStyle::Balanced => 2,
            BotStyle::Aggressive => 1,
            BotStyle::Reckless => 1,
        }
    }

    fn bunkers(self) -> u32 {
        match self {
            BotStyle::Defensive => 3,
            BotStyle::Counter => 2,
            BotStyle::Balanced => 1,
            BotStyle::Aggressive => 1,
            BotStyle::Reckless => 0,
        }
    }

    fn radars(self) -> u32 {
        match self {
            BotStyle::Defensive => 1,
            BotStyle::Counter => 1,
            BotStyle::Balanced => 1,
            BotStyle::Aggressive => 1,
            BotStyle::Reckless => 0,
        }
    }

    /// Ticks to wait after a building dies before even considering that kind again.
    fn rebuild_delay(self) -> u64 {
        match self {
            BotStyle::Reckless => 240,
            BotStyle::Aggressive => 300,
            BotStyle::Balanced => 380,
            BotStyle::Defensive => 340,
            BotStyle::Counter => 320,
        }
    }
}

pub fn seed_opening_bots(sim: &mut MatchSim) {
    let target = OPENING_BOT_TARGET.min(MAX_PLAYERS as usize);
    let mut n = 0usize;
    while sim.players.len() < target {
        let profile = &PROFILES[n % PROFILES.len()];
        let batch = n / PROFILES.len();
        let id = Uuid::new_v4();
        let team = if sim.ffa {
            80 + sim.players.values().filter(|p| p.is_bot()).count() as u8
        } else {
            0
        };
        let name = if batch == 0 {
            format!("{} · {}", profile.name, profile.country)
        } else {
            format!("{} {} · {}", profile.name, batch + 1, profile.country)
        };
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
                seen_counts: HashMap::new(),
                rebuild_hold: HashMap::new(),
            }),
        );
        n += 1;
        if n > 200 {
            break;
        }
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
        let slot = (id.as_u128() % 5) as u64;
        // ~4 Hz — fast enough to retask when a fight starts on the road.
        if sim.tick % 5 != slot {
            continue;
        }
        think(sim, id);
    }
}

#[derive(Clone)]
struct OwnedUnit {
    id: Uuid,
    x: f32,
    y: f32,
    tank: bool,
    mortar: bool,
    range: f32,
    target: Option<Uuid>,
    dest: Option<(f32, f32)>,
}

struct Contact {
    id: Uuid,
    owner: Uuid,
    x: f32,
    y: f32,
    tank: bool,
    building: bool,
    power: f32,
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

    let Some((_hq_id, hx, hy, hq_hp, hq_max)) = own_hq(sim, bot_id) else {
        return;
    };
    let hq_hurt = hq_hp < hq_max * 0.82;
    let threatened = enemy_near(sim, team, hx, hy, 16.0);

    expand_base(sim, bot_id, style, hx, hy, team, threatened);
    train_army(sim, bot_id, style, threatened);

    let own = collect_own(sim, bot_id);
    if own.is_empty() {
        return;
    }

    let home_fight = collect_enemies(sim, team, hx, hy, 16.0);
    let war = pick_war_target(sim, team, style, hx, hy, &home_fight);
    if let Some(player) = sim.players.get_mut(&bot_id) {
        if let Some(mind) = player.bot.as_mut() {
            mind.war_owner = war.as_ref().map(|w| w.owner);
        }
    }

    // 1) Instant: anyone who can see a fight, fights. Cancels a pointless march.
    let mut busy: HashSet<Uuid> = HashSet::new();
    react_contacts(sim, bot_id, team, &own, &mut busy);

    let free: Vec<OwnedUnit> = own
        .iter()
        .filter(|u| !busy.contains(&u.id))
        .cloned()
        .collect();
    let mut home = Vec::new();
    let mut field = Vec::new();
    for u in free {
        if dist2(u.x, u.y, hx, hy) < 8.5 * 8.5 {
            home.push(u);
        } else {
            field.push(u);
        }
    }

    // 2) Field squads: retreat, hold, or push as a mixed group — not one blob.
    for squad in spatial_groups(&field, 5.2) {
        command_squad(sim, bot_id, team, style, hx, hy, &squad, war.as_ref());
    }

    // 3) Home: rally idle troops; commit a wave without recalling outbound marchers.
    let strategic = sim.tick.saturating_sub(last_move) >= style.react_ticks();
    if strategic {
        let pushed = command_home(
            sim,
            bot_id,
            team,
            style,
            hx,
            hy,
            &home,
            threatened || hq_hurt,
            sim.tick >= next_wave,
            war.as_ref(),
        );
        if let Some(player) = sim.players.get_mut(&bot_id) {
            if let Some(mind) = player.bot.as_mut() {
                mind.last_move = sim.tick;
                if pushed {
                    mind.next_wave = sim.tick + style.rest_ticks();
                } else if sim.tick >= next_wave {
                    // Retry soon if the squad wasn't formed yet.
                    mind.next_wave = sim.tick + 20;
                }
            }
        }
    } else if threatened || !home_fight.is_empty() {
        let _ = command_home(
            sim,
            bot_id,
            team,
            style,
            hx,
            hy,
            &home,
            true,
            false,
            war.as_ref(),
        );
    }
}

fn react_contacts(
    sim: &mut MatchSim,
    bot_id: Uuid,
    team: u8,
    own: &[OwnedUnit],
    busy: &mut HashSet<Uuid>,
) {
    for u in own {
        let scan = (u.range + 2.2).max(5.0);
        let threats = collect_enemies(sim, team, u.x, u.y, scan);
        if threats.is_empty() {
            continue;
        }
        let Some(tid) = pick_target(u, &threats) else {
            continue;
        };
        order_attack(sim, bot_id, u, tid);
        busy.insert(u.id);
    }
}

fn pick_target(u: &OwnedUnit, threats: &[Contact]) -> Option<Uuid> {
    threats
        .iter()
        .max_by(|a, b| {
            score_target(u, a)
                .partial_cmp(&score_target(u, b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|c| c.id)
}

fn score_target(u: &OwnedUnit, c: &Contact) -> f32 {
    let d = dist2(u.x, u.y, c.x, c.y).sqrt();
    let mut s = 12.0 - d;
    if !c.building {
        s += 8.0;
    }
    if u.mortar && !c.tank && !c.building {
        s += 36.0; // HE loves soft targets
    }
    if u.mortar && c.building {
        s += 16.0;
    }
    if u.mortar && c.tank {
        s += 3.0; // poor vs armor
    }
    if u.tank && c.tank {
        s += 22.0;
    }
    if u.tank && c.building {
        s += 6.0;
    }
    s + c.power * 0.4
}

fn command_squad(
    sim: &mut MatchSim,
    bot_id: Uuid,
    team: u8,
    style: BotStyle,
    hx: f32,
    hy: f32,
    squad: &[OwnedUnit],
    war: Option<&HqMark>,
) {
    if squad.is_empty() {
        return;
    }
    let (cx, cy) = centroid(squad);
    let our_p: f32 = squad.iter().map(unit_power).sum();
    let local = collect_enemies(sim, team, cx, cy, 11.0);
    let enemy = strongest_cluster(&local);

    if let Some(en) = enemy.as_ref() {
        if our_p + 4.0 < en.power && !matches!(style, BotStyle::Reckless) {
            hold_facing(sim, bot_id, hx, hy, en.x, en.y, &ids_of(squad));
            return;
        }
        assign_combined_arms(sim, bot_id, squad, en.x, en.y, en.focus);
        return;
    }

    // No local fight: keep a useful destination, don't reshuffle every tick.
    let (tx, ty, standoff) = if let Some(w) = war {
        let front = collect_enemies(sim, team, w.x, w.y, 12.0);
        if let Some(en) = strongest_cluster(&front) {
            (en.x, en.y, 3.6)
        } else {
            // Still no contact — lock onto the enemy HQ and march/fight it.
            if let Some(hq_id) = hq_id_of(sim, w.owner) {
                for u in squad {
                    order_attack(sim, bot_id, u, hq_id);
                }
                return;
            }
            (w.x, w.y, 4.4)
        }
    } else {
        return;
    };
    let flank = flank_sign(bot_id, style);
    let (ax, ay) = approach_point(hx, hy, tx, ty, standoff, flank);
    let mut raid_ids: HashSet<Uuid> = HashSet::new();
    if squad.len() >= 9 {
        if let Some(soft) = soft_target(sim, team, war) {
            for u in squad.iter().filter(|u| !u.tank && !u.mortar).take(3) {
                order_attack(sim, bot_id, u, soft);
                raid_ids.insert(u.id);
            }
        }
    }
    let rest: Vec<OwnedUnit> = squad
        .iter()
        .filter(|u| !raid_ids.contains(&u.id))
        .cloned()
        .collect();
    assign_combined_arms(sim, bot_id, &rest, ax, ay, None);
}

fn assign_combined_arms(
    sim: &mut MatchSim,
    bot_id: Uuid,
    squad: &[OwnedUnit],
    tx: f32,
    ty: f32,
    focus: Option<Uuid>,
) {
    let tanks: Vec<&OwnedUnit> = squad.iter().filter(|u| u.tank).collect();
    let mortars: Vec<&OwnedUnit> = squad.iter().filter(|u| u.mortar).collect();
    let infantry: Vec<&OwnedUnit> = squad.iter().filter(|u| !u.tank && !u.mortar).collect();

    if let Some(tid) = focus {
        for u in mortars.iter().chain(tanks.iter()) {
            order_attack(sim, bot_id, u, tid);
        }
    } else {
        for u in &tanks {
            let stand = (u.range - 0.45).max(2.8);
            let (x, y) = approach_point(u.x, u.y, tx, ty, stand, 0.0);
            order_move(sim, bot_id, u, x, y, 1.6);
        }
        for u in &mortars {
            // Stand off and lob — don't rush the line with the tube.
            let stand = (u.range - 1.4).max(5.5);
            let (x, y) = approach_point(u.x, u.y, tx, ty, stand, 0.0);
            order_move(sim, bot_id, u, x, y, 2.0);
        }
    }

    let (sx, sy) = if !tanks.is_empty() {
        let (x, y) = centroid_refs(&tanks);
        let dx = tx - x;
        let dy = ty - y;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        (x + dx / len * 1.15, y + dy / len * 1.15)
    } else {
        (tx, ty)
    };
    for u in &infantry {
        order_move(sim, bot_id, u, sx, sy, 1.8);
    }
}

fn command_home(
    sim: &mut MatchSim,
    bot_id: Uuid,
    _team: u8,
    style: BotStyle,
    hx: f32,
    hy: f32,
    home: &[OwnedUnit],
    threatened: bool,
    ready_wave: bool,
    war: Option<&HqMark>,
) -> bool {
    if home.is_empty() {
        return false;
    }
    let face = war
        .as_ref()
        .map(|w| (w.x, w.y))
        .unwrap_or((hx + 5.0, hy));
    let dx = face.0 - hx;
    let dy = face.1 - hy;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let rally_x = hx + dx / len * 4.1;
    let rally_y = hy + dy / len * 4.1;

    // Already ordered out on a push — never yank them back to the rally.
    let idle: Vec<&OwnedUnit> = home
        .iter()
        .filter(|u| {
            if u.target.is_some() {
                return false;
            }
            if let Some((mx, my)) = u.dest {
                // Destination well away from HQ = outbound march.
                if dist2(mx, my, hx, hy) > 12.0 * 12.0 {
                    return false;
                }
            }
            true
        })
        .collect();

    if threatened {
        let intercept: Vec<Uuid> = idle.iter().map(|u| u.id).collect();
        if intercept.is_empty() {
            return false;
        }
        let n_guard =
            ((intercept.len() as f32) * (1.0 - style.assault_ratio(true, true))).round() as usize;
        let n_guard = n_guard.clamp(1, intercept.len().saturating_sub(1).max(1));
        let (guard, sorties) = intercept.split_at(n_guard.min(intercept.len()));
        hold_facing(sim, bot_id, hx, hy, face.0, face.1, guard);
        if !sorties.is_empty() {
            let (ax, ay) = approach_point(hx, hy, face.0, face.1, 2.6, 0.0);
            for id in sorties {
                if let Some(u) = idle.iter().find(|u| u.id == *id) {
                    order_move(sim, bot_id, u, ax, ay, 2.0);
                }
            }
        }
        return false;
    }

    if !ready_wave {
        // Only park idle fresh troops — do not cancel an outbound march.
        for u in &idle {
            if dist2(u.x, u.y, rally_x, rally_y) > 2.4 * 2.4 {
                order_move(sim, bot_id, u, rally_x, rally_y, 1.4);
            }
        }
        return false;
    }

    // Wave: take everyone idle at home (rally proximity is soft — small armies must leave).
    let ready: Vec<&OwnedUnit> = idle
        .iter()
        .copied()
        .filter(|u| {
            dist2(u.x, u.y, rally_x, rally_y) <= 5.5 * 5.5
                || dist2(u.x, u.y, hx, hy) <= 7.0 * 7.0
        })
        .collect();
    let power: f32 = ready.iter().map(|u| unit_power(u)).sum();
    let tanks = ready.iter().filter(|u| u.tank).count();
    let inf = ready.iter().filter(|u| !u.tank).count();
    let min_n = style.min_push_squad();
    let formed = ready.len() >= min_n
        && (power >= style.min_push_power() || tanks >= 1 || inf >= min_n);
    if !formed {
        for u in &idle {
            if dist2(u.x, u.y, rally_x, rally_y) > 2.4 * 2.4 {
                order_move(sim, bot_id, u, rally_x, rally_y, 1.4);
            }
        }
        return false;
    }

    let keep = ((ready.len() as f32) * (1.0 - style.assault_ratio(false, false))).round() as usize;
    let keep = keep.min(ready.len().saturating_sub(2));
    let mut ranked = ready;
    ranked.sort_by(|a, b| {
        role_push(b)
            .partial_cmp(&role_push(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let commit: Vec<OwnedUnit> = ranked
        .iter()
        .skip(keep)
        .map(|u| (*u).clone())
        .collect();
    if commit.is_empty() {
        return false;
    }
    let (tx, ty) = war.map(|w| (w.x, w.y)).unwrap_or((face.0, face.1));
    // Prefer a hard attack order on the enemy HQ so units chase and shoot it.
    if let Some(w) = war {
        if let Some(hq_id) = hq_id_of(sim, w.owner) {
            for u in &commit {
                order_attack(sim, bot_id, u, hq_id);
            }
            return true;
        }
    }
    let (ax, ay) = approach_point(hx, hy, tx, ty, 2.0, flank_sign(bot_id, style));
    assign_combined_arms(sim, bot_id, &commit, ax, ay, None);
    true
}

fn role_push(u: &OwnedUnit) -> f32 {
    if u.tank {
        3.0
    } else if u.mortar {
        2.0
    } else {
        1.0
    }
}

fn order_attack(sim: &mut MatchSim, bot_id: Uuid, u: &OwnedUnit, target: Uuid) {
    if u.target == Some(target) {
        return;
    }
    sim.attack(bot_id, &[u.id], target);
}

fn order_move(sim: &mut MatchSim, bot_id: Uuid, u: &OwnedUnit, x: f32, y: f32, slack: f32) {
    if u.target.is_some() {
        return;
    }
    if let Some((mx, my)) = u.dest {
        if dist2(mx, my, x, y) <= slack * slack {
            return;
        }
    }
    if dist2(u.x, u.y, x, y) <= slack * slack * 0.35 {
        return;
    }
    sim.move_units(bot_id, &[u.id], x, y);
}

fn spatial_groups(units: &[OwnedUnit], radius: f32) -> Vec<Vec<OwnedUnit>> {
    let r2 = radius * radius;
    let mut left: Vec<OwnedUnit> = units.to_vec();
    let mut groups = Vec::new();
    while let Some(seed) = left.pop() {
        let mut g = vec![seed];
        loop {
            let mut grew = false;
            left.retain(|u| {
                if g.iter().any(|v| dist2(u.x, u.y, v.x, v.y) <= r2) {
                    g.push(u.clone());
                    grew = true;
                    false
                } else {
                    true
                }
            });
            if !grew {
                break;
            }
        }
        groups.push(g);
    }
    groups
}

fn collect_own(sim: &MatchSim, bot_id: Uuid) -> Vec<OwnedUnit> {
    sim.entities
        .values()
        .filter(|e| e.owner == bot_id && e.unit && e.hp > 0.0)
        .map(|e| OwnedUnit {
            id: e.id,
            x: e.x,
            y: e.y,
            tank: e.kind.contains("tank"),
            mortar: e.kind.contains("mortar"),
            range: e.range,
            target: e.target,
            dest: e.move_to,
        })
        .collect()
}

fn collect_enemies(sim: &MatchSim, team: u8, x: f32, y: f32, radius: f32) -> Vec<Contact> {
    let mut out = Vec::new();
    sim.grid.for_each_nearby(x, y, radius + MAX_ENTITY_RADIUS, |id| {
        let Some(e) = sim.entities.get(&id) else {
            return false;
        };
        if e.team == team || e.hp <= 0.0 || !(e.unit || e.building) {
            return false;
        }
        if dist2(x, y, e.x, e.y) > radius * radius {
            return false;
        }
        out.push(Contact {
            id: e.id,
            owner: e.owner,
            x: e.x,
            y: e.y,
            tank: e.kind.contains("tank"),
            building: e.building,
            power: contact_power(e.unit, &e.kind),
        });
        false
    });
    out
}

fn unit_power(u: &OwnedUnit) -> f32 {
    if u.tank {
        6.0
    } else if u.mortar {
        2.4
    } else {
        1.0
    }
}

fn contact_power(unit: bool, kind: &str) -> f32 {
    if !unit {
        0.4
    } else if kind.contains("tank") {
        6.0
    } else if kind.contains("mortar") {
        2.4
    } else {
        1.0
    }
}

fn centroid(units: &[OwnedUnit]) -> (f32, f32) {
    let n = units.len().max(1) as f32;
    (
        units.iter().map(|u| u.x).sum::<f32>() / n,
        units.iter().map(|u| u.y).sum::<f32>() / n,
    )
}

fn centroid_refs(units: &[&OwnedUnit]) -> (f32, f32) {
    let n = units.len().max(1) as f32;
    (
        units.iter().map(|u| u.x).sum::<f32>() / n,
        units.iter().map(|u| u.y).sum::<f32>() / n,
    )
}

fn ids_of(units: &[OwnedUnit]) -> Vec<Uuid> {
    units.iter().map(|u| u.id).collect()
}

struct Cluster {
    x: f32,
    y: f32,
    power: f32,
    focus: Option<Uuid>,
}

fn strongest_cluster(contacts: &[Contact]) -> Option<Cluster> {
    let fighters: Vec<&Contact> = contacts.iter().filter(|c| !c.building || c.tank).collect();
    let pool: Vec<&Contact> = if fighters.is_empty() {
        contacts.iter().collect()
    } else {
        fighters
    };
    if pool.is_empty() {
        return None;
    }
    let mut best: Option<Cluster> = None;
    for seed in &pool {
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut p = 0.0;
        let mut n = 0.0;
        let mut focus = seed.id;
        let mut focus_p = seed.power;
        for c in &pool {
            if dist2(seed.x, seed.y, c.x, c.y) > 36.0 {
                continue;
            }
            sx += c.x * c.power.max(0.2);
            sy += c.y * c.power.max(0.2);
            p += c.power;
            n += c.power.max(0.2);
            if c.power > focus_p {
                focus_p = c.power;
                focus = c.id;
            }
        }
        if n <= 0.0 {
            continue;
        }
        let cluster = Cluster {
            x: sx / n,
            y: sy / n,
            power: p,
            focus: Some(focus),
        };
        if best.as_ref().map(|b| cluster.power > b.power).unwrap_or(true) {
            best = Some(cluster);
        }
    }
    best
}

fn flank_sign(bot_id: Uuid, style: BotStyle) -> f32 {
    let side = if bot_id.as_u128() % 2 == 0 { 1.0 } else { -1.0 };
    match style {
        BotStyle::Defensive => 0.0,
        BotStyle::Reckless => side * 1.4,
        _ => side * 2.2,
    }
}

fn approach_point(from_x: f32, from_y: f32, to_x: f32, to_y: f32, standoff: f32, flank: f32) -> (f32, f32) {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let ux = dx / len;
    let uy = dy / len;
    (
        to_x - ux * standoff + (-uy) * flank,
        to_y - uy * standoff + ux * flank,
    )
}

fn hold_facing(
    sim: &mut MatchSim,
    bot_id: Uuid,
    hx: f32,
    hy: f32,
    face_x: f32,
    face_y: f32,
    ids: &[Uuid],
) {
    if ids.is_empty() {
        return;
    }
    let dx = face_x - hx;
    let dy = face_y - hy;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let dist = len.clamp(2.8, 5.4);
    let x = hx + (dx / len) * dist;
    let y = hy + (dy / len) * dist;
    let need: Vec<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| {
            sim.entities.get(id).is_some_and(|e| {
                e.target.is_none()
                    && e.move_to
                        .map(|(mx, my)| dist2(mx, my, x, y) > 2.2 * 2.2)
                        .unwrap_or(true)
            })
        })
        .collect();
    if !need.is_empty() {
        sim.move_units(bot_id, &need, x, y);
    }
}

#[derive(Clone)]
struct HqMark {
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

fn hq_id_of(sim: &MatchSim, owner: Uuid) -> Option<Uuid> {
    sim.entities.values().find_map(|e| {
        if e.owner == owner && e.kind == "hq" && e.hp > 0.0 {
            Some(e.id)
        } else {
            None
        }
    })
}

fn pick_war_target(
    sim: &MatchSim,
    team: u8,
    style: BotStyle,
    hx: f32,
    hy: f32,
    home_fight: &[Contact],
) -> Option<HqMark> {
    let mut hqs: Vec<HqMark> = sim
        .entities
        .values()
        .filter(|e| e.kind == "hq" && e.team != team && e.hp > 0.0)
        .map(|e| HqMark {
            owner: e.owner,
            x: e.x,
            y: e.y,
            hp: e.hp,
        })
        .collect();
    if hqs.is_empty() {
        return None;
    }

    if let Some(attacker) = home_fight
        .iter()
        .filter(|c| !c.building)
        .min_by(|a, b| dist2(hx, hy, a.x, a.y).partial_cmp(&dist2(hx, hy, b.x, b.y)).unwrap_or(std::cmp::Ordering::Equal))
    {
        if let Some(hq) = hqs.iter().find(|h| h.owner == attacker.owner).cloned() {
            return Some(hq);
        }
    }

    match style {
        BotStyle::Reckless | BotStyle::Aggressive => {
            hqs.sort_by(|a, b| {
                let da = dist2(hx, hy, a.x, a.y);
                let db = dist2(hx, hy, b.x, b.y);
                (a.hp + da * 8.0)
                    .partial_cmp(&(b.hp + db * 8.0))
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

fn soft_target(sim: &MatchSim, team: u8, war: Option<&HqMark>) -> Option<Uuid> {
    let owner = war.map(|w| w.owner);
    let mut best: Option<(Uuid, f32)> = None;
    for e in sim.entities.values() {
        if e.team == team || e.hp <= 0.0 || !e.building {
            continue;
        }
        if e.kind == "hq" {
            continue;
        }
        if let Some(oid) = owner {
            if e.owner != oid {
                continue;
            }
        }
        let score = match e.kind.as_str() {
            "war_factory" => 0.0,
            "barracks" => 1.0,
            "supply" => 2.0,
            "power_plant" => 2.4,
            _ => 3.0,
        } + e.hp * 0.0001;
        if best.map(|(_, s)| score < s).unwrap_or(true) {
            best = Some((e.id, score));
        }
    }
    best.map(|(id, _)| id)
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

fn expand_base(
    sim: &mut MatchSim,
    bot_id: Uuid,
    style: BotStyle,
    hx: f32,
    hy: f32,
    team: u8,
    threatened: bool,
) {
    let faction = sim
        .players
        .get(&bot_id)
        .map(|p| p.faction.clone())
        .unwrap_or_else(|| "usa".into());

    let (power_kind, factory_kind, supply_kind, defense_kind, bunker_kind) = match faction.as_str() {
        "china" => (
            "nuclear_reactor",
            "war_factory",
            "supply",
            "gatling_cannon",
            "bunker",
        ),
        "gla" => (
            "power_plant", // unused — GLA has no power buildings
            "arms_dealer",
            "supply_stash",
            "stinger_site",
            "tunnel_network",
        ),
        _ => (
            "power_plant",
            "war_factory",
            "supply",
            "turret",
            "firebase",
        ),
    };

    // GLA has no power plants — treat as already powered.
    let plants = if faction == "gla" {
        1
    } else {
        count_kind(sim, bot_id, power_kind)
    };
    let barracks = count_kind(sim, bot_id, "barracks");
    let supply = count_kind(sim, bot_id, supply_kind);
    let factory = count_kind(sim, bot_id, factory_kind);
    let turrets = count_kind(sim, bot_id, defense_kind);
    let bunkers = count_kind(sim, bot_id, bunker_kind);
    let radars = count_kind(sim, bot_id, "radar");
    let now = sim.tick;

    if sim.entities.values().any(|e| {
        e.owner == bot_id && e.building && e.hp > 0.0 && e.build_remaining_ms > 0
    }) {
        return;
    }

    let snapshot = [
        (power_kind, plants),
        ("barracks", barracks),
        (supply_kind, supply),
        (factory_kind, factory),
        (defense_kind, turrets),
        (bunker_kind, bunkers),
        ("radar", radars),
    ];
    if let Some(mind) = sim.players.get_mut(&bot_id).and_then(|p| p.bot.as_mut()) {
        let delay = style.rebuild_delay();
        for (kind, count) in snapshot {
            let prev = mind.seen_counts.get(kind).copied().unwrap_or(count);
            if count < prev {
                mind.rebuild_hold.insert(kind.into(), now + delay);
            }
            mind.seen_counts.insert(kind.into(), count);
        }
    }

    let held = |kind: &str| -> bool {
        sim.players
            .get(&bot_id)
            .and_then(|p| p.bot.as_ref())
            .and_then(|m| m.rebuild_hold.get(kind).copied())
            .is_some_and(|until| now < until)
    };

    let army = count_units(sim, bot_id, |_| true);
    let need = if threatened {
        // Fight first. Only replace a missing production building in the rear.
        if factory == 0 && !held(factory_kind) {
            Some((factory_kind, 3.8))
        } else if barracks == 0 && army < 6 && !held("barracks") {
            Some(("barracks", 3.4))
        } else if plants == 0 && faction != "gla" && !held(power_kind) {
            Some((power_kind, 3.2))
        } else {
            None
        }
    } else if plants == 0 && faction != "gla" && !held(power_kind) {
        Some((power_kind, 2.8))
    } else if factory == 0 && !held(factory_kind) {
        Some((factory_kind, 3.6))
    } else if barracks == 0 && !held("barracks") {
        Some(("barracks", 3.2))
    } else if supply == 0 && !held(supply_kind) {
        Some((supply_kind, 3.4))
    } else if factory < 2 && now > 180 && !held(factory_kind) {
        Some((factory_kind, 4.2))
    } else if plants < 2 && faction != "gla" && now > 220 && !held(power_kind) {
        Some((power_kind, 4.2))
    } else if radars < style.radars() && faction != "gla" && now > 280 && !held("radar") {
        Some(("radar", 5.5))
    } else if bunkers < style.bunkers() && !held(bunker_kind) {
        Some((bunker_kind, 4.0))
    } else if turrets < style.turrets() && !held(defense_kind) {
        Some((defense_kind, 5.2))
    } else {
        None
    };

    let Some((kind, radius)) = need else {
        return;
    };
    let (tx, ty) = threat_xy(sim, team, hx, hy);
    try_place_away(sim, bot_id, kind, hx, hy, radius, tx, ty);
}

fn threat_xy(sim: &MatchSim, team: u8, hx: f32, hy: f32) -> (f32, f32) {
    let mut best_unit: Option<(f32, f32, f32)> = None;
    sim.grid.for_each_nearby(hx, hy, 22.0 + MAX_ENTITY_RADIUS, |id| {
        let Some(e) = sim.entities.get(&id) else {
            return false;
        };
        if e.team == team || e.hp <= 0.0 || !e.unit {
            return false;
        }
        let d = dist2(hx, hy, e.x, e.y);
        if best_unit.map(|(_, _, bd)| d < bd).unwrap_or(true) {
            best_unit = Some((e.x, e.y, d));
        }
        false
    });
    if let Some((x, y, _)) = best_unit {
        return (x, y);
    }
    let mut best_hq: Option<(f32, f32, f32)> = None;
    for e in sim.entities.values() {
        if e.kind != "hq" || e.team == team || e.hp <= 0.0 {
            continue;
        }
        let d = dist2(hx, hy, e.x, e.y);
        if best_hq.map(|(_, _, bd)| d < bd).unwrap_or(true) {
            best_hq = Some((e.x, e.y, d));
        }
    }
    best_hq
        .map(|(x, y, _)| (x, y))
        .unwrap_or((hx + 8.0, hy))
}

fn try_place_away(
    sim: &mut MatchSim,
    bot_id: Uuid,
    kind: &str,
    hx: f32,
    hy: f32,
    radius: f32,
    threat_x: f32,
    threat_y: f32,
) {
    let br = building_radius(kind);
    let tdx = threat_x - hx;
    let tdy = threat_y - hy;
    let tlen = (tdx * tdx + tdy * tdy).sqrt().max(0.001);
    let ux = tdx / tlen;
    let uy = tdy / tlen;
    let mut spots: Vec<(f32, i32, i32)> = Vec::with_capacity(48);
    for k in 0..48 {
        let ang = (k as f32) * 0.42;
        let dist = radius + br + ((k % 10) as f32) * 0.28;
        let px = hx + ang.cos() * dist;
        let py = hy + ang.sin() * dist;
        // Prefer the back side of the HQ relative to the threat.
        let toward_threat = ((px - hx) * ux + (py - hy) * uy) / dist.max(0.001);
        let score = -toward_threat;
        spots.push((score, px.floor() as i32, py.floor() as i32));
    }
    spots.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, x, y) in spots {
        if sim.place_building(bot_id, kind, x, y).is_ok() {
            return;
        }
    }
}

fn train_army(sim: &mut MatchSim, bot_id: Uuid, style: BotStyle, threatened: bool) {
    let faction = sim
        .players
        .get(&bot_id)
        .map(|p| p.faction.clone())
        .unwrap_or_else(|| "usa".into());

    let infantry = count_units(sim, bot_id, |k| {
        matches!(
            k,
            "ranger" | "red_guard" | "rebel" | "missile_defender" | "tank_hunter" | "rpg_trooper"
        )
    });
    let mbt = count_units(sim, bot_id, |k| {
        matches!(
            k,
            "tank" | "battlemaster" | "scorpion_tank" | "paladin_tank" | "marauder_tank" | "overlord"
        )
    });
    let support = count_units(sim, bot_id, |k| {
        k.contains("mlrs")
            || k.contains("tomahawk")
            || k.contains("inferno")
            || k.contains("scud")
            || k.contains("gatling")
            || k.contains("quad")
            || k.contains("buggy")
            || k.contains("humvee")
            || k.contains("technical")
    });

    let (inf_bldg, veh_bldg, mbt_unit, heavy_unit, arty_unit, scout_unit) = match faction.as_str() {
        "china" => (
            "barracks",
            "war_factory",
            "battlemaster",
            "overlord",
            "inferno_cannon",
            "red_guard",
        ),
        "gla" => (
            "barracks",
            "arms_dealer",
            "scorpion_tank",
            "marauder_tank",
            "rocket_buggy",
            "rebel",
        ),
        _ => (
            "barracks",
            "war_factory",
            "tank",
            "paladin_tank",
            "tomahawk",
            "ranger",
        ),
    };

    let barracks: Vec<Uuid> = sim
        .entities
        .values()
        .filter(|e| {
            e.owner == bot_id
                && e.kind == inf_bldg
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
                && e.kind == veh_bldg
                && e.build_remaining_ms == 0
                && e.hp > 0.0
                && e.train_queue.is_empty()
        })
        .map(|e| e.id)
        .collect();

    for id in factories {
        let heavies = count_units(sim, bot_id, |k| k == heavy_unit);
        if heavies < style.abrams_cap() && (threatened || mbt >= 2) {
            if sim.train_unit(bot_id, id, heavy_unit).is_ok() {
                continue;
            }
        }
        if mbt < style.tank_cap() {
            if sim.train_unit(bot_id, id, mbt_unit).is_ok() {
                continue;
            }
        }
        if support < style.mlrs_cap() && mbt >= 3 {
            let _ = sim.train_unit(bot_id, id, arty_unit);
        }
    }
    for id in barracks {
        let rocket = count_units(sim, bot_id, |k| {
            k.contains("defender") || k.contains("hunter") || k.contains("rpg")
        });
        if rocket < style.mortar_cap() && (threatened || mbt > 0) {
            let rocket_unit = match faction.as_str() {
                "china" => "tank_hunter",
                "gla" => "rpg_trooper",
                _ => "missile_defender",
            };
            if sim.train_unit(bot_id, id, rocket_unit).is_ok() {
                continue;
            }
        }
        if infantry < style.ranger_cap() && mbt >= infantry {
            let _ = sim.train_unit(bot_id, id, scout_unit);
        }
    }
}
