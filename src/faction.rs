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
            "headquarters" => "Komuta Merkezi",
            "barracks" => "Kışla",
            "stable" => "Havaalanı",
            "workshop" => "Savaş Fabrikası",
            "academy" => "Strateji Merkezi",
            "smithy" => "Strateji Desteği",
            "rally_point" => "Toplanma Alanı",
            "statue" => "Radar Kubbesi",
            "market" => "İkmal Düşürme",
            "timber" => "Malzeme Yığını",
            "clay" => "Yakıt Deposu",
            "iron" => "Mühimmat Tesisi",
            "farm" => "Soğuk Füzyon Reaktörü",
            "warehouse" => "İkmal Merkezi",
            "hiding_place" => "Gözaltı Kampı",
            "wall" => "Hisar Bataryası",
            _ => "Bilinmeyen",
        },
        Faction::China => match kind {
            "headquarters" => "Komuta Merkezi",
            "barracks" => "Kışla",
            "stable" => "Havaalanı",
            "workshop" => "Savaş Fabrikası",
            "academy" => "Propaganda Merkezi",
            "smithy" => "Nükleer Laboratuvar",
            "rally_point" => "Seferberlik Sahası",
            "statue" => "Hoparlör Kulesi",
            "market" => "İnternet Merkezi",
            "timber" => "İkmal Sahası",
            "clay" => "Yakıt Deposu",
            "iron" => "Maden Tesisi",
            "farm" => "Nükleer Reaktör",
            "warehouse" => "İkmal Merkezi",
            "hiding_place" => "Sığınak",
            "wall" => "Gatling Topu",
            _ => "Bilinmeyen",
        },
        Faction::Gla => match kind {
            "headquarters" => "Komuta Merkezi",
            "barracks" => "Kışla",
            "workshop" => "Silah Taciri",
            "academy" => "Saray",
            "smithy" => "Tuzak Laboratuvarı",
            "rally_point" => "Toplanma Bayrağı",
            "statue" => "Sahte Yapı",
            "market" => "Kara Borsa",
            "timber" => "Hurda Sahası",
            "clay" => "Yakıt Stoku",
            "iron" => "Silah Stoku",
            "farm" => "İşçi Kampı",
            "warehouse" => "İkmal Deposu",
            "hiding_place" => "Tünel Ağı",
            "wall" => "Sungur Üssü",
            _ => "Bilinmeyen",
        },
    }
}

pub fn building_description(faction: Faction, kind: &str) -> &'static str {
    match faction {
        Faction::Usa => match kind {
            "headquarters" => "Türkiye ana komuta binası",
            "barracks" => "Komando ve havan eğitimi",
            "stable" => "F-16 Şahin, Chinook ve hava birimleri",
            "workshop" => "Altay ve kara araçları",
            "academy" => "Bombardıman, Arama-Yok Et, Hattı Tut",
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
            "wall" => "Hisar hava ve kara savunma bataryası",
            _ => "",
        },
        Faction::China => match kind {
            "headquarters" => "Çin ana komuta binası",
            "barracks" => "Kızıl Muhafız ve Tank Avcısı eğitimi",
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
            "wall" => "Gatling savunma",
            _ => "",
        },
        Faction::Gla => match kind {
            "headquarters" => "GLA ana komuta binası",
            "barracks" => "Asi, Fedai ve RPG askerleri",
            "workshop" => "Scorpion, Technical, Marauder ve Quad",
            "academy" => "Saray — elit birimler ve güçler",
            "smithy" => "Tuzak ve yıkım teknolojileri",
            "rally_point" => "Saldırı bayrağı",
            "statue" => "Sahte yapı / yanıltma",
            "market" => "Kara borsa geliri ve özel birimler",
            "timber" => "Hurda toplama",
            "clay" => "Yakıt stoku",
            "iron" => "Silah stoku",
            "farm" => "İşçi ve nüfus kampı",
            "warehouse" => "İkmal Deposu — ana depo",
            "hiding_place" => "Tünel ağı ve gizli stok",
            "wall" => "Sungur hava savunması",
            _ => "",
        },
    }
}

pub fn starting_blurb(faction: Faction) -> &'static str {
    match faction {
        Faction::Usa => {
            "Komuta Merkezi, Toplanma Alanı, Soğuk Füzyon Reaktörü, İkmal Merkezi ve Gözaltı Kampı hazır gelir."
        }
        Faction::China => {
            "Komuta Merkezi, Seferberlik Sahası, Nükleer Reaktör, İkmal Merkezi ve Sığınak hazır gelir."
        }
        Faction::Gla => {
            "Komuta Merkezi, Toplanma Bayrağı, İşçi Kampı, İkmal Deposu ve Tünel Ağı hazır gelir."
        }
    }
}
