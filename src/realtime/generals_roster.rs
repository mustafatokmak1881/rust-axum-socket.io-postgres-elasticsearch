//! USA starter roster — slim tech tree while we re-add buildings step by step.
//! Later countries will share the same core weapons with unique extras (e.g. France Grand Cannon).

use super::match_sim::{BuildDef, UnitDef};

#[inline]
pub fn faction_ok(allowed: &str, faction: &str) -> bool {
    allowed == "any" || allowed == faction
}

pub fn buildables() -> &'static [BuildDef] {
    &[
        BuildDef {
            kind: "power_plant",
            name: "Cold Fusion Reactor",
            faction: "usa",
            cost_gold: 1_000,
            build_ms: 8_000,
            power: 100,
            hp: 1_800.0,
        },
        BuildDef {
            kind: "barracks",
            name: "Barracks",
            faction: "usa",
            cost_gold: 800,
            build_ms: 10_000,
            power: -20,
            hp: 2_200.0,
        },
        BuildDef {
            kind: "supply",
            name: "Supply Center",
            faction: "usa",
            cost_gold: 500,
            build_ms: 7_000,
            power: -10,
            hp: 1_500.0,
        },
        BuildDef {
            kind: "war_factory",
            name: "War Factory",
            faction: "usa",
            cost_gold: 2_800,
            build_ms: 14_000,
            power: -30,
            hp: 2_800.0,
        },
        BuildDef {
            kind: "turret",
            name: "Patriot Battery",
            faction: "usa",
            cost_gold: 1_200,
            build_ms: 10_000,
            power: -30,
            hp: 2_600.0,
        },
        BuildDef {
            kind: "airfield",
            name: "Airfield",
            faction: "usa",
            cost_gold: 5_500,
            build_ms: 22_000,
            power: -50,
            hp: 3_200.0,
        },
    ]
}

pub fn trainables() -> &'static [UnitDef] {
    // Scale: HQ visual ~2.15 wu ≈ 22–28 m → 1 wu ≈ 12–13 m.
    // Power ladder: rifle < tank gun < MLRS saturation < Patriot guided.
    &[
        UnitDef {
            unit: "ranger",
            name: "Ranger",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 265,
            train_ms: 5_000,
            hp: 280.0,
            damage: 85.0,
            speed: 0.20,
            range: 4.5,
            attack_ms: 800,
        },
        UnitDef {
            // Armed infiltrator — large intel vision + building sabotage.
            unit: "spy",
            name: "Spy",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 520,
            train_ms: 9_000,
            hp: 240.0,
            damage: 70.0,
            speed: 0.24,
            range: 4.0,
            attack_ms: 720,
        },
        UnitDef {
            // Cyber specialist — disable buildings, drain power, steal gold.
            unit: "hacker",
            name: "Hacker",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 680,
            train_ms: 11_000,
            hp: 180.0,
            damage: 28.0,
            speed: 0.18,
            range: 2.8,
            attack_ms: 1_100,
        },
        UnitDef {
            // Insurgent — convert nearby enemy infantry (revolt).
            unit: "terrorist",
            name: "Terrorist",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 600,
            train_ms: 10_000,
            hp: 220.0,
            damage: 60.0,
            speed: 0.21,
            range: 3.8,
            attack_ms: 850,
        },
        UnitDef {
            // M1A1 Abrams — real refs: GDLS / USMC factfile / AFV Database
            // Cross-country ~48 km/h → speed 0.60 (game scale ≈50 km/h at 0.58).
            // M256 effective ~3.5–4 km (Desert Storm) → range 13.5 wu (compressed map).
            // Loader cadence ~6–8 s between aimed shots → attack_ms 6200.
            unit: "tank",
            name: "M1A1 Abrams",
            faction: "usa",
            from_building: "war_factory",
            cost_gold: 1_600,
            train_ms: 16_000,
            hp: 7_800.0,
            damage: 650.0,
            speed: 0.60,
            range: 13.5,
            attack_ms: 6_200,
        },
        UnitDef {
            // M270 MLRS (M993 + M269) — FM 6-60 / AFV Database
            // Road ~64 km/h; combat pace under Abrams → speed 0.55.
            // M26 rockets 32–45 km → range 22 wu (compressed vs Abrams 13.5).
            // Soft aluminum cab — dies fast to tank guns.
            unit: "mlrs",
            name: "M270 MLRS",
            faction: "usa",
            from_building: "war_factory",
            cost_gold: 3_700,
            train_ms: 42_000,
            hp: 2_400.0,
            damage: 450.0,
            speed: 0.55,
            range: 22.0,
            attack_ms: 9_200,
        },
        UnitDef {
            // F-16C — hangared at Airfield. Each attack order burns a sortie fee
            // (see F16_SORTIE_GOLD); one bomb then RTB + rearm on the pad.
            // Mk84-class blast sized to erase a tight tank column (~10 MBT seat).
            unit: "f16",
            name: "F-16 Fighting Falcon",
            faction: "usa",
            from_building: "airfield",
            cost_gold: 3_400,
            train_ms: 48_000,
            hp: 1_600.0,
            damage: 8_800.0,
            speed: 10.8,
            range: 16.5,
            attack_ms: 2_600,
        },
    ]
}

/// Soft per-type caps (unused for training — HQ army budget is the sole gate).
#[allow(dead_code)]
pub fn army_cap_for(unit: &str) -> usize {
    match unit {
        "ranger" => 8,
        "spy" => 3,
        "hacker" => 2,
        "terrorist" => 3,
        "tank" => 10,
        "mlrs" => 3,
        "f16" => 2,
        _ => 6,
    }
}
