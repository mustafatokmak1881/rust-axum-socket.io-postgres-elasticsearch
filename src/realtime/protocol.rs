use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello,
    CreateLobby {
        max_players: u8,
        map_size: u16,
        #[serde(default)]
        ffa: bool,
    },
    JoinLobby {
        lobby_id: Uuid,
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
        removed: Vec<Uuid>,
        resources: Option<ResourcesView>,
        focus_hint: Option<[f32; 2]>,
        /// Newly explored cell indices (y * map_size + x).
        explored_new: Vec<u16>,
    },
    MatchEnd {
        match_id: Uuid,
        winner_team: Option<u8>,
        reason: String,
        xp_gained: i64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenMatchView {
    pub id: Uuid,
    pub players: u8,
    pub max_players: u8,
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
    pub max_players: u8,
    pub map_size: u16,
    pub ffa: bool,
    pub phase: String,
    pub slots: Vec<LobbySlot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesView {
    pub supplies: i32,
    pub fuel: i32,
    pub munitions: i32,
    pub power: i32,
    pub power_used: i32,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSnapshot {
    pub match_id: Uuid,
    pub map_size: u16,
    pub tick: u64,
    pub you: Uuid,
    pub you_name: String,
    pub team: u8,
    pub ffa: bool,
    pub aoi_radius: f32,
    pub focus: [f32; 2],
    /// Packed little-endian u64 words of explored cells (row-major).
    pub explored: Vec<u8>,
    pub resources: ResourcesView,
    pub entities: Vec<EntityView>,
    pub buildable: Vec<BuildableInfo>,
    pub trainable: Vec<TrainableInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildableInfo {
    pub kind: String,
    pub name: String,
    pub cost_supplies: i32,
    pub cost_fuel: i32,
    pub cost_munitions: i32,
    pub build_ms: u32,
    pub power: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainableInfo {
    pub unit: String,
    pub name: String,
    pub from_building: String,
    pub cost_supplies: i32,
    pub cost_fuel: i32,
    pub cost_munitions: i32,
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
            id: "tank_desert_skin".into(),
            name: "Desert Tank Skin".into(),
            kind: "cosmetic_unit".into(),
            price_label: "$3.99".into(),
            description: "Visual only — same stats as standard tank.".into(),
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
