use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_faction() -> String {
    "usa".into()
}

pub fn normalize_faction(_faction: &str) -> String {
    // Locked to USA while the roster is rebuilt country-by-country.
    "usa".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello,
    CreateLobby {
        max_players: u16,
        map_size: u16,
        #[serde(default)]
        ffa: bool,
        /// `"usa"` | `"china"` | `"gla"` — applied when the match starts.
        #[serde(default = "default_faction")]
        faction: String,
    },
    JoinLobby {
        lobby_id: Uuid,
        #[serde(default = "default_faction")]
        faction: String,
    },
    LeaveLobby,
    SetFaction {
        faction: String,
    },
    Ready {
        ready: bool,
    },
    StartMatch,
    PlaceBuilding {
        kind: String,
        x: i32,
        y: i32,
    },
    TrainUnit {
        building_id: Uuid,
        unit: String,
    },
    /// Scrap an owned non-HQ building (partial gold refund + wreck FX).
    DemolishBuilding {
        building_id: Uuid,
    },
    MoveUnits {
        ids: Vec<Uuid>,
        x: f32,
        y: f32,
    },
    Attack {
        ids: Vec<Uuid>,
        target_id: Uuid,
    },
    EquipCosmetic {
        slot: String,
        id: String,
    },
    SetFocus {
        x: f32,
        y: f32,
    },
    /// Dev only: toggle personal full-map vision (does not affect other players).
    ToggleDebugVision,
    /// Ally-mode team chat (ignored in Alone / FFA).
    AllyChat {
        text: String,
    },
    Ping {
        n: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        user: crate::store::users::CurrentUser,
        entitlements: Vec<String>,
        catalog: Vec<CatalogItem>,
    },
    LobbyUpdate {
        lobby: LobbyView,
    },
    LobbyLeft,
    OpenMatches {
        matches: Vec<OpenMatchView>,
    },
    MatchStart {
        match_id: Uuid,
        snapshot: MatchSnapshot,
    },
    Delta {
        tick: u64,
        entities: Vec<EntityView>,
        /// Left this viewer's FOW / AOI (hide mesh — not a death).
        removed: Vec<Uuid>,
        /// Actually destroyed this tick (wreck FX). Distinct from FOW leave.
        #[serde(default)]
        died: Vec<Uuid>,
        resources: Option<ResourcesView>,
        focus_hint: Option<[f32; 2]>,
        /// Newly explored cell indices (y * map_size + x).
        explored_new: Vec<u16>,
        /// Attacks that happened since the last broadcast (muzzle / tracer FX).
        #[serde(default)]
        shots: Vec<ShotEvent>,
        /// Full commander roster — FOW-independent, ~1 Hz for Tab scoreboard.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoreboard: Option<Vec<ScoreboardRow>>,
        /// Dev: this client has personal full-map vision (M key). Never match-wide.
        #[serde(default)]
        global_vision: bool,
    },
    MatchEnd {
        match_id: Uuid,
        winner_team: Option<u8>,
        reason: String,
        xp_gained: i64,
        /// True if this client's team won (Ally) / this player won (FFA).
        #[serde(default)]
        you_won: bool,
        /// This commander's lifetime stats.
        #[serde(default)]
        you: Option<MatchPlayerStats>,
        /// Full roster for the end-screen table.
        #[serde(default)]
        roster: Vec<MatchPlayerStats>,
    },
    Error {
        message: String,
    },
    Pong {
        n: u64,
    },
    StoreOk {
        entitlements: Vec<String>,
    },
    /// Ally team chat, or Alone all-chat / `@name` whisper.
    AllyChat {
        from: Uuid,
        name: String,
        team: u8,
        text: String,
        /// Unix ms for client ordering / display.
        ts: u64,
        /// Alone mode private message (`@name`).
        #[serde(default)]
        whisper: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to_name: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenMatchView {
    pub id: Uuid,
    pub players: u16,
    pub max_players: u16,
    pub map_size: u16,
    pub ffa: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub price_label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbySlot {
    pub user_id: Uuid,
    pub name: String,
    pub faction: String,
    pub ready: bool,
    pub team: u8,
    pub flag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyView {
    pub id: Uuid,
    pub host_id: Uuid,
    pub max_players: u16,
    pub map_size: u16,
    pub ffa: bool,
    pub phase: String,
    pub slots: Vec<LobbySlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShotEvent {
    pub from: Uuid,
    pub to: Uuid,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub kind: String,
    /// False = tracer / shell flew wide (no damage).
    #[serde(default = "default_shot_hit")]
    pub hit: bool,
}

fn default_shot_hit() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MatchPlayerStats {
    pub id: Uuid,
    pub name: String,
    pub faction: String,
    pub team: u8,
    pub bot: bool,
    pub you: bool,
    pub won: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreboardRow {
    pub id: Uuid,
    pub name: String,
    pub faction: String,
    pub colors: [u32; 3],
    pub team: u8,
    pub alive: bool,
    pub bot: bool,
    pub you: bool,
    pub infantry: u32,
    pub tanks: u32,
    pub buildings: u32,
    /// Living Command Centers owned (home + captured).
    #[serde(default)]
    pub bases: u32,
    pub gold: i32,
    pub power: i32,
    pub power_used: i32,
    /// Living Command Center — Tab click jumps the camera here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hq_x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hq_y: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesView {
    pub gold: i32,
    pub power: i32,
    pub power_used: i32,
    /// Living + queued army size.
    #[serde(default)]
    pub units: u32,
    /// Max army from owned HQs: home + home/2 per extra base.
    #[serde(default)]
    pub units_cap: u32,
    /// Non-HQ buildings (finished + under construction).
    #[serde(default)]
    pub buildings: u32,
    /// Max structures from owned HQs (same x + x/2 rule).
    #[serde(default)]
    pub buildings_cap: u32,
    /// Living command centers owned.
    #[serde(default)]
    pub bases: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityView {
    pub id: Uuid,
    pub kind: String,
    pub owner: Uuid,
    pub owner_name: String,
    pub colors: [u32; 3],
    pub team: u8,
    pub x: f32,
    pub y: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub building: bool,
    pub unit: bool,
    pub flag: Option<String>,
    pub progress: Option<f32>,
    /// 0..1 while this building is training a unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub train_progress: Option<f32>,
    /// Infantry lying down in combat.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prone: bool,
    /// Building under hacker blackout.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hacked: bool,
    /// Current attack target — tanks/Patriot slew toward this before firing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aim_at: Option<Uuid>,
    /// Launcher / turret yaw (radians). Sent for tanks and Patriot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aim_yaw: Option<f32>,
    /// F-16 (and similar): true while on a sortie / RTB; false while hangared on the pad.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub airborne: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PondView {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountainView {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub match_id: Uuid,
    pub map_size: u16,
    pub tick: u64,
    pub you: Uuid,
    pub you_name: String,
    /// `"usa"` | `"china"` | `"gla"` — drives this player's build/train roster.
    #[serde(default = "default_faction")]
    pub you_faction: String,
    pub team: u8,
    pub ffa: bool,
    pub aoi_radius: f32,
    /// Dev: personal full-map vision for this viewer (M key). Default fog.
    #[serde(default)]
    pub global_vision: bool,
    pub focus: [f32; 2],
    /// Packed little-endian u64 words of explored cells (row-major).
    pub explored: Vec<u8>,
    pub resources: ResourcesView,
    pub entities: Vec<EntityView>,
    pub buildable: Vec<BuildableInfo>,
    pub trainable: Vec<TrainableInfo>,
    #[serde(default)]
    pub scoreboard: Vec<ScoreboardRow>,
    /// Impassable ponds (units + buildings).
    #[serde(default)]
    pub ponds: Vec<PondView>,
    /// Impassable mountain masses — ground units must path around.
    #[serde(default)]
    pub mountains: Vec<MountainView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildableInfo {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub faction: String,
    pub cost_gold: i32,
    pub build_ms: u32,
    pub power: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainableInfo {
    pub unit: String,
    pub name: String,
    #[serde(default)]
    pub faction: String,
    pub from_building: String,
    pub cost_gold: i32,
    pub train_ms: u32,
    pub hp: f32,
    pub damage: f32,
    pub speed: f32,
    pub range: f32,
}

pub fn default_catalog() -> Vec<CatalogItem> {
    vec![
        CatalogItem {
            id: "flag_gold".into(),
            name: "Gold Command Flag".into(),
            kind: "cosmetic_flag".into(),
            price_label: "$2.99 / or free grant in dev".into(),
            description: "Cosmetic flag on your HQ. No combat effect.".into(),
        },
        CatalogItem {
            id: "flag_stripe".into(),
            name: "Stripe Banner".into(),
            kind: "cosmetic_flag".into(),
            price_label: "$1.99".into(),
            description: "Cosmetic banner skin for buildings.".into(),
        },
        CatalogItem {
            id: "mlrs_camo_skin".into(),
            name: "M270 Woodland Camo".into(),
            kind: "cosmetic_unit".into(),
            price_label: "$3.99".into(),
            description: "Visual only — same stats as standard M270 MLRS.".into(),
        },
        CatalogItem {
            id: "xp_boost".into(),
            name: "Season XP Boost".into(),
            kind: "meta_boost".into(),
            price_label: "$4.99".into(),
            description: "+15% match XP (soft-capped). No in-match power.".into(),
        },
    ]
}
