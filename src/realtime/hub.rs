use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use redis::AsyncCommands;
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;

use super::match_sim::{self, MatchSim, MAX_PLAYERS, map_size_for_players};
use super::protocol::{
    ClientMsg, OpenMatchView, ServerMsg, default_catalog, normalize_faction,
};
use crate::store::{Redis, keys, users};
use crate::error::AppError;

pub type Outbox = mpsc::UnboundedSender<ServerMsg>;

#[derive(Clone)]
pub struct MatchHub {
    inner: Arc<HubInner>,
}

struct ConnSlot {
    generation: u64,
    tx: Outbox,
}

struct HubInner {
    redis: Redis,
    matches: DashMap<Uuid, Arc<RwLock<MatchRuntime>>>,
    /// user_id -> live websocket outbox (generation guards refresh races)
    connections: DashMap<Uuid, ConnSlot>,
    conn_epoch: AtomicU64,
    /// user_id -> lobby_id (legacy, unused in drop-in flow)
    user_lobby: DashMap<Uuid, Uuid>,
    /// user_id -> match_id (kept across disconnect for resume)
    user_match: DashMap<Uuid, Uuid>,
}

struct MatchRuntime {
    sim: MatchSim,
    /// subscribers in this match
    members: HashMap<Uuid, ()>,
    max_players: u8,
    /// Still accepting drop-in joiners.
    open: bool,
}

impl MatchHub {
    pub fn new(redis: Redis) -> Self {
        Self {
            inner: Arc::new(HubInner {
                redis,
                matches: DashMap::new(),
                connections: DashMap::new(),
                conn_epoch: AtomicU64::new(1),
                user_lobby: DashMap::new(),
                user_match: DashMap::new(),
            }),
        }
    }

    /// Returns a connection generation used to ignore stale unregister on refresh.
    pub fn register(&self, user_id: Uuid, tx: Outbox) -> u64 {
        let generation = self.inner.conn_epoch.fetch_add(1, Ordering::Relaxed);
        self.inner
            .connections
            .insert(user_id, ConnSlot { generation, tx });
        generation
    }

    pub fn unregister(&self, user_id: Uuid, generation: u64) {
        let removed = self
            .inner
            .connections
            .remove_if(&user_id, |_, slot| slot.generation == generation);
        if removed.is_none() {
            // A newer socket already owns this user — do not mark disconnected.
            return;
        }
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

    /// If the user still belongs to a live match, re-send MatchStart snapshot.
    pub async fn resume_match(&self, user_id: Uuid) -> bool {
        let Some(match_id) = self.inner.user_match.get(&user_id).map(|e| *e) else {
            return false;
        };
        let Some(runtime) = self.inner.matches.get(&match_id).map(|e| e.clone()) else {
            self.inner.user_match.remove(&user_id);
            return false;
        };

        let snapshot = {
            let mut rt = runtime.write().await;
            if rt.sim.ended {
                drop(rt);
                self.inner.user_match.remove(&user_id);
                return false;
            }
            if !rt.sim.players.contains_key(&user_id) {
                drop(rt);
                self.inner.user_match.remove(&user_id);
                return false;
            }
            if let Some(player) = rt.sim.players.get_mut(&user_id) {
                player.connected = true;
            }
            rt.sim.force_aoi_resync(user_id);
            rt.members.insert(user_id, ());
            rt.sim.snapshot_for(user_id)
        };

        if let Some(snapshot) = snapshot {
            self.send(
                user_id,
                ServerMsg::MatchStart {
                    match_id,
                    snapshot,
                },
            );
            true
        } else {
            false
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
                self.resume_match(user_id).await;
            }
            ClientMsg::Ping { n } => self.send(user_id, ServerMsg::Pong { n }),
            ClientMsg::CreateLobby {
                max_players,
                map_size,
                ffa,
                faction,
            } => {
                if self.inner.user_match.contains_key(&user_id) {
                    if self.resume_match(user_id).await {
                        return Ok(());
                    }
                }
                let faction = normalize_faction(&faction);
                let _ = self.set_faction(user_id, &faction).await;
                self.create_and_enter_match(user_id, max_players, map_size, ffa, faction)
                    .await?;
            }
            ClientMsg::JoinLobby { lobby_id, faction } => {
                // lobby_id is the live match id (drop-in join).
                if let Some(existing) = self.inner.user_match.get(&user_id).map(|e| *e) {
                    if existing == lobby_id {
                        let _ = self.resume_match(user_id).await;
                        return Ok(());
                    }
                    if self.resume_match(user_id).await {
                        return Err("Already in another match".into());
                    }
                }
                let faction = normalize_faction(&faction);
                let _ = self.set_faction(user_id, &faction).await;
                self.join_match(user_id, lobby_id, faction).await?;
            }
            ClientMsg::LeaveLobby => self.leave_lobby(user_id).await?,
            ClientMsg::SetFaction { faction } => {
                self.set_faction(user_id, &faction).await?;
            }
            ClientMsg::Ready { .. } | ClientMsg::StartMatch => {
                // Lobby wait removed — create/join enter the match immediately.
            }
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
            ClientMsg::ToggleDebugVision => {
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
                let (tick, entities, removed, died, resources, explored_new, shots, global_vision) = {
                    let mut rt = runtime.write().await;
                    let on = rt.sim.toggle_debug_vision(user_id);
                    let tick = rt.sim.tick;
                    let (entities, removed, died, resources, explored_new, shots) =
                        rt.sim.delta_for(user_id);
                    (tick, entities, removed, died, resources, explored_new, shots, on)
                };
                self.send(
                    user_id,
                    ServerMsg::Delta {
                        tick,
                        entities,
                        removed,
                        died,
                        resources,
                        focus_hint: None,
                        explored_new,
                        shots,
                        scoreboard: None,
                        global_vision,
                    },
                );
            }
            ClientMsg::AllyChat { text } => {
                self.broadcast_ally_chat(user_id, text).await?;
            }
        }
        Ok(())
    }

