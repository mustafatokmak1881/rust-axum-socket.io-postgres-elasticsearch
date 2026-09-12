//! Generals fraksiyonları: USA, China, GLA.

use serde::Serialize;

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Faction {
    Usa,
    China,
    Gla,
}

impl Faction {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "usa" => Ok(Self::Usa),
            "china" => Ok(Self::China),
            "gla" => Ok(Self::Gla),
            _ => Err(AppError::BadRequest(
                "Geçersiz fraksiyon. usa, china veya gla seç.",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usa => "usa",
            Self::China => "china",
            Self::Gla => "gla",
        }
    }
}

/// Fraksiyona açık bina kind listesi (Generals hissi).
pub fn building_kinds(faction: Faction) -> &'static [&'static str] {
    match faction {
        Faction::Usa => &[
            "headquarters",
            "barracks",
            "stable",
            "workshop",
            "academy",
            "smithy",
            "rally_point",
            "statue",
            "market",
            "timber",
            "clay",
            "iron",
            "farm",
            "warehouse",
            "hiding_place",
            "wall",
        ],
        Faction::China => &[
            "headquarters",
            "barracks",
            "stable",
            "workshop",
            "academy",
            "smithy",
            "rally_point",
            "statue",
            "market",
            "timber",
            "clay",
            "iron",
            "farm",
            "warehouse",
            "hiding_place",
            "wall",
        ],
        // GLA'da klasik havaalanı yok; Arms Dealer + Palace + Tunnel + Stinger.
        Faction::Gla => &[
            "headquarters",
            "barracks",
            "workshop",
            "academy",
            "smithy",
            "rally_point",
            "statue",
            "market",
            "timber",
            "clay",
            "iron",
            "farm",
            "warehouse",
            "hiding_place",
            "wall",
        ],
    }
}

pub fn building_name(faction: Faction, kind: &str) -> &'static str {
    match faction {
        Faction::Usa => match kind {
            "headquarters" => "Command Center",
            "barracks" => "Barracks",
            "stable" => "Airfield",
            "workshop" => "War Factory",
            "academy" => "Strategy Center",
            "smithy" => "Strategy Support",
            "rally_point" => "Staging Area",
            "statue" => "Radar Dome",
            "market" => "Supply Drop",
            "timber" => "Supply Pile",
            "clay" => "Fuel Depot",
            "iron" => "Munitions Plant",
            "farm" => "Cold Fusion Reactor",
            "warehouse" => "Supply Center",
            "hiding_place" => "Detention Camp",
            "wall" => "Patriot Battery",
            _ => "Unknown",
        },
        Faction::China => match kind {
            "headquarters" => "Command Center",
            "barracks" => "Barracks",
            "stable" => "Airfield",
            "workshop" => "War Factory",
            "academy" => "Propaganda Center",
            "smithy" => "Nuclear Lab",
            "rally_point" => "Assembly Yard",
            "statue" => "Speaker Tower",
            "market" => "Internet Center",
            "timber" => "Supply Yard",
            "clay" => "Fuel Depot",
            "iron" => "Ore Plant",
            "farm" => "Nuclear Reactor",
            "warehouse" => "Supply Center",
            "hiding_place" => "Bunker",
            "wall" => "Gattling Cannon",
            _ => "Unknown",
        },
        Faction::Gla => match kind {
            "headquarters" => "Command Center",
            "barracks" => "Barracks",
            "workshop" => "Arms Dealer",
            "academy" => "Palace",
            "smithy" => "Demo Trap Lab",
            "rally_point" => "Rally Flag",
            "statue" => "Fake Structure",
            "market" => "Black Market",
            "timber" => "Scrap Yard",
            "clay" => "Fuel Cache",
            "iron" => "Arms Cache",
            "farm" => "Worker Compound",
            "warehouse" => "Supply Stash",
            "hiding_place" => "Tunnel Network",
            "wall" => "Stinger Site",
            _ => "Unknown",
        },
    }
}

pub fn building_description(faction: Faction, kind: &str) -> &'static str {
    match faction {
        Faction::Usa => match kind {
            "headquarters" => "USA ana komuta binası",
            "barracks" => "Ranger ve Missile Defender eğitimi",
            "stable" => "Raptor, Chinook ve hava birimleri",
            "workshop" => "Tank, Paladin ve kara araçları",
            "academy" => "Bombardment, Search & Destroy, Hold the Line",
            "smithy" => "Birim yükseltmeleri ve destek teknolojileri",
            "rally_point" => "Birliklerin toplanma noktası",
            "statue" => "Keşif ve radar kapsamı",
            "market" => "Acil ikmal ve lojistik",
            "timber" => "Malzeme üretimi",
            "clay" => "Yakıt üretimi",
            "iron" => "Mühimmat üretimi",
            "farm" => "Üs enerji kapasitesi",
            "warehouse" => "Kaynak depolama",
            "hiding_place" => "Korunan stoklar",
            "wall" => "Hava ve kara savunma bataryası",
            _ => "",
        },
        Faction::China => match kind {
            "headquarters" => "China ana komuta binası",
            "barracks" => "Red Guard ve Tank Hunter eğitimi",
            "stable" => "MiG ve Helix hava birimleri",
            "workshop" => "Battlemaster, Inferno ve Overlord",
            "academy" => "Propaganda ve nüfus moral etkisi",
            "smithy" => "Nükleer ve birim araştırmaları",
            "rally_point" => "Seferberlik alanı",
            "statue" => "Propaganda hoparlörü",
            "market" => "İstihbarat ve ağ etkisi",
            "timber" => "Malzeme üretimi",
            "clay" => "Yakıt üretimi",
            "iron" => "Maden üretimi",
            "farm" => "Nükleer enerji kapasitesi",
            "warehouse" => "Kaynak depolama",
            "hiding_place" => "Korugan depo",
            "wall" => "Gattling savunma",
            _ => "",
        },
        Faction::Gla => match kind {
            "headquarters" => "GLA ana komuta binası",
            "barracks" => "Rebel, Terrorist ve RPG askerleri",
            "workshop" => "Scorpion, Technical, Marauder ve Quad",
            "academy" => "GLA Palace — elit birimler ve güçler",
            "smithy" => "Tuzak ve yıkım teknolojileri",
            "rally_point" => "Saldırı bayrağı",
            "statue" => "Sahte yapı / yanıltma",
            "market" => "Kara borsa geliri ve özel birimler",
            "timber" => "Hurda toplama",
            "clay" => "Yakıt stoku",
            "iron" => "Silah stoku",
            "farm" => "İşçi ve nüfus kampı",
            "warehouse" => "Supply Stash — GLA ana deposu",
            "hiding_place" => "Tünel ağı ve gizli stok",
            "wall" => "Stinger hava savunması",
            _ => "",
        },
    }
}

pub fn starting_blurb(faction: Faction) -> &'static str {
    match faction {
        Faction::Usa => {
            "Command Center, Staging Area, Cold Fusion Reactor, Supply Center ve Detention Camp hazır gelir."
        }
        Faction::China => {
            "Command Center, Assembly Yard, Nuclear Reactor, Supply Center ve Bunker hazır gelir."
        }
        Faction::Gla => {
            "Command Center, Rally Flag, Worker Compound, Supply Stash ve Tunnel Network hazır gelir."
        }
    }
}
