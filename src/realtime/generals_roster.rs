//! Türkiye başlangıç roster’ı — ince tech tree; binalar adım adım geri ekleniyor.
//! Ülke özel güçleri RA2 tarzı olacak (ör. Fransa Grand Cannon). Şimdilik TB2
//! Türkiye’nin imza SİHA’sı; diğer ülkelerin ekstra özellikleri sonra eklenecek.

use super::match_sim::{BuildDef, UnitDef};

#[inline]
pub fn faction_ok(allowed: &str, faction: &str) -> bool {
    allowed == "any" || allowed == faction
}

pub fn buildables() -> &'static [BuildDef] {
    &[
        BuildDef {
            kind: "power_plant",
            name: "Soğuk Füzyon Reaktörü",
            faction: "usa",
            cost_gold: 1_000,
            build_ms: 8_000,
            power: 100,
            hp: 1_800.0,
        },
        BuildDef {
            kind: "barracks",
            name: "Kışla",
            faction: "usa",
            cost_gold: 800,
            build_ms: 10_000,
            power: -20,
            hp: 2_200.0,
        },
        BuildDef {
            kind: "supply",
            name: "İkmal Merkezi",
            faction: "usa",
            cost_gold: 500,
            build_ms: 7_000,
            power: -10,
            hp: 1_500.0,
        },
        BuildDef {
            kind: "war_factory",
            name: "Savaş Fabrikası",
            faction: "usa",
            cost_gold: 2_800,
            build_ms: 14_000,
            power: -30,
            hp: 2_800.0,
        },
        BuildDef {
            kind: "turret",
            name: "Hisar Bataryası",
            faction: "usa",
            cost_gold: 1_200,
            build_ms: 10_000,
            power: -30,
            hp: 2_600.0,
        },
        BuildDef {
            kind: "airfield",
            name: "Havaalanı",
            faction: "usa",
            // Late unlock: needs 3 command centers (see AIRFIELD_MIN_BASES).
            // Large runway — hangar holds 4 jets max; Kaan sorties are decisive.
            cost_gold: 16_500,
            build_ms: 28_000,
            power: -80,
            hp: 4_800.0,
        },
    ]
}

pub fn trainables() -> &'static [UnitDef] {
    // Scale: HQ visual ~2.15 wu ≈ 22–28 m → 1 wu ≈ 12–13 m.
    // Power ladder: rifle < tank gun < MLRS saturation < Hisar guided.
    &[
        UnitDef {
            unit: "ranger",
            name: "Komando",
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
            // Unarmed infiltrator — stealth scout + building sabotage (no gunfight).
            unit: "spy",
            name: "Casus",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 520,
            train_ms: 9_000,
            hp: 240.0,
            damage: 0.0,
            speed: 0.24,
            range: 0.0,
            attack_ms: 0,
        },
        UnitDef {
            // Unarmed cyber specialist — stealth + hack (no gunfight).
            unit: "hacker",
            name: "Siber Operatör",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 680,
            train_ms: 11_000,
            hp: 180.0,
            damage: 0.0,
            speed: 0.18,
            range: 0.0,
            attack_ms: 0,
        },
        UnitDef {
            // Unarmed insurgent — stealth + revolt (no gunfight).
            unit: "terrorist",
            name: "Fedai",
            faction: "usa",
            from_building: "barracks",
            cost_gold: 600,
            train_ms: 10_000,
            hp: 220.0,
            damage: 0.0,
            speed: 0.21,
            range: 0.0,
            attack_ms: 0,
        },
        UnitDef {
            // Altay — ana muharebe tankı (görsel şimdilik aynı; sadece isim).
            // Cross-country ~48 km/h → speed 0.60 (game scale ≈50 km/h at 0.58).
            // M256 effective ~3.5–4 km (Desert Storm) → range 13.5 wu (compressed map).
            // Loader cadence ~6–8 s between aimed shots → attack_ms 6200.
            unit: "tank",
            name: "Altay",
            faction: "usa",
            from_building: "war_factory",
            cost_gold: 1_850,
            train_ms: 18_000,
            hp: 7_800.0,
            damage: 650.0,
            speed: 0.60,
            range: 13.5,
            attack_ms: 6_200,
        },
        UnitDef {
            // T-300 Kasırga — Türk ÇNRA; menzil/tempo M270 ölçeğinde.
            // Road ~64 km/h; combat pace under Altay → speed 0.55.
            // Soft aluminum cab — dies fast to tank guns.
            unit: "mlrs",
            name: "T-300 Kasırga",
            faction: "usa",
            from_building: "war_factory",
            cost_gold: 4_200,
            train_ms: 48_000,
            hp: 2_400.0,
            damage: 450.0,
            speed: 0.55,
            range: 22.0,
            attack_ms: 9_200,
        },
        UnitDef {
            // Kaan — hangared at Airfield. Each attack order burns a sortie fee
            // (see F16_SORTIE_GOLD); one bomb then RTB + rearm on the pad.
            // Mk84-class blast sized to erase a tight tank column (~10 MBT seat).
            unit: "f16",
            name: "Kaan",
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
        UnitDef {
            // Bayraktar TB2 — Türkiye’nin imza SİHA’sı (ülke özel gücü).
            // Persistent ISR (radar-like vision), 4× MAM-L; loiters when empty.
            // Cruise ~140 km/h → speed ~1.7; MAM-L ~8 km → range 14.5 wu.
            unit: "tb2",
            name: "Bayraktar TB2",
            faction: "usa",
            from_building: "airfield",
            cost_gold: 2_450,
            train_ms: 36_000,
            hp: 520.0,
            damage: 420.0,
            speed: 1.72,
            range: 14.5,
            attack_ms: 3_400,
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
        "f16" => 4,
        "tb2" => 4,
        _ => 6,
    }
}