    async fn create_and_enter_match(
        &self,
        user_id: Uuid,
        max_players: u8,
        _map_size: u16,
        ffa: bool,
        faction: String,
    ) -> Result<(), String> {
        if self.inner.user_lobby.contains_key(&user_id)
            || self.inner.user_match.contains_key(&user_id)
        {
            return Err("Already in a match".into());
        }

        let max_players = max_players.clamp(2, MAX_PLAYERS);
        // Area fully automatic from commander count — roomy between bases.
        let map_size = map_size_for_players(max_players);
        let user = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "User missing".to_string())?;

        let faction = normalize_faction(&faction);
        let match_id = Uuid::new_v4();

        let roster = vec![(
            user_id,
            user.display_name.clone(),
            faction,
            0u8,
            user.equipped_flag.clone(),
        )];

        let sim = MatchSim::new(match_id, map_size, ffa, roster, max_players);
        let runtime = Arc::new(RwLock::new(MatchRuntime {
            sim,
            members: HashMap::from([(user_id, ())]),
            max_players,
            open: true,
        }));

        {
            let mut conn = self.inner.redis.clone();
            let meta = serde_json::json!({
                "id": match_id,
                "map_size": map_size,
                "ffa": ffa,
                "max_players": max_players,
                "open": true,
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
        self.inner.user_match.insert(user_id, match_id);

        {
            let rt = runtime.read().await;
            if let Some(snapshot) = rt.sim.snapshot_for(user_id) {
                self.send(
                    user_id,
                    ServerMsg::MatchStart {
                        match_id,
                        snapshot,
                    },
                );
            }
        }

        self.spawn_match_loop(match_id, runtime);
        Ok(())
    }

    async fn join_match(
        &self,
        user_id: Uuid,
        match_id: Uuid,
        faction: String,
    ) -> Result<(), String> {
        if self.inner.user_lobby.contains_key(&user_id)
            || self.inner.user_match.contains_key(&user_id)
        {
            return Err("Already in a match".into());
        }

        let user = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "User missing".to_string())?;

        let Some(runtime) = self.inner.matches.get(&match_id).map(|e| e.clone()) else {
            return Err("Match not found or already ended".into());
        };

        {
            let mut rt = runtime.write().await;
            if rt.sim.ended || !rt.open {
                return Err("Match is closed".into());
            }
            if rt.sim.human_count() as u8 >= rt.max_players {
                return Err("Match is full".into());
            }

            let faction = normalize_faction(&faction);
            rt.sim
                .add_player(
                    user_id,
                    user.display_name.clone(),
                    faction,
                    user.equipped_flag.clone(),
                )
                .map_err(|e| e.to_string())?;
            rt.members.insert(user_id, ());
        }

        self.inner.user_match.insert(user_id, match_id);

        {
            let rt = runtime.read().await;
            if let Some(snapshot) = rt.sim.snapshot_for(user_id) {
                self.send(
                    user_id,
                    ServerMsg::MatchStart {
                        match_id,
                        snapshot,
                    },
                );
            }
        }

        Ok(())
    }

    async fn leave_lobby(&self, user_id: Uuid) -> Result<(), String> {
        // Drop-in model: leaving a waiting lobby is a no-op; disconnect handles match.
        self.inner.user_lobby.remove(&user_id);
        self.send(user_id, ServerMsg::LobbyLeft);
        Ok(())
    }

    async fn set_faction(&self, user_id: Uuid, faction: &str) -> Result<(), String> {
        let faction = normalize_faction(faction);

        if let Some(mut user) = users::load_user(&self.inner.redis, user_id)
            .await
            .map_err(|e| e.to_string())?
        {
            user.faction = Some(faction);
            users::save_user(&self.inner.redis, &user)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn spawn_match_loop(&self, match_id: Uuid, runtime: Arc<RwLock<MatchRuntime>>) {
        let hub = self.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_millis(1000 / match_sim::TICK_HZ as u64));
            // Do not catch up missed ticks in a burst — that dumps a second of
            // movement in one frame so the whole army "releases" at once.
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let mut outgoing: Vec<(Uuid, ServerMsg)> = Vec::new();
                let ended = {
                    let mut rt = runtime.write().await;
                    rt.sim.tick_once();

                    while let Some(front) = rt.sim.stream_jobs.front() {
                        if front.due_tick > rt.sim.tick {
                            break;
                        }
                        rt.sim.stream_jobs.pop_front();
                    }

                    if rt.sim.tick % match_sim::BROADCAST_EVERY as u64 == 0 {
                        let member_ids: Vec<Uuid> = rt.members.keys().copied().collect();
                        let tick = rt.sim.tick;
                        outgoing.reserve(member_ids.len());
                        for uid in member_ids {
                            if rt.sim.players.get(&uid).is_some_and(|p| !p.connected) {
                                // Offline commanders: no WS payload, no vision stamp.
                                continue;
                            }
                            let (entities, removed, died, resources, explored_new, shots) =
                                rt.sim.delta_for(uid);
                            // Tab scoreboard — FOW-independent roster; ~1 Hz is enough & cheap.
                            let scoreboard = if tick % (match_sim::TICK_HZ as u64) == 0 {
                                Some(rt.sim.scoreboard_for(uid))
                            } else {
                                None
                            };
                            let global_vision = rt.sim.player_has_global_vision(uid);
                            // Skip empty heartbeats when nothing in this FOW window changed.
                            if entities.is_empty()
                                && removed.is_empty()
                                && died.is_empty()
                                && explored_new.is_empty()
                                && shots.is_empty()
                                && scoreboard.is_none()
                                && !global_vision
                            {
                                // Still push a light tick so resources/UI stay alive ~2 Hz.
                                if tick % 10 != 0 {
                                    continue;
                                }
                            }
                            outgoing.push((
                                uid,
                                ServerMsg::Delta {
                                    tick,
                                    entities,
                                    removed,
                                    died,
                                    resources,
                                    focus_hint: None,
                                    explored_new,
                                    shots,
                                    scoreboard,
                                    global_vision,
                                },
                            ));
                        }
                        rt.sim.clear_frame_flags();
                    }

                    if rt.sim.ended {
                        rt.open = false;
                    }
                    rt.sim.ended
                };

                for (uid, msg) in outgoing {
                    hub.send(uid, msg);
                }

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
                        let is_bot = {
                            let rt = runtime.read().await;
                            rt.sim.players.get(&uid).is_some_and(|p| p.is_bot())
                        };
                        if is_bot {
                            continue;
                        }
                        let (won, you_stats, roster) = {
                            let rt = runtime.read().await;
                            let won = rt
                                .sim
                                .players
                                .get(&uid)
                                .map(|p| Some(p.team) == winner)
                                .unwrap_or(false);
                            let roster = rt.sim.match_end_roster(uid);
                            let you_stats = roster.iter().find(|r| r.you).cloned();
                            (won, you_stats, roster)
                        };
                        let xp = hub.award_xp(uid, won).await;
                        hub.send(
                            uid,
                            ServerMsg::MatchEnd {
                                match_id,
                                winner_team: winner,
                                reason: reason.clone(),
                                xp_gained: xp,
                                you_won: won,
                                you: you_stats,
                                roster,
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
        if let Some(slot) = self.inner.connections.get(&user_id) {
            let _ = slot.tx.send(msg);
        }
    }

    /// Match chat: Ally → team; Alone → all, or `@name` whisper.
    async fn broadcast_ally_chat(&self, user_id: Uuid, raw: String) -> Result<(), String> {
        let cleaned = sanitize_ally_chat(&raw).ok_or("Empty chat")?;
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
        let (recipients, payload) = {
            let rt = runtime.read().await;
            let player = rt
                .sim
                .players
                .get(&user_id)
                .ok_or("Not in match")?;
            let team = player.team;
            let name = player.label();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            if rt.sim.ffa {
                if let Some((target_query, body)) = parse_whisper_target(&cleaned) {
                    if body.is_empty() {
                        return Err("PM: @isim sonrası mesaj yaz".into());
                    }
                    let target = resolve_chat_target(&rt.sim, user_id, &target_query)?;
                    let to_name = target.label();
                    let to_id = target.user_id;
                    let connected = target.connected;
                    let msg = ServerMsg::AllyChat {
                        from: user_id,
                        name,
                        team,
                        text: body,
                        ts,
                        whisper: true,
                        to: Some(to_id),
                        to_name: Some(to_name),
                    };
                    let mut recipients = vec![user_id];
                    if to_id != user_id && connected {
                        recipients.push(to_id);
                    }
                    (recipients, msg)
                } else {
                    // General Alone chat — everyone in the match.
                    let msg = ServerMsg::AllyChat {
                        from: user_id,
                        name,
                        team,
                        text: cleaned,
                        ts,
                        whisper: false,
                        to: None,
                        to_name: None,
                    };
                    let recipients: Vec<Uuid> = rt
                        .members
                        .keys()
                        .copied()
                        .filter(|uid| {
                            rt.sim
                                .players
                                .get(uid)
                                .map(|p| p.connected)
                                .unwrap_or(false)
                        })
                        .collect();
                    (recipients, msg)
                }
            } else {
                let msg = ServerMsg::AllyChat {
                    from: user_id,
                    name,
                    team,
                    text: cleaned,
                    ts,
                    whisper: false,
                    to: None,
                    to_name: None,
                };
                let mut recipients = Vec::new();
                for uid in rt.members.keys().copied() {
                    if let Some(p) = rt.sim.players.get(&uid) {
                        if p.team == team && p.connected {
                            recipients.push(uid);
                        }
                    }
                }
                (recipients, msg)
            }
        };
        for uid in recipients {
            self.send(uid, payload.clone());
        }
        Ok(())
    }

    pub async fn list_open_matches(&self) -> Vec<OpenMatchView> {
        let mut out = Vec::new();
        for entry in self.inner.matches.iter() {
            let rt = entry.value().read().await;
            if !rt.open || rt.sim.ended {
                continue;
            }
            out.push(OpenMatchView {
                id: *entry.key(),
                players: rt.sim.player_count() as u8,
                max_players: rt.max_players,
                map_size: rt.sim.map_size,
                ffa: rt.sim.ffa,
            });
        }
        out
    }
}

fn sanitize_ally_chat(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    const MAX: usize = 180;
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        if i >= MAX {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `@Name rest of message` → (name, body). Name may include spaces if quoted later; for now one token.
fn parse_whisper_target(raw: &str) -> Option<(String, String)> {
    let s = raw.trim();
    if !s.starts_with('@') {
        return None;
    }
    let rest = s[1..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let target = parts.next()?.trim().trim_matches(|c| c == ':' || c == ',');
    if target.is_empty() {
        return None;
    }
    let body = parts.next().unwrap_or("").trim().to_string();
    Some((target.to_string(), body))
}

fn resolve_chat_target<'a>(
    sim: &'a match_sim::MatchSim,
    from: Uuid,
    query: &str,
) -> Result<&'a match_sim::PlayerState, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Err("Alone: @isim gerekli".into());
    }
    let mut exact: Vec<&match_sim::PlayerState> = Vec::new();
    let mut prefix: Vec<&match_sim::PlayerState> = Vec::new();
    for p in sim.players.values() {
        if p.user_id == from {
            continue;
        }
        let bare = p.name.to_lowercase();
        let label = p.label().to_lowercase();
        if bare == q || label == q {
            exact.push(p);
        } else if bare.starts_with(&q) || label.starts_with(&q) {
            prefix.push(p);
        }
    }
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    if exact.len() > 1 {
        return Err("Alone: isim belirsiz — daha uzun yaz".into());
    }
    if prefix.len() == 1 {
        return Ok(prefix[0]);
    }
    if prefix.len() > 1 {
        return Err("Alone: birden fazla eşleşme — tam isim kullan".into());
    }
    Err(format!("Alone: @{query} bulunamadı"))
}
