use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use parking_lot::Mutex;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use super::match_sim::{self, MatchSim, MAX_PLAYERS};
use super::protocol::{
    ClientMsg, LobbySlot, LobbyView, ServerMsg, default_catalog,
};
use crate::store::{Redis, keys, users};
use crate::error::AppError;

pub type Outbox = mpsc::UnboundedSender<ServerMsg>;

#[derive(Clone)]
pub struct MatchHub {
    inner: Arc<HubInner>,
}

struct HubInner {
    redis: Redis,
    lobbies: DashMap<Uuid, Arc<Mutex<Lobby>>>,
    matches: DashMap<Uuid, Arc<RwLock<MatchRuntime>>>,
    /// user_id -> connection outbox
    connections: DashMap<Uuid, Outbox>,
    /// user_id -> lobby_id
    user_lobby: DashMap<Uuid, Uuid>,
    /// user_id -> match_id
    user_match: DashMap<Uuid, Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Lobby {
    id: Uuid,
    host_id: Uuid,
    max_players: u8,
    map_size: u16,
    ffa: bool,
    phase: String,
    slots: Vec<LobbyMember>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LobbyMember {
    user_id: Uuid,
    name: String,
    faction: String,
    ready: bool,
    team: u8,
    flag: Option<String>,
}

struct MatchRuntime {
    sim: MatchSim,
    /// subscribers in this match
    members: HashMap<Uuid, ()>,
}

impl MatchHub {
    pub fn new(redis: Redis) -> Self {
        Self {
            inner: Arc::new(HubInner {
                redis,
                lobbies: DashMap::new(),
                matches: DashMap::new(),
                connections: DashMap::new(),
                user_lobby: DashMap::new(),
                user_match: DashMap::new(),
            }),
        }
    }

    pub fn register(&self, user_id: Uuid, tx: Outbox) {
        self.inner.connections.insert(user_id, tx);
    }

    pub fn unregister(&self, user_id: Uuid) {
        self.inner.connections.remove(&user_id);
        if let Some(match_id) = self.inner.user_match.get(&user_id).map(|e| *e) {
            if let Some(runtime) = self.inner.matches.get(&match_id) {
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    let mut rt = runtime.write().await;
                    if let Some(player) = rt.sim.players.get_mut(&user_id) {
                        player.connected = false;
                    }
                });
            }
        }
    }

    pub async fn welcome(&self, user_id: Uuid) -> Result<ServerMsg, AppError> {
        let user = users::load_user(&self.inner.redis, user_id)
            .await?
            .ok_or(AppError::Unauthorized)?;
        let entitlements = users::list_entitlements(&self.inner.redis, user_id).await?;
        Ok(ServerMsg::Welcome {
            user: user.into(),
            entitlements,
            catalog: default_catalog(),
        })
    }

    pub async fn handle(&self, user_id: Uuid, msg: ClientMsg) {
        let result = self.handle_inner(user_id, msg).await;
        if let Err(message) = result {
            self.send(user_id, ServerMsg::Error { message });
        }
    }

    async fn handle_inner(&self, user_id: Uuid, msg: ClientMsg) -> Result<(), String> {
        match msg {
            ClientMsg::Hello => {
                if let Ok(welcome) = self.welcome(user_id).await {
                    self.send(user_id, welcome);
                }
            }
            ClientMsg::Ping { n } => self.send(user_id, ServerMsg::Pong { n }),
            ClientMsg::CreateLobby {
                max_players,
                map_size,
                ffa,
            } => {
                self.create_lobby(user_id, max_players, map_size, ffa)
                    .await?;
            }
            ClientMsg::JoinLobby { lobby_id } => {
                self.join_lobby(user_id, lobby_id).await?;
            }
            ClientMsg::LeaveLobby => self.leave_lobby(user_id).await?,
            ClientMsg::SetFaction { faction } => {
                self.set_faction(user_id, &faction).await?;
            }
            ClientMsg::Ready { ready } => self.set_ready(user_id, ready)?,
            ClientMsg::StartMatch => self.start_match(user_id).await?,
            ClientMsg::PlaceBuilding { kind, x, y } => {
                self.with_match_mut(user_id, |sim| {
                    sim.place_building(user_id, &kind, x, y)
                        .map_err(|e| e.to_string())
                })
                .await?;
            }
            ClientMsg::TrainUnit { building_id, unit } => {
                self.with_match_mut(user_id, |sim| {
                    sim.train_unit(user_id, building_id, &unit)
                        .map_err(|e| e.to_string())
                })
                .await?;
            }
            ClientMsg::MoveUnits { ids, x, y } => {
                self.with_match_mut(user_id, |sim| {
                    sim.move_units(user_id, &ids, x, y);
                    Ok(())
                })
                .await?;
            }
            ClientMsg::Attack { ids, target_id } => {
                self.with_match_mut(user_id, |sim| {
                    sim.attack(user_id, &ids, target_id);
                    Ok(())
                })
                .await?;
            }
            ClientMsg::SetFocus { x, y } => {
                self.with_match_mut(user_id, |sim| {
                    sim.set_focus(user_id, x, y);
                    Ok(())
                })
                .await?;
            }
            ClientMsg::EquipCosmetic { slot, id } => {
                self.equip_cosmetic(user_id, &slot, &id).await?;
            }
        }
        Ok(())
    }

    async fn create_lobby(
        &self,
        user_id: Uuid,
        max_players: u8,
        map_size: u16,
        ffa: bool,
    ) -> Result<(), String> {
        if self.inner.user_lobby.contains_key(&user_id)
            || self.inner.user_match.contains_key(&user_id)
        {
            return Err("Already in a lobby or match".into());
        }

        let max_players = max_players.clamp(2, MAX_PLAYERS);
        let map_size = map_size.clamp(48, 128);
        let user = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "User missing".to_string())?;

        let id = Uuid::new_v4();
        let lobby = Lobby {
            id,
            host_id: user_id,
            max_players,
            map_size,
            ffa,
            phase: "open".into(),
            slots: vec![LobbyMember {
                user_id,
                name: user.display_name.clone(),
                faction: user.faction.clone().unwrap_or_else(|| "usa".into()),
                ready: false,
                team: 0,
                flag: user.equipped_flag.clone(),
            }],
        };

        self.persist_lobby(&lobby).await;
        self.inner.lobbies.insert(id, Arc::new(Mutex::new(lobby)));
        self.inner.user_lobby.insert(user_id, id);
        self.broadcast_lobby(id);
        Ok(())
    }

    async fn join_lobby(&self, user_id: Uuid, lobby_id: Uuid) -> Result<(), String> {
        if self.inner.user_lobby.contains_key(&user_id)
            || self.inner.user_match.contains_key(&user_id)
        {
            return Err("Already in a lobby or match".into());
        }

        let user = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "User missing".to_string())?;

        let Some(lobby_arc) = self.inner.lobbies.get(&lobby_id).map(|e| e.clone()) else {
            return Err("Lobby not found".into());
        };

        {
            let mut lobby = lobby_arc.lock();
            if lobby.phase != "open" {
                return Err("Lobby not joinable".into());
            }
            if lobby.slots.len() as u8 >= lobby.max_players {
                return Err("Lobby full".into());
            }
            if lobby.slots.iter().any(|s| s.user_id == user_id) {
                return Err("Already joined".into());
            }
            let team = if lobby.ffa {
                lobby.slots.len() as u8
            } else {
                (lobby.slots.len() % 2) as u8
            };
            lobby.slots.push(LobbyMember {
                user_id,
                name: user.display_name,
                faction: user.faction.unwrap_or_else(|| "usa".into()),
                ready: false,
                team,
                flag: user.equipped_flag,
            });
            self.persist_lobby_sync(&lobby);
        }

        self.inner.user_lobby.insert(user_id, lobby_id);
        self.broadcast_lobby(lobby_id);
        Ok(())
    }

    async fn leave_lobby(&self, user_id: Uuid) -> Result<(), String> {
        let Some((_, lobby_id)) = self.inner.user_lobby.remove(&user_id) else {
            self.send(user_id, ServerMsg::LobbyLeft);
            return Ok(());
        };

        if let Some(lobby_arc) = self.inner.lobbies.get(&lobby_id).map(|e| e.clone()) {
            let mut empty = false;
            {
                let mut lobby = lobby_arc.lock();
                lobby.slots.retain(|s| s.user_id != user_id);
                if lobby.slots.is_empty() {
                    empty = true;
                } else if lobby.host_id == user_id {
                    lobby.host_id = lobby.slots[0].user_id;
                }
                if !empty {
                    self.persist_lobby_sync(&lobby);
                }
            }
            if empty {
                self.inner.lobbies.remove(&lobby_id);
                let mut conn = self.inner.redis.clone();
                let _: Result<(), _> = conn.del(keys::lobby(&lobby_id.to_string())).await;
                let _: Result<(), _> = conn.srem(keys::lobbies_index(), lobby_id.to_string()).await;
            } else {
                self.broadcast_lobby(lobby_id);
            }
        }

        self.send(user_id, ServerMsg::LobbyLeft);
        Ok(())
    }

    async fn set_faction(&self, user_id: Uuid, faction: &str) -> Result<(), String> {
        let faction = match faction.to_ascii_lowercase().as_str() {
            "usa" | "china" | "gla" => faction.to_ascii_lowercase(),
            _ => return Err("Invalid faction".into()),
        };

        if let Some(mut user) = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
        {
            user.faction = Some(faction.clone());
            users::save_user(&self.inner.redis, &user)
                .await
                .map_err(|e| e.to_string())?;
        }

        if let Some(lobby_id) = self.inner.user_lobby.get(&user_id).map(|e| *e) {
            if let Some(lobby_arc) = self.inner.lobbies.get(&lobby_id).map(|e| e.clone()) {
                {
                    let mut lobby = lobby_arc.lock();
                    if let Some(slot) = lobby.slots.iter_mut().find(|s| s.user_id == user_id) {
                        slot.faction = faction;
                        slot.ready = false;
                    }
                    self.persist_lobby_sync(&lobby);
                }
                self.broadcast_lobby(lobby_id);
            }
        }
        Ok(())
    }

    fn set_ready(&self, user_id: Uuid, ready: bool) -> Result<(), String> {
        let lobby_id = *self
            .inner
            .user_lobby
            .get(&user_id)
            .ok_or("Not in a lobby")?;
        let lobby_arc = self
            .inner
            .lobbies
            .get(&lobby_id)
            .map(|e| e.clone())
            .ok_or("Lobby missing")?;
        {
            let mut lobby = lobby_arc.lock();
            if let Some(slot) = lobby.slots.iter_mut().find(|s| s.user_id == user_id) {
                slot.ready = ready;
            }
            self.persist_lobby_sync(&lobby);
        }
        self.broadcast_lobby(lobby_id);
        Ok(())
    }

    async fn start_match(&self, user_id: Uuid) -> Result<(), String> {
        let lobby_id = *self
            .inner
            .user_lobby
            .get(&user_id)
            .ok_or("Not in a lobby")?;
        let lobby_arc = self
            .inner
            .lobbies
            .get(&lobby_id)
            .map(|e| e.clone())
            .ok_or("Lobby missing")?;

        let (roster, map_size, ffa, member_ids) = {
            let mut lobby = lobby_arc.lock();
            if lobby.host_id != user_id {
                return Err("Only host can start".into());
            }
            if lobby.slots.len() < 1 {
                return Err("Empty lobby".into());
            }
            // Practice: solo host gets a training bot opponent.
            if lobby.slots.len() == 1 {
                let bot_id = Uuid::new_v4();
                let team = if lobby.ffa { 1 } else { 1 };
                lobby.slots.push(LobbyMember {
                    user_id: bot_id,
                    name: "Training Drone".into(),
                    faction: "gla".into(),
                    ready: true,
                    team,
                    flag: None,
                });
            }
            if !lobby.slots.iter().all(|s| s.ready || s.user_id == lobby.host_id) {
                let host_id = lobby.host_id;
                for slot in &mut lobby.slots {
                    if slot.user_id == host_id {
                        slot.ready = true;
                    }
                }
            }
            if !lobby.slots.iter().all(|s| s.ready) {
                return Err("All players must ready".into());
            }
            lobby.phase = "starting".into();
            let roster: Vec<_> = lobby
                .slots
                .iter()
                .map(|s| {
                    (
                        s.user_id,
                        s.name.clone(),
                        s.faction.clone(),
                        s.team,
                        s.flag.clone(),
                    )
                })
                .collect();
            let members: Vec<Uuid> = lobby.slots.iter().map(|s| s.user_id).collect();
            self.persist_lobby_sync(&lobby);
            (roster, lobby.map_size, lobby.ffa, members)
        };

        let match_id = Uuid::new_v4();
        let sim = MatchSim::new(match_id, map_size, ffa, roster);
        let runtime = Arc::new(RwLock::new(MatchRuntime {
            sim,
            members: member_ids.iter().map(|id| (*id, ())).collect(),
        }));

        // Persist match meta + stream key (Redis Streams stand-in).
        {
            let mut conn = self.inner.redis.clone();
            let meta = serde_json::json!({
                "id": match_id,
                "map_size": map_size,
                "ffa": ffa,
                "players": member_ids.len(),
            });
            let _: Result<(), _> = conn
                .set_ex(
                    keys::match_meta(&match_id.to_string()),
                    meta.to_string(),
                    2 * 3600,
                )
                .await;
            let _: Result<(), _> = redis::cmd("XADD")
                .arg(keys::match_stream(&match_id.to_string()))
                .arg("MAXLEN")
                .arg("~")
                .arg(1000)
                .arg("*")
                .arg("event")
                .arg("match_start")
                .query_async(&mut conn)
                .await;
        }

        self.inner.matches.insert(match_id, runtime.clone());

        for uid in &member_ids {
            self.inner.user_lobby.remove(uid);
            self.inner.user_match.insert(*uid, match_id);
        }
        self.inner.lobbies.remove(&lobby_id);

        // Send snapshots
        {
            let rt = runtime.read().await;
            for uid in &member_ids {
                if let Some(snapshot) = rt.sim.snapshot_for(*uid) {
                    self.send(
                        *uid,
                        ServerMsg::MatchStart {
                            match_id,
                            snapshot,
                        },
                    );
                }
            }
        }

        self.spawn_match_loop(match_id, runtime);
        Ok(())
    }

    fn spawn_match_loop(&self, match_id: Uuid, runtime: Arc<RwLock<MatchRuntime>>) {
        let hub = self.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(1000 / match_sim::TICK_HZ as u64));
            loop {
                interval.tick().await;
                let ended = {
                    let mut rt = runtime.write().await;
                    rt.sim.tick_once();

                    // Push stream job completions marker
                    while let Some(front) = rt.sim.stream_jobs.front() {
                        if front.due_tick > rt.sim.tick {
                            break;
                        }
                        rt.sim.stream_jobs.pop_front();
                    }

                    if rt.sim.tick % match_sim::BROADCAST_EVERY as u64 == 0 {
                        let member_ids: Vec<Uuid> = rt.members.keys().copied().collect();
                        for uid in member_ids {
                            let (entities, removed, resources) = rt.sim.delta_for(uid);
                            let focus = rt.sim.players.get(&uid).map(|p| p.focus);
                            hub.send(
                                uid,
                                ServerMsg::Delta {
                                    tick: rt.sim.tick,
                                    entities,
                                    removed,
                                    resources,
                                    focus_hint: focus,
                                },
                            );
                        }
                        rt.sim.clear_frame_flags();
                    }

                    rt.sim.ended
                };

                if ended {
                    let (winner, reason, players) = {
                        let rt = runtime.read().await;
                        (
                            rt.sim.winner_team,
                            rt.sim.end_reason.clone(),
                            rt.sim.players.keys().copied().collect::<Vec<_>>(),
                        )
                    };

                    for uid in players {
                        let won = {
                            let rt = runtime.read().await;
                            rt.sim
                                .players
                                .get(&uid)
                                .map(|p| Some(p.team) == winner)
                                .unwrap_or(false)
                        };
                        let xp = hub.award_xp(uid, won).await;
                        hub.send(
                            uid,
                            ServerMsg::MatchEnd {
                                match_id,
                                winner_team: winner,
                                reason: reason.clone(),
                                xp_gained: xp,
                            },
                        );
                        hub.inner.user_match.remove(&uid);
                    }
                    hub.inner.matches.remove(&match_id);
                    break;
                }
            }
        });
    }

    async fn award_xp(&self, user_id: Uuid, won: bool) -> i64 {
        let mut base = if won { 120i64 } else { 50 };
        if let Ok(true) = users::has_entitlement(&self.inner.redis, user_id, "xp_boost").await {
            base = ((base as f64) * 1.15) as i64;
        }
        base = base.min(200);
        if let Ok(Some(mut user)) = users::load_user(&self.inner.redis, user_id).await {
            user.xp += base;
            let _ = users::save_user(&self.inner.redis, &user).await;
        }
        base
    }

    async fn equip_cosmetic(
        &self,
        user_id: Uuid,
        slot: &str,
        id: &str,
    ) -> Result<(), String> {
        if slot != "flag" {
            return Err("Unknown cosmetic slot".into());
        }
        let owned = users::has_entitlement(&self.inner.redis, user_id, id)
            .await
            .map_err(|e| e.to_string())?;
        if !owned && id != "flag_none" {
            return Err("Cosmetic not owned".into());
        }
        let mut user = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("User missing")?;
        user.equipped_flag = if id == "flag_none" {
            None
        } else {
            Some(id.to_owned())
        };
        users::save_user(&self.inner.redis, &user)
            .await
            .map_err(|e| e.to_string())?;

        let entitlements = users::list_entitlements(&self.inner.redis, user_id)
            .await
            .unwrap_or_default();
        self.send(user_id, ServerMsg::StoreOk { entitlements });
        Ok(())
    }

    async fn with_match_mut<F>(&self, user_id: Uuid, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut MatchSim) -> Result<(), String>,
    {
        let match_id = *self
            .inner
            .user_match
            .get(&user_id)
            .ok_or("Not in a match")?;
        let runtime = self
            .inner
            .matches
            .get(&match_id)
            .map(|e| e.clone())
            .ok_or("Match missing")?;
        let mut rt = runtime.write().await;
        f(&mut rt.sim)
    }

    pub fn send(&self, user_id: Uuid, msg: ServerMsg) {
        if let Some(tx) = self.inner.connections.get(&user_id) {
            let _ = tx.send(msg);
        }
    }

    fn broadcast_lobby(&self, lobby_id: Uuid) {
        let Some(lobby_arc) = self.inner.lobbies.get(&lobby_id).map(|e| e.clone()) else {
            return;
        };
        let view = {
            let lobby = lobby_arc.lock();
            lobby_view(&lobby)
        };
        let members: Vec<Uuid> = {
            let lobby = lobby_arc.lock();
            lobby.slots.iter().map(|s| s.user_id).collect()
        };
        for uid in members {
            self.send(
                uid,
                ServerMsg::LobbyUpdate {
                    lobby: view.clone(),
                },
            );
        }
    }

    async fn persist_lobby(&self, lobby: &Lobby) {
        let mut conn = self.inner.redis.clone();
        if let Ok(raw) = serde_json::to_string(lobby) {
            let _: Result<(), _> = conn
                .set_ex(keys::lobby(&lobby.id.to_string()), raw, 3600)
                .await;
            let _: Result<(), _> = conn
                .sadd(keys::lobbies_index(), lobby.id.to_string())
                .await;
        }
    }

    fn persist_lobby_sync(&self, lobby: &Lobby) {
        let redis = self.inner.redis.clone();
        let lobby = lobby.clone();
        tokio::spawn(async move {
            let mut conn = redis;
            if let Ok(raw) = serde_json::to_string(&lobby) {
                let _: Result<(), _> = conn
                    .set_ex(keys::lobby(&lobby.id.to_string()), raw, 3600)
                    .await;
            }
        });
    }

    pub async fn list_open_lobbies(&self) -> Vec<LobbyView> {
        self.inner
            .lobbies
            .iter()
            .filter_map(|entry| {
                let lobby = entry.value().lock();
                if lobby.phase == "open" {
                    Some(lobby_view(&lobby))
                } else {
                    None
                }
            })
            .collect()
    }
}

fn lobby_view(lobby: &Lobby) -> LobbyView {
    LobbyView {
        id: lobby.id,
        host_id: lobby.host_id,
        max_players: lobby.max_players,
        map_size: lobby.map_size,
        ffa: lobby.ffa,
        phase: lobby.phase.clone(),
        slots: lobby
            .slots
            .iter()
            .map(|s| LobbySlot {
                user_id: s.user_id,
                name: s.name.clone(),
                faction: s.faction.clone(),
                ready: s.ready,
                team: s.team,
                flag: s.flag.clone(),
            })
            .collect(),
    }
}
