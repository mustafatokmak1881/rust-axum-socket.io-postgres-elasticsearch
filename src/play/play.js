import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

const $ = (selector) => document.querySelector(selector);

const state = {
  ws: null,
  user: null,
  entitlements: [],
  catalog: [],
  lobby: null,
  match: null,
  entities: new Map(),
  meshes: new Map(),
  selectedBuild: null,
  selectedUnits: [],
  selectedBuilding: null,
  buildable: [],
  trainable: [],
  faction: "usa",
  ready: false,
  reconnectAttempt: 0,
  matchEnded: false,
  scoreboard: [],
};

const factionColors = {
  usa: 0x3a6a2a,
  china: 0x8a3030,
  gla: 0xa09040,
  default: 0x556644,
};

function toast(message, ms = 2800) {
  const el = $("#toast");
  el.textContent = message;
  el.hidden = false;
  clearTimeout(toast._t);
  toast._t = setTimeout(() => {
    el.hidden = true;
  }, ms);
}

function send(msg) {
  if (state.ws?.readyState === WebSocket.OPEN) {
    state.ws.send(JSON.stringify(msg));
  }
}

function currentFullscreenElement() {
  return document.fullscreenElement || document.webkitFullscreenElement || null;
}

async function enterGameFullscreen() {
  if (currentFullscreenElement()) return;
  const el = document.documentElement;
  try {
    if (el.requestFullscreen) {
      await el.requestFullscreen();
    } else if (el.webkitRequestFullscreen) {
      el.webkitRequestFullscreen();
    }
  } catch {
    // Ignored: must be called from a click/tap. Lobby buttons already do that.
  }
}

async function exitGameFullscreen() {
  if (!currentFullscreenElement()) return;
  try {
    if (document.exitFullscreen) {
      await document.exitFullscreen();
    } else if (document.webkitExitFullscreen) {
      document.webkitExitFullscreen();
    }
  } catch {
    // ignore
  }
}

function connect() {
  if (state.ws && (state.ws.readyState === WebSocket.OPEN || state.ws.readyState === WebSocket.CONNECTING)) {
    return;
  }

  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  state.ws = ws;

  ws.addEventListener("open", () => {
    state.reconnectAttempt = 0;
    send({ t: "hello" });
  });
  ws.addEventListener("close", () => {
    if (state.matchEnded) return;
    const attempt = state.reconnectAttempt++;
    const delay = Math.min(8000, 700 * 2 ** Math.min(attempt, 4));
    toast(state.match ? "Bağlantı koptu — maça geri bağlanılıyor…" : "Yeniden bağlanılıyor…");
    setTimeout(connect, delay);
  });
  ws.addEventListener("message", (event) => {
    let msg;
    try {
      msg = JSON.parse(event.data);
    } catch {
      return;
    }
    onServer(msg);
  });
}

function onServer(msg) {
  switch (msg.t) {
    case "welcome":
      state.user = msg.user;
      state.entitlements = msg.entitlements || [];
      state.catalog = msg.catalog || [];
      $("#user-name").textContent = msg.user.display_name || msg.user.email;
      $("#user-xp").textContent = `${msg.user.xp || 0} XP`;
      if (msg.user.faction) {
        state.faction = msg.user.faction;
        syncFactionButtons();
      }
      renderStore();
      break;
    case "lobby_update":
      // Waiting lobby removed — ignore.
      break;
    case "lobby_left":
      break;
    case "open_matches":
      renderOpenMatches(msg.matches || []);
      break;
    case "match_start": {
      const wasInMatch = Boolean(state.match);
      const midGame = (msg.snapshot?.tick || 0) > 0;
      enterMatch(msg.snapshot);
      if (wasInMatch || midGame) {
        toast("Maça devam — kaldığın yerden");
      } else {
        toast("Match live — move mouse to screen edges to pan");
      }
      break;
    }
    case "delta":
      queueDelta(msg);
      break;
    case "match_end":
      state.matchEnded = true;
      toast(`Match over: ${msg.reason} · +${msg.xp_gained} XP`);
      setTimeout(() => location.reload(), 4000);
      break;
    case "store_ok":
      state.entitlements = msg.entitlements || [];
      renderStore();
      toast("Entitlement updated");
      break;
    case "error":
      toast(msg.message);
      break;
    case "pong":
      break;
    default:
      break;
  }
}

function syncFactionButtons() {
  document.querySelectorAll(".faction").forEach((btn) => {
    btn.classList.toggle("on", btn.dataset.faction === state.faction);
  });
}

function renderLobby() {}

function renderOpenMatches(matches) {
  const root = $("#open-lobbies");
  if (!root) return;
  if (!matches.length) {
    root.innerHTML = "<p class='muted'>No open matches — Start Match to host one.</p>";
    return;
  }
  root.innerHTML = matches
    .map(
      (m) => `
      <button type="button" class="store-item" data-join="${escapeHtml(m.id)}">
        <strong>${m.players}/${m.max_players} live</strong>
        <small>${escapeHtml(m.id)}</small>
        <span>Map ${m.map_size}${m.ffa ? " · FFA" : " · Allied"} · click to join</span>
      </button>`,
    )
    .join("");
}

function joinMatchId(raw) {
  const lobby_id = String(raw || "").trim();
  if (!lobby_id) {
    toast("Enter a match UUID or pick one from the list");
    return;
  }
  // UUID shape check (lenient)
  if (!/^[0-9a-fA-F-]{36}$/.test(lobby_id)) {
    toast("Invalid match UUID");
    return;
  }
  $("#lobby-id").value = lobby_id;
  send({ t: "join_lobby", lobby_id, faction: state.faction || "usa" });
  toast("Joining match…");
}

function renderStore() {
  $("#store-items").innerHTML = (state.catalog || [])
    .map((item) => {
      const owned = state.entitlements.includes(item.id);
      return `
        <button type="button" class="store-item" data-item="${escapeHtml(item.id)}">
          <strong>${escapeHtml(item.name)}</strong>
          <small>${escapeHtml(item.description)}</small>
          <span>${owned ? "Owned — Equip" : escapeHtml(item.price_label)}</span>
        </button>`;
    })
    .join("");
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );
}

function hexColor(n) {
  return `#${(Number(n) >>> 0).toString(16).padStart(6, "0")}`;
}

function renderScoreboard() {
  const body = $("#scoreboard-body");
  if (!body) return;
  const rows = state.scoreboard || [];
  if (!rows.length) {
    body.innerHTML = `<tr><td colspan="10" class="muted">Veri yok</td></tr>`;
    return;
  }
  body.innerHTML = rows
    .map((r) => {
      const [c0, c1, c2] = r.colors || [0x888888, 0x555555, 0x333333];
      const ally =
        !state.match?.ffa &&
        !r.you &&
        Number(r.team) === Number(state.match?.team);
      const cls = [
        r.you ? "you" : "",
        ally ? "ally" : "",
        r.alive ? "alive" : "dead",
        "jumpable",
      ]
        .filter(Boolean)
        .join(" ");
      const faction = String(r.faction || "").toUpperCase();
      const tag = r.bot ? "BOT" : r.you ? "SEN" : ally ? "DOST" : "OYUNCU";
      const teamLabel = state.match?.ffa
        ? "—"
        : `T${Number(r.team) + 1}`;
      const status = r.alive
        ? `<span class="status on">ACTIVE</span>`
        : `<span class="status off">DEAD</span>`;
      const hqX = r.hq_x != null ? Number(r.hq_x) : "";
      const hqY = r.hq_y != null ? Number(r.hq_y) : "";
      return `<tr class="${cls}" data-owner="${escapeHtml(r.id || "")}" data-hq-x="${hqX}" data-hq-y="${hqY}" title="Command Center'a git">
        <td><span class="swatch"><i style="background:${hexColor(c0)}"></i><i style="background:${hexColor(c1)}"></i><i style="background:${hexColor(c2)}"></i></span></td>
        <td><div class="who"><strong>${escapeHtml(r.name || "—")}</strong><small>${escapeHtml(faction)} · ${tag} · ${teamLabel}</small></div></td>
        <td>${status}</td>
        <td>${r.infantry ?? 0}</td>
        <td>${r.tanks ?? 0}</td>
        <td>${r.buildings ?? 0}</td>
        <td>${r.supplies ?? 0}</td>
        <td>${r.fuel ?? 0}</td>
        <td>${r.munitions ?? 0}</td>
        <td>${r.power_used ?? 0}/${r.power ?? 0}</td>
      </tr>`;
    })
    .join("");
}

function centerCameraOnOwner(ownerId, hqX, hqY) {
  if (!controls || !camera || !state.match) return;
  let x = Number(hqX);
  let y = Number(hqY);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    let hq = null;
    for (const entity of state.entities.values()) {
      if (entity.owner === ownerId && entity.kind === "hq") {
        hq = entity;
        break;
      }
    }
    if (!hq) {
      toast("Command Center görünmüyor / yok");
      return;
    }
    x = hq.x;
    y = hq.y;
  }
  panCameraTo(x, y);
  const row = (state.scoreboard || []).find((r) => r.id === ownerId);
  const label = row
    ? `${String(row.faction || "").toUpperCase()} · ${row.name || "HQ"}`
    : "Command Center";
  toast(label);
}

function centerCameraOnHq() {
  if (!state.match) return;
  const self = (state.scoreboard || []).find((r) => r.you);
  if (self) {
    centerCameraOnOwner(self.id, self.hq_x, self.hq_y);
    return;
  }
  centerCameraOnOwner(state.match.you, null, null);
}

function setScoreboardOpen(open) {
  const el = $("#scoreboard");
  if (!el || $("#match-screen")?.hidden) return;
  el.hidden = !open;
  if (open) renderScoreboard();
}

function clearWorldMeshes() {
  if (!state.meshes.size) {
    for (const id of [...(Sfx.engines?.keys?.() || [])]) Sfx.stopEngine(id);
    return;
  }
  for (const [id, mesh] of state.meshes.entries()) {
    Sfx.stopEngine(id);
    if (scene) scene.remove(mesh);
    mesh.traverse?.((obj) => {
      if (obj.material) {
        if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose?.());
        else obj.material.dispose?.();
      }
    });
  }
  state.meshes.clear();
}

function enterMatch(snapshot) {
  state.match = snapshot;
  state.scoreboard = snapshot.scoreboard || [];
  if (snapshot.you_faction) {
    state.faction = snapshot.you_faction;
    syncFactionButtons();
  }
  state.entities.clear();
  clearWorldMeshes();
  aoiRadius = Number(snapshot.aoi_radius) || 28;
  globalVision = snapshot.global_vision === true;
  for (const entity of snapshot.entities || []) {
    state.entities.set(entity.id, entity);
  }
  $("#lobby-screen").hidden = true;
  $("#match-screen").hidden = false;
  const fac = String(snapshot.you_faction || state.faction || "usa").toUpperCase();
  if (snapshot.ffa) {
    toast(`${fac} · FFA — everyone is hostile`);
  } else {
    toast(`${fac} · Team ${Number(snapshot.team) + 1} (shared vision)`);
  }
  // Fullscreen only from click handlers (create/join/pointer) — browsers block gesture-less FS.
  void setupMatchScene(snapshot);
}

function hashMatchSeed(id) {
  const s = String(id || "seed");
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) || 1;
}

async function setupMatchScene(snapshot) {
  updateResources(snapshot.resources);
  updateArmyCounts();
  renderBuildList(snapshot.buildable || []);
  renderUnitList(snapshot.trainable || []);
  await ensureBuildingModel();
  const terrainSeed = hashMatchSeed(snapshot.match_id || snapshot.tick || 1);
  const terrain = await loadTerrainTexture(snapshot.map_size, terrainSeed);
  const home = findOwnHome(snapshot);
  if (snapshot.focus) {
    home.x = snapshot.focus[0];
    home.z = snapshot.focus[1];
  }
  aoiRadius = Number(snapshot.aoi_radius) || aoiRadius;
  lastFocusSent = { x: home.x, z: home.z };
  initThree(snapshot.map_size, terrain, home);
  Radar.bind();
  loadExploredFromSnapshot(snapshot);
  rebuildMeshes();
  refreshLiveVision();
  startVisionLoop();
}

function findOwnHome(snapshot) {
  const you = snapshot.you;
  const hq = (snapshot.entities || []).find(
    (e) => e.owner === you && e.kind === "hq",
  );
  if (hq) return { x: hq.x, z: hq.y };
  const any = (snapshot.entities || []).find((e) => e.owner === you);
  if (any) return { x: any.x, z: any.y };
  return { x: snapshot.map_size / 2, z: snapshot.map_size / 2 };
}

function panCameraTo(lookX, lookZ, quiet) {
  if (!controls || !camera) return;
  const dist = controls.getDistance?.() || CAMERA_DIST;
  const margin = 4;
  lookX = Math.max(margin, Math.min(mapSize - margin, lookX));
  lookZ = Math.max(margin, Math.min(mapSize - margin, lookZ));
  controls.target.set(lookX, 0, lookZ);
  camera.position.set(
    lookX,
    Math.sin(CAMERA_PITCH) * dist,
    lookZ + Math.cos(CAMERA_PITCH) * dist,
  );
  controls.update();
  lastFocusSent = { x: lookX, z: lookZ };
  if (quiet) return;
  send({ t: "set_focus", x: lookX, y: lookZ });
}

function updateResources(res) {
  if (!res) return;
  $("#res-supplies").textContent = res.supplies;
  $("#res-fuel").textContent = res.fuel;
  $("#res-munitions").textContent = res.munitions;
  $("#res-power").textContent = `${res.power_used}/${res.power}`;
}

function updateArmyCounts() {
  const you = state.match?.you;
  const infEl = $("#army-inf");
  const tankEl = $("#army-tank");
  const totalEl = $("#army-total");
  if (!infEl || !tankEl) return;
  let inf = 0;
  let tanks = 0;
  if (you) {
    for (const entity of state.entities.values()) {
      if (!entity.unit || entity.owner !== you) continue;
      if (
        String(entity.kind || "").includes("tank") ||
        String(entity.kind || "").includes("mlrs")
      )
        tanks += 1;
      else inf += 1;
    }
  }
  infEl.textContent = inf;
  tankEl.textContent = tanks;
  if (totalEl) totalEl.textContent = inf + tanks;
}

function renderBuildList(items) {
  state.buildable = items || [];
  $("#build-list").innerHTML = state.buildable
    .map(
      (item, index) => `
      <button type="button" class="build-item" data-kind="${escapeHtml(item.kind)}" data-hotkey="${index + 1}">
        <strong><span class="hotkey">${index + 1}</span> ${escapeHtml(item.name)}</strong>
        <small>${item.cost_supplies}/${item.cost_fuel}/${item.cost_munitions} · ${Math.round(item.build_ms / 1000)}s</small>
      </button>`,
    )
    .join("");
}

const BUILDING_LABELS = {
  barracks: "Barracks",
  war_factory: "War Factory",
  arms_dealer: "Arms Dealer",
  airfield: "Airfield",
  palace: "Palace",
  supply: "Supply Center",
  supply_stash: "Supply Stash",
};

function renderUnitList(items) {
  state.trainable = items || [];
  refreshTrainablePanel();
}

function refreshTrainablePanel() {
  const items = state.trainable || [];
  const selected = state.selectedBuilding
    ? state.entities.get(state.selectedBuilding)
    : null;
  const fromKind = selected?.building ? String(selected.kind || "") : "";
  const filtered = fromKind
    ? items.filter((item) => item.from_building === fromKind)
    : items;
  const hint = fromKind
    ? BUILDING_LABELS[fromKind] || fromKind
    : "select a production building";
  $("#unit-list").innerHTML =
    `<div class="unit-hint" style="opacity:.7;font-size:12px;margin:0 0 6px">${escapeHtml(
      String(state.faction || "usa").toUpperCase(),
    )} · ${escapeHtml(hint)}</div>` +
    (filtered.length
      ? filtered
          .map(
            (item) => `
      <button type="button" class="unit-item" data-unit="${escapeHtml(item.unit)}" data-from="${escapeHtml(item.from_building)}">
        <strong>${escapeHtml(item.name)}</strong>
        <small>${item.cost_supplies}s/${item.cost_fuel}f/${item.cost_munitions || 0}m · ${Math.round((item.train_ms || 0) / 1000)}s</small>
      </button>`,
          )
          .join("")
      : `<small style="opacity:.65">${
          fromKind
            ? "No units from this building"
            : "Click barracks / factory / arms dealer to train"
        }</small>`);
}

function queueDelta(msg) {
  if (!queueDelta.pending) {
    queueDelta.pending = msg;
  } else {
    mergeDelta(queueDelta.pending, msg);
  }
  if (queueDelta.raf) return;
  queueDelta.raf = requestAnimationFrame(() => {
    queueDelta.raf = 0;
    const next = queueDelta.pending;
    queueDelta.pending = null;
    if (next) applyDelta(next);
  });
}

function mergeDelta(into, extra) {
  into.tick = extra.tick;
  if (extra.resources) into.resources = extra.resources;
  if (extra.focus_hint) into.focus_hint = extra.focus_hint;
  const latest = new Map();
  for (const entity of into.entities || []) latest.set(entity.id, entity);
  for (const entity of extra.entities || []) latest.set(entity.id, entity);
  into.entities = [...latest.values()];
  const removed = new Set(into.removed || []);
  for (const id of extra.removed || []) removed.add(id);
  into.removed = [...removed];
  into.shots = [...(into.shots || []), ...(extra.shots || [])];
  if (extra.scoreboard) into.scoreboard = extra.scoreboard;
  if (typeof extra.global_vision === "boolean") into.global_vision = extra.global_vision;
}

function applyDelta(msg) {
  updateResources(msg.resources);
  if (typeof msg.global_vision === "boolean") {
    const wasOpen = globalVision;
    globalVision = msg.global_vision;
    if (wasOpen !== globalVision) {
      lastVisionAt = 0;
      refreshLiveVision();
      if (globalVision) toast("Dev map: full vision ON (M)");
      else toast("Dev map: fog restored (M)");
    }
  }
  if (msg.scoreboard) {
    state.scoreboard = msg.scoreboard;
    if (!$("#scoreboard")?.hidden) renderScoreboard();
  }
  applyExploredNew(msg.explored_new);

  const tankHits = (msg.shots || []).filter(
    (s) => s.hit !== false && String(s.kind || "").includes("tank") && s.x1 != null && s.y1 != null,
  );
  for (const id of msg.removed || []) {
    const prev = state.entities.get(id);
    state.entities.delete(id);
    const mesh = state.meshes.get(id);
    const kind = String(prev?.kind || mesh?.userData?.kind || "");
    const nearHe =
      mesh?.userData?.isInfantry &&
      tankHits.some((s) => Math.hypot(mesh.position.x - s.x1, mesh.position.z - s.y1) <= 1.35);
    if (nearHe) {
      mesh.userData.corpse = true;
      mesh.userData.corpseAt = performance.now();
      mesh.userData.moving = false;
      continue;
    }
    if (mesh && (mesh.userData.isTank || kind.includes("tank") || kind.includes("mlrs"))) {
      beginTankWreck(mesh, prev?.x ?? mesh.position.x, prev?.y ?? mesh.position.z);
      Sfx.stopBuild(id);
      Sfx.stopEngine(id);
      continue;
    }
    if (mesh && (prev?.building || mesh.userData.building)) {
      beginBuildingWreck(
        mesh,
        prev?.x ?? mesh.position.x,
        prev?.y ?? mesh.position.z,
        kind,
      );
      Sfx.stopBuild(id);
      continue;
    }
    reapUnitMesh(mesh);
    Sfx.stopBuild(id);
    Sfx.stopEngine(id);
  }
  // Collapse SFX is triggered inside beginBuildingWreck.

  if (msg.removed?.length) {
    const dead = new Set(msg.removed);
    const before = state.selectedUnits.length;
    state.selectedUnits = state.selectedUnits.filter((id) => !dead.has(id));
    if (state.selectedUnits.length !== before) syncSelectionMarkers();
    if (dead.has(state.selectedBuilding)) {
      state.selectedBuilding = null;
      refreshTrainablePanel();
    }
  }
  for (const entity of msg.entities || []) {
    const prev = state.entities.get(entity.id);
    state.entities.set(entity.id, entity);
    if (scene) upsertMesh(entity);
    syncBuildingSfx(prev, entity);
  }
  playShots(msg.shots || []);
  updateArmyCounts();
  $("#match-caption").textContent =
    `Tick ${msg.tick} · ${state.entities.size} entities · ${globalVision ? "open map" : "vision fog"}`;
}

function syncBuildingSfx(prev, entity) {
  if (!entity?.building) return;
  const was = prev?.progress != null && prev.progress < 1;
  const now = entity.progress != null && entity.progress < 1;
  if (now) {
    Sfx.startBuild(entity.id, entity.x, entity.y);
  } else if (was && !now) {
    Sfx.stopBuild(entity.id);
    Sfx.buildingComplete(entity.x, entity.y);
  } else if (!now) {
    Sfx.stopBuild(entity.id);
  }
}

/* ---------- Procedural SFX (Web Audio, distance-attenuated) ---------- */

const Sfx = {
  ctx: null,
  master: null,
  compressor: null,
  limiter: null,
  buses: { rifle: null, tank: null, fx: null },
  builds: new Map(),
  engines: new Map(),
  tankMoveBuf: null,
  tankMoveWait: null,
  tankShootBuf: null,
  tankShootWait: null,
  tankDestroyedBuf: null,
  tankDestroyedWait: null,
  mlrsRocketBuf: null,
  mlrsRocketWait: null,
  patriotBuf: null,
  patriotWait: null,
  soldierShootBuf: null,
  soldierShootWait: null,
  buildingBuf: null,
  buildingWait: null,
  lastRifleAt: 0,
  rifleRest: 1.12,
  // World-units: full volume inside ref, silent past max. Camera look-at is listener.
  ranges: {
    rifle: { ref: 3.5, max: 16, exp: 2.6 },
    tank: { ref: 5.5, max: 28, exp: 2.35 },
    missile: { ref: 4.5, max: 20, exp: 2.4 },
    build: { ref: 3.5, max: 14, exp: 2.5 },
    collapse: { ref: 5, max: 22, exp: 2.2 },
    complete: { ref: 3.5, max: 14, exp: 2.5 },
    wreck: { ref: 7, max: 28, exp: 2.0 },
    mlrs: { ref: 6, max: 30, exp: 2.2 },
    patriot: { ref: 6.5, max: 32, exp: 2.15 },
  },

  ensure() {
    if (!this.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) return false;
      this.ctx = new AC();
      this.master = this.ctx.createGain();
      // ~3× previous mix; limiter catches peaks so speakers don't square-wave.
      this.master.gain.value = 3;

      this.compressor = this.ctx.createDynamicsCompressor();
      this.compressor.threshold.value = -18;
      this.compressor.knee.value = 10;
      this.compressor.ratio.value = 3.5;
      this.compressor.attack.value = 0.005;
      this.compressor.release.value = 0.2;

      this.limiter = this.ctx.createDynamicsCompressor();
      this.limiter.threshold.value = -1.8;
      this.limiter.knee.value = 0.5;
      this.limiter.ratio.value = 20;
      this.limiter.attack.value = 0.001;
      this.limiter.release.value = 0.1;

      this.buses.rifle = this.ctx.createGain();
      this.buses.tank = this.ctx.createGain();
      this.buses.fx = this.ctx.createGain();
      this.buses.rifle.gain.value = this.rifleRest;
      this.buses.tank.gain.value = 1.9;
      this.buses.fx.gain.value = 0.62;
      // Real-world mix: tank cannon (dry) > rifles (dry) > FX (compressed) > engine.
      this.buses.fx.connect(this.compressor);
      this.compressor.connect(this.master);
      this.buses.rifle.connect(this.master);
      this.buses.tank.connect(this.master);
      this.master.connect(this.limiter);
      this.limiter.connect(this.ctx.destination);
    }
    if (this.ctx.state === "suspended") void this.ctx.resume();
    void this.loadTankMove();
    void this.loadTankShoot();
    void this.loadTankDestroyed();
    void this.loadMlrsRocket();
    void this.loadPatriot();
    void this.loadSoldierShoot();
    void this.loadBuilding();
    return true;
  },

  dest(name) {
    return this.buses[name] || this.master;
  },

  /** Cannon blast buries small-arms the way a real 120 mm does. */
  duckRifles(seconds = 0.85) {
    const bus = this.buses.rifle;
    if (!bus || !this.ctx) return;
    const t0 = this.ctx.currentTime;
    const g = bus.gain;
    const rest = this.rifleRest;
    try {
      g.cancelScheduledValues(t0);
      g.setValueAtTime(Math.max(0.22, g.value), t0);
      g.linearRampToValueAtTime(0.22, t0 + 0.01);
      g.setValueAtTime(0.22, t0 + seconds * 0.28);
      g.linearRampToValueAtTime(rest, t0 + seconds);
    } catch {
      // ignore
    }
  },

  /** Listener = where the camera is looking on the ground (RTS "ear"). */
  listenerXZ() {
    if (controls?.target) return { x: controls.target.x, z: controls.target.z };
    if (camera) return { x: camera.position.x, z: camera.position.z };
    return { x: 0, z: 0 };
  },

  /** Ground radius the current camera actually sees. */
  viewRadius() {
    const dist = controls?.getDistance?.() || CAMERA_DIST;
    return Math.max(5, dist * 0.95);
  },

  panAt(worldX) {
    const ear = this.listenerXZ();
    const span = this.viewRadius() * 0.85;
    return Math.max(-0.95, Math.min(0.95, (worldX - ear.x) / span));
  },

  /**
   * 0 = mute, 1 = at the look-point. Steep falloff so near vs far is obvious.
   */
  volumeAt(worldX, worldY, range) {
    const spec = range || this.ranges.rifle;
    const ear = this.listenerXZ();
    const zoom = (controls?.getDistance?.() || CAMERA_DIST) / CAMERA_DIST;
    const z = Math.max(0.8, Math.min(1.4, zoom));
    const ref = spec.ref * z;
    const max = spec.max * z;
    const d = Math.hypot(worldX - ear.x, worldY - ear.z);
    if (!(d < max)) return 0;
    if (d <= ref) return 1;
    const t = (d - ref) / (max - ref);
    // Extra square so mid-screen is already clearly quieter than under the cursor.
    return Math.pow(1 - t, spec.exp || 2.4);
  },

  /** Gain used for samples. Lower curve = more presence at mid-distance. */
  sampleGain(vol, peak, curve = 2) {
    return Math.max(0.0001, peak * Math.pow(vol, curve));
  },

  startSpatialSource(src, x, y, vol, dest, peak, curve = 2) {
    const g = this.ctx.createGain();
    g.gain.value = this.sampleGain(vol, peak, curve);
    const pan = this.ctx.createStereoPanner();
    pan.pan.value = this.panAt(x);
    src.connect(g);
    g.connect(pan);
    pan.connect(dest);
    src.start();
  },

  noiseBuffer(seconds = 0.2) {
    const len = Math.max(1, Math.floor(this.ctx.sampleRate * seconds));
    const buf = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
    const data = buf.getChannelData(0);
    for (let i = 0; i < len; i++) data[i] = Math.random() * 2 - 1;
    return buf;
  },

  tone(freq, dur, type = "square", gain = 0.08, freqEnd = null, vol = 1, dest = null) {
    if (!this.ensure() || vol <= 0.004) return;
    const t0 = this.ctx.currentTime;
    const osc = this.ctx.createOscillator();
    const g = this.ctx.createGain();
    const peak = gain * vol;
    osc.type = type;
    osc.frequency.setValueAtTime(freq, t0);
    if (freqEnd != null) osc.frequency.exponentialRampToValueAtTime(Math.max(40, freqEnd), t0 + dur);
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(Math.max(0.0001, peak), t0 + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    osc.connect(g);
    g.connect(dest || this.master);
    osc.start(t0);
    osc.stop(t0 + dur + 0.02);
  },

  /**
   * Filtered noise with optional Q / attack — used for realistic gun layers.
   */
  noiseBurst(dur, gain = 0.12, filterFreq = 2500, filterType = "bandpass", vol = 1, q = 0.7, dest = null) {
    if (!this.ensure() || vol <= 0.004) return;
    const t0 = this.ctx.currentTime;
    const src = this.ctx.createBufferSource();
    src.buffer = this.noiseBuffer(Math.max(dur, 0.05));
    const filt = this.ctx.createBiquadFilter();
    filt.type = filterType;
    filt.frequency.value = filterFreq;
    filt.Q.value = q;
    const g = this.ctx.createGain();
    const peak = gain * vol;
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(Math.max(0.0001, peak), t0 + 0.002);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
    src.connect(filt);
    filt.connect(g);
    g.connect(dest || this.master);
    src.start(t0);
    src.stop(t0 + dur + 0.02);
  },

  rifle(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.rifle);
    if (vol <= 0.008) return;
    const now = performance.now();
    // Distant pops must not steal the slot from a shot under the camera.
    if (vol < 0.35 && now - this.lastRifleAt < 18) return;
    if (vol >= 0.35 || now - this.lastRifleAt >= 18) this.lastRifleAt = now;
    const play = (buf) => {
      if (!buf || !this.ctx) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.playbackRate.value = 0.96 + Math.random() * 0.08;
      this.startSpatialSource(src, x, y, vol, this.dest("rifle"), 2.2, 1.22);
    };
    if (this.soldierShootBuf) play(this.soldierShootBuf);
    else void this.loadSoldierShoot().then(play);
  },

  tankCannon(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.tank);
    if (vol <= 0.006) return;
    this.duckRifles(0.7 + vol * 0.6);
    const play = (buf) => {
      if (!buf || !this.ctx) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      this.startSpatialSource(src, x, y, vol, this.dest("tank"), 2.45, 1.45);
    };
    if (this.tankShootBuf) play(this.tankShootBuf);
    else void this.loadTankShoot().then(play);
  },

  loadTankMove() {
    if (this.tankMoveBuf) return Promise.resolve(this.tankMoveBuf);
    if (this.tankMoveWait) return this.tankMoveWait;
    if (!this.ctx) return Promise.resolve(null);
    this.tankMoveWait = fetch("/assets/sounds/tank-move.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("tank-move");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.tankMoveBuf = buf;
        return buf;
      })
      .catch(() => {
        this.tankMoveWait = null;
        return null;
      });
    return this.tankMoveWait;
  },

  loadTankShoot() {
    if (this.tankShootBuf) return Promise.resolve(this.tankShootBuf);
    if (this.tankShootWait) return this.tankShootWait;
    if (!this.ctx) return Promise.resolve(null);
    this.tankShootWait = fetch("/assets/sounds/tank-shoot.wav")
      .then((res) => {
        if (!res.ok) throw new Error("tank-shoot");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.tankShootBuf = buf;
        return buf;
      })
      .catch(() => {
        this.tankShootWait = null;
        return null;
      });
    return this.tankShootWait;
  },

  loadTankDestroyed() {
    if (this.tankDestroyedBuf) return Promise.resolve(this.tankDestroyedBuf);
    if (this.tankDestroyedWait) return this.tankDestroyedWait;
    if (!this.ctx) return Promise.resolve(null);
    this.tankDestroyedWait = fetch("/assets/sounds/tank-destroyed.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("tank-destroyed");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.tankDestroyedBuf = buf;
        return buf;
      })
      .catch(() => {
        this.tankDestroyedWait = null;
        return null;
      });
    return this.tankDestroyedWait;
  },

  tankDestroyed(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.wreck);
    if (vol <= 0.004) return;
    this.duckRifles(1.1 + vol * 0.8);
    const play = (buf) => {
      if (!buf || !this.ctx) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      this.startSpatialSource(src, x, y, vol * 1.15, this.dest("tank"), 2.6, 1.55);
    };
    if (this.tankDestroyedBuf) play(this.tankDestroyedBuf);
    else void this.loadTankDestroyed().then(play);
  },

  loadMlrsRocket() {
    if (this.mlrsRocketBuf) return Promise.resolve(this.mlrsRocketBuf);
    if (this.mlrsRocketWait) return this.mlrsRocketWait;
    if (!this.ctx) return Promise.resolve(null);
    this.mlrsRocketWait = fetch("/assets/sounds/mlrs-rocket.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("mlrs-rocket");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.mlrsRocketBuf = buf;
        return buf;
      })
      .catch(() => {
        this.mlrsRocketWait = null;
        return null;
      });
    return this.mlrsRocketWait;
  },

  mlrsRocket(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.mlrs);
    if (vol <= 0.005) return;
    this.duckRifles(0.35 + vol * 0.35);
    const play = (buf) => {
      if (!buf || !this.ctx) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.playbackRate.value = 0.94 + Math.random() * 0.12;
      this.startSpatialSource(src, x, y, vol, this.dest("tank"), 2.35, 1.35);
    };
    if (this.mlrsRocketBuf) play(this.mlrsRocketBuf);
    else void this.loadMlrsRocket().then(play);
  },

  loadPatriot() {
    if (this.patriotBuf) return Promise.resolve(this.patriotBuf);
    if (this.patriotWait) return this.patriotWait;
    if (!this.ctx) return Promise.resolve(null);
    this.patriotWait = fetch("/assets/sounds/patriot.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("patriot");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.patriotBuf = buf;
        return buf;
      })
      .catch(() => {
        this.patriotWait = null;
        return null;
      });
    return this.patriotWait;
  },

  patriotLaunch(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.patriot);
    if (vol <= 0.004) return;
    this.duckRifles(0.85 + vol * 0.55);
    const play = (buf) => {
      if (!buf || !this.ctx) return;
      const src = this.ctx.createBufferSource();
      src.buffer = buf;
      src.playbackRate.value = 0.97 + Math.random() * 0.06;
      this.startSpatialSource(src, x, y, vol * 1.1, this.dest("tank"), 2.5, 1.5);
    };
    if (this.patriotBuf) play(this.patriotBuf);
    else void this.loadPatriot().then(play);
  },

  loadSoldierShoot() {
    if (this.soldierShootBuf) return Promise.resolve(this.soldierShootBuf);
    if (this.soldierShootWait) return this.soldierShootWait;
    if (!this.ctx) return Promise.resolve(null);
    this.soldierShootWait = fetch("/assets/sounds/soldier-shoot.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("soldier-shoot");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.soldierShootBuf = buf;
        return buf;
      })
      .catch(() => {
        this.soldierShootWait = null;
        return null;
      });
    return this.soldierShootWait;
  },

  loadBuilding() {
    if (this.buildingBuf) return Promise.resolve(this.buildingBuf);
    if (this.buildingWait) return this.buildingWait;
    if (!this.ctx) return Promise.resolve(null);
    this.buildingWait = fetch("/assets/sounds/building.mp3")
      .then((res) => {
        if (!res.ok) throw new Error("building");
        return res.arrayBuffer();
      })
      .then((raw) => this.ctx.decodeAudioData(raw))
      .then((buf) => {
        this.buildingBuf = buf;
        return buf;
      })
      .catch(() => {
        this.buildingWait = null;
        return null;
      });
    return this.buildingWait;
  },

  startEngineLoop(node) {
    if (!this.tankMoveBuf || !this.ctx || node.src) return;
    const src = this.ctx.createBufferSource();
    src.buffer = this.tankMoveBuf;
    src.loop = true;
    src.connect(node.g);
    try {
      src.start();
    } catch {
      return;
    }
    node.src = src;
  },

  setEngine(id, x, y, throttle) {
    if (!this.ensure() || !id) return;
    const moving = throttle > 0.22;
    void this.loadTankMove().then((buf) => {
      if (!buf) return;
      const node = this.engines.get(id);
      if (node && !node.stopping && node.throttle > 0.22) this.startEngineLoop(node);
    });
    let node = this.engines.get(id);
    if (!moving) {
      if (node) this.stopEngine(id);
      return;
    }
    if (node?.stopping) {
      this.engines.delete(id);
      node = null;
    }
    if (!node) {
      const g = this.ctx.createGain();
      g.gain.value = 0.0001;
      const pan = this.ctx.createStereoPanner();
      g.connect(pan);
      pan.connect(this.dest("tank"));
      node = { src: null, g, pan, x, y, throttle: 0, stopping: false };
      this.engines.set(id, node);
      if (this.tankMoveBuf) this.startEngineLoop(node);
    }
    node.x = x;
    node.y = y;
    node.throttle = Math.max(0, Math.min(1, throttle));
  },

  stopEngine(id) {
    const node = this.engines.get(id);
    if (!node || node.stopping) return;
    node.stopping = true;
    node.throttle = 0;
    try {
      const t0 = this.ctx.currentTime;
      node.g.gain.cancelScheduledValues(t0);
      node.g.gain.setValueAtTime(Math.max(0.0001, node.g.gain.value), t0);
      node.g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.08);
      node.src?.stop(t0 + 0.09);
      node.src = null;
    } catch {
      // ignore
    }
    try {
      node.g.disconnect();
    } catch {
      // ignore
    }
    this.engines.delete(id);
  },

  missile(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.missile);
    if (vol <= 0.004) return;
    const bus = this.dest("fx");
    const loud = vol * 3;
    this.noiseBurst(0.2, 0.14, 1100, "bandpass", loud, 0.7, bus);
    this.tone(520, 0.25, "sawtooth", 0.07, 180, loud, bus);
  },

  startBuildLoop(node) {
    if (!this.buildingBuf || !this.ctx || node.src) return;
    const src = this.ctx.createBufferSource();
    src.buffer = this.buildingBuf;
    src.loop = true;
    src.connect(node.g);
    try {
      src.start();
    } catch {
      return;
    }
    node.src = src;
  },

  startBuild(id, x, y) {
    if (!this.ensure() || !id) return;
    void this.loadBuilding().then((buf) => {
      if (!buf) return;
      const node = this.builds.get(id);
      if (node && !node.stopping) this.startBuildLoop(node);
    });
    let node = this.builds.get(id);
    if (node?.stopping) {
      this.builds.delete(id);
      node = null;
    }
    if (node) {
      node.x = x;
      node.y = y;
      return;
    }
    const g = this.ctx.createGain();
    g.gain.value = 0.0001;
    const pan = this.ctx.createStereoPanner();
    pan.pan.value = this.panAt(x);
    g.connect(pan);
    pan.connect(this.dest("fx"));
    const vol = this.volumeAt(x, y, this.ranges.build);
    const t0 = this.ctx.currentTime;
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(
      Math.max(0.0001, this.sampleGain(vol, 0.036, 1.9)),
      t0 + 0.4,
    );
    node = { src: null, g, pan, x, y, stopping: false, baseGain: 0.036 };
    this.builds.set(id, node);
    if (this.buildingBuf) this.startBuildLoop(node);
  },

  stopBuild(id) {
    const node = this.builds.get(id);
    if (!node || node.stopping) return;
    node.stopping = true;
    try {
      const t0 = this.ctx.currentTime;
      node.g.gain.cancelScheduledValues(t0);
      node.g.gain.setValueAtTime(Math.max(0.0001, node.g.gain.value), t0);
      node.g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.22);
      node.src?.stop(t0 + 0.24);
      node.src = null;
    } catch {
      // ignore
    }
    try {
      node.g.disconnect();
    } catch {
      // ignore
    }
    this.builds.delete(id);
  },

  /** Keep looping build hum / tank engines loud/quiet as the camera pans. */
  updateSpatial() {
    if (!this.ctx) return;
    const t0 = this.ctx.currentTime;
    for (const node of this.builds.values()) {
      if (node.stopping) continue;
      const vol = this.volumeAt(node.x, node.y, this.ranges.build);
      const target = Math.max(0.0001, this.sampleGain(vol, node.baseGain || 0.036, 1.9));
      try {
        node.g.gain.setTargetAtTime(target, t0, 0.12);
        if (node.pan) node.pan.pan.setTargetAtTime(this.panAt(node.x), t0, 0.12);
      } catch {
        // ignore
      }
    }
    for (const node of this.engines.values()) {
      if (node.stopping) continue;
      const vol = this.volumeAt(node.x, node.y, this.ranges.tank);
      const th = node.throttle || 0;
      const target = Math.max(0.0001, this.sampleGain(vol, 0.032, 2.2) * (0.45 + th * 0.55));
      try {
        node.g.gain.setTargetAtTime(target, t0, 0.08);
        if (node.pan) node.pan.pan.setTargetAtTime(this.panAt(node.x), t0, 0.08);
        node.src?.playbackRate?.setTargetAtTime(0.94 + th * 0.12, t0, 0.2);
      } catch {
        // ignore
      }
    }
  },

  buildingComplete(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.complete);
    if (vol <= 0.004) return;
    const bus = this.dest("fx");
    const loud = vol * 3;
    this.tone(320, 0.12, "triangle", 0.06, 520, loud, bus);
    this.tone(480, 0.16, "sine", 0.05, 640, loud, bus);
    this.noiseBurst(0.08, 0.05, 2000, "highpass", loud, 0.7, bus);
  },

  buildingCollapse(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.collapse);
    if (vol <= 0.004) return;
    const bus = this.dest("fx");
    const loud = vol * 3;
    this.noiseBurst(0.55, 0.32, 220, "lowpass", loud, 0.7, bus);
    this.noiseBurst(0.35, 0.2, 700, "bandpass", loud, 0.7, bus);
    this.tone(140, 0.5, "sawtooth", 0.1, 40, loud, bus);
    this.tone(70, 0.6, "sine", 0.12, 28, loud, bus);
    setTimeout(() => this.noiseBurst(0.25, 0.12, 400, "lowpass", this.volumeAt(x, y, this.ranges.collapse) * 2.1, 0.7, this.dest("fx")), 120);
    setTimeout(() => this.noiseBurst(0.2, 0.08, 900, "bandpass", this.volumeAt(x, y, this.ranges.collapse) * 1.5, 0.7, this.dest("fx")), 220);
  },

  shot(kind, x, y) {
    const k = String(kind || "");
    if (k.includes("tank_mg") || k.includes("_mg") || k.includes("bunker")) this.rifle(x, y);
    else if (k.includes("mlrs")) this.mlrsRocket(x, y);
    else if (k.includes("patriot") || k === "turret") this.patriotLaunch(x, y);
    else if (k.includes("tank")) this.tankCannon(x, y);
    else if (k.includes("mortar")) this.missile(x, y);
    else if (k.includes("missile")) this.missile(x, y);
    else this.rifle(x, y);
  },
};

function shotKindOf(shot) {
  const fromMesh = state.meshes.get(shot.from);
  return (
    shot.kind ||
    (fromMesh?.userData?.isTank ? "tank" : fromMesh?.userData?.kind) ||
    ""
  );
}

function playShots(shots) {
  const list = [...(shots || [])];
  const rank = (kind) => {
    const k = String(kind || "");
    if (k.includes("tank") && !k.includes("mg") && !k.includes("mlrs")) return 0;
    if (k.includes("mlrs") || k.includes("mortar") || k.includes("missile")) return 1;
    return 2;
  };
  list.sort((a, b) => rank(shotKindOf(a)) - rank(shotKindOf(b)));
  let rifles = 0;
  for (const shot of list) {
    spawnShotFx(shot);
    const kind = shotKindOf(shot);
    const k = String(kind);
    Radar.ping(shot.x0, shot.y0, k);
    if (k.includes("tank") && !k.includes("mg") && shot.x1 != null && shot.y1 != null) {
      Radar.ping(shot.x1, shot.y1, "tank");
    }
    if (k.includes("mlrs") && shot.x1 != null && shot.y1 != null) {
      Radar.ping(shot.x1, shot.y1, "mlrs");
    }
    const isCannon = k.includes("tank") && !k.includes("mg") && !k.includes("mlrs");
    const isHeavy =
      k.includes("missile") ||
      k.includes("mortar") ||
      k.includes("mlrs") ||
      k.includes("patriot") ||
      k.includes("bunker");
    if (!isCannon && !isHeavy) {
      rifles += 1;
      if (rifles > 4) continue;
    }
    Sfx.shot(kind, shot.x0, shot.y0);
  }
}

/* ---------- Generals-style radar (minimap) ---------- */

const Radar = {
  canvas: null,
  ctx: null,
  pings: [],
  dragging: false,
  fogScratch: null,
  fogImage: null,
  fogAt: 0,
  lastDraw: 0,
  rifleSkip: 0,
  bound: false,

  bind() {
    this.canvas = $("#radar-canvas");
    if (!this.canvas) return;
    this.ctx = this.canvas.getContext("2d", { alpha: false });
    if (!this.timer) {
      this.timer = setInterval(() => {
        if (!state.match || $("#match-screen")?.hidden) return;
        this.draw(performance.now());
      }, 200);
    }
    if (this.bound) return;
    this.bound = true;
    const go = (event) => {
      const w = this.eventToWorld(event);
      if (!w) return;
      panCameraTo(w.x, w.y, true);
    };
    this.canvas.addEventListener("pointerdown", (event) => {
      event.preventDefault();
      this.dragging = true;
      this.canvas.setPointerCapture?.(event.pointerId);
      go(event);
    });
    this.canvas.addEventListener("pointermove", (event) => {
      if (!this.dragging) return;
      go(event);
    });
    const stop = () => {
      this.dragging = false;
      if (controls) {
        lastFocusSent = { x: controls.target.x, z: controls.target.z };
        send({ t: "set_focus", x: controls.target.x, y: controls.target.z });
      }
    };
    this.canvas.addEventListener("pointerup", stop);
    this.canvas.addEventListener("pointercancel", stop);
    this.canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  },

  eventToWorld(event) {
    if (!this.canvas || !mapSize) return null;
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width < 2 || rect.height < 2) return null;
    const x = ((event.clientX - rect.left) / rect.width) * mapSize;
    const y = ((event.clientY - rect.top) / rect.height) * mapSize;
    return { x, y };
  },

  ping(x, y, kind) {
    const k = String(kind || "");
    const tank = k.includes("tank");
    const heavy =
      k.includes("missile") || k.includes("mortar") || k.includes("patriot");
    if (!tank && !heavy) {
      this.rifleSkip += 1;
      if (this.rifleSkip % 5) return;
    }
    this.pings.push({
      x,
      y,
      tank,
      born: performance.now(),
      life: tank || heavy ? 900 : 500,
    });
    if (this.pings.length > 18) this.pings.splice(0, this.pings.length - 18);
  },

  worldToPx(x, y, w, h) {
    return [(x / mapSize) * w, (y / mapSize) * h];
  },

  drawFog(ctx, w, h) {
    ctx.fillStyle = "#0b120b";
    ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = "#1c2818";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(w * 0.5, 0);
    ctx.lineTo(w * 0.5, h);
    ctx.moveTo(0, h * 0.5);
    ctx.lineTo(w, h * 0.5);
    ctx.stroke();
  },

  draw(now) {
    const ctx = this.ctx;
    const canvas = this.canvas;
    if (!ctx || !canvas || !state.match || !mapSize) return;

    const w = canvas.width;
    const h = canvas.height;
    this.drawFog(ctx, w, h);

    const you = state.match.you;
    const myTeam = state.match.team;
    let ownN = 0;
    let foeN = 0;
    ctx.fillStyle = "#9fef4a";
    for (const entity of state.entities.values()) {
      if (entity.hp != null && entity.hp <= 0) continue;
      if (entity.building || entity.kind === "hq" || String(entity.kind).includes("tank") || String(entity.kind).includes("mlrs")) {
        continue;
      }
      if (entity.owner !== you) continue;
      ownN += 1;
      if (ownN % 2) continue;
      ctx.fillRect((entity.x / mapSize) * w - 0.75, (entity.y / mapSize) * h - 0.75, 2, 2);
    }
    // Allied infantry (non-FFA)
    if (!state.match.ffa) {
      ctx.fillStyle = "#5ad0ff";
      let allyN = 0;
      for (const entity of state.entities.values()) {
        if (entity.hp != null && entity.hp <= 0) continue;
        if (entity.building || entity.kind === "hq" || String(entity.kind).includes("tank") || String(entity.kind).includes("mlrs")) {
          continue;
        }
        if (entity.owner === you || entity.team !== myTeam) continue;
        allyN += 1;
        if (allyN % 2) continue;
        ctx.fillRect((entity.x / mapSize) * w - 0.75, (entity.y / mapSize) * h - 0.75, 2, 2);
      }
    }
    ctx.fillStyle = "#ff5a3a";
    for (const entity of state.entities.values()) {
      if (entity.hp != null && entity.hp <= 0) continue;
      if (entity.building || entity.kind === "hq" || String(entity.kind).includes("tank") || String(entity.kind).includes("mlrs")) {
        continue;
      }
      if (entity.owner === you || entity.team === myTeam) continue;
      foeN += 1;
      if (foeN % 3) continue;
      ctx.fillRect((entity.x / mapSize) * w - 0.75, (entity.y / mapSize) * h - 0.75, 2, 2);
    }

    for (const entity of state.entities.values()) {
      if (entity.hp != null && entity.hp <= 0) continue;
      const kind = entity.kind || "";
      const heavy = entity.building || kind.includes("tank") || kind.includes("mlrs") || kind === "hq";
      if (!heavy) continue;
      const px = (entity.x / mapSize) * w;
      const py = (entity.y / mapSize) * h;
      const mine = entity.owner === you;
      const ally = !mine && entity.team === myTeam;
      ctx.fillStyle = mine ? "#9fef4a" : ally ? "#5ad0ff" : "#ff5a3a";
      if (kind === "hq") {
        ctx.fillRect(px - 2.5, py - 2.5, 5, 5);
      } else if (entity.building) {
        ctx.fillRect(px - 1.5, py - 1.5, 3, 3);
      } else {
        ctx.fillRect(px - 2, py - 2, 4, 4);
      }
    }

    this.pings = this.pings.filter((p) => now - p.born < p.life);
    ctx.strokeStyle = "rgba(255, 210, 80, 0.75)";
    ctx.lineWidth = 1.5;
    for (const ping of this.pings) {
      if (!ping.tank) continue;
      const t = (now - ping.born) / ping.life;
      ctx.beginPath();
      ctx.arc((ping.x / mapSize) * w, (ping.y / mapSize) * h, 4 + t * 7, 0, Math.PI * 2);
      ctx.stroke();
    }

    if (controls) {
      const cx = controls.target.x;
      const cz = controls.target.z;
      const dist = controls.getDistance?.() || CAMERA_DIST;
      const halfW = Math.max(6, dist * 0.7 * (camera?.aspect || 1.6));
      const halfH = Math.max(5, dist * 0.5);
      ctx.strokeStyle = "rgba(255, 244, 180, 0.9)";
      ctx.lineWidth = 1;
      ctx.strokeRect(
        ((cx - halfW) / mapSize) * w,
        ((cz - halfH) / mapSize) * h,
        (halfW * 2 / mapSize) * w,
        (halfH * 2 / mapSize) * h,
      );
    }
  },
};

/* ---------- Three.js ---------- */

let renderer, scene, camera, controls, ground, raycaster, pointer;
let mapSize = 192;
let buildingGeometries = Object.create(null);
let buildingTemplates = Object.create(null);
let buildingModelsPromise = null;
/** Procedural buildings are always ready — no async OBJ/STL wait. */
let buildingModelsReady = true;
let ghostMesh = null;
let fogOfWar = null;
let fogExploredData = null; // Uint8Array size*size — 0/1 explored
let fogVisionData = null;   // Uint8Array size*size — 0/1 currently visible
let fogDataTexture = null;
let aoiRadius = 20;
/** Opening window: full map visible until server closes fog. */
let globalVision = false;
let lastFocusSentAt = 0;
let lastFocusSent = { x: 0, z: 0 };
const edgeMouse = { x: 0, y: 0, w: 1, h: 1, inside: false, overUi: false };

/** Generals-style locked pitch (radians from vertical-ish). */
const CAMERA_PITCH = Math.PI / 3.0;
/** Very close RTS camera — almost no pull-back. */
const CAMERA_DIST = 9;
const CAMERA_DIST_MIN = 7;
const CAMERA_DIST_MAX = 10;
const CAMERA_FOV = 32;
const EDGE_SCROLL_PX = 160;

/** Visual max-dimension targets — must match server `building_visual_size`. */
const BUILDING_VISUAL = {
  hq: 2.15,
  war_factory: 2.1,
  arms_dealer: 2.1,
  barracks: 1.35,
  power_plant: 1.7,
  nuclear_reactor: 1.7,
  supply: 1.7,
  supply_stash: 1.7,
  airfield: 2.0,
  strategy_center: 1.9,
  propaganda_center: 1.9,
  palace: 1.9,
  internet_center: 1.9,
  black_market: 1.9,
  turret: 0.55,
  stinger_site: 0.55,
  gatling_cannon: 0.55,
  bunker: 0.34,
  tunnel_network: 0.34,
  demo_trap: 0.34,
  radar: 0.85,
  firebase: 0.85,
  particle_cannon: 2.2,
  nuclear_silo: 2.2,
  scud_storm: 2.2,
};

/** Bump when procedural building meshes change so live matches remesh. */
const BUILDING_FIT_VERSION = 6;
/** Procedural Patriot mesh revision — forces remesh of old batteries. */
const PATRIOT_RIG_VERSION = 3;

function bldgPart(parent, geo, color, x, y, z, rx = 0, ry = 0, rz = 0, opts = {}) {
  const m = new THREE.Mesh(
    geo,
    matStd(color, {
      metalness: opts.metalness ?? 0.28,
      roughness: opts.roughness ?? 0.62,
      emissive: opts.emissive,
      emissiveIntensity: opts.emissiveIntensity,
    }),
  );
  if (opts.transparent) {
    m.material.transparent = true;
    m.material.opacity = opts.opacity ?? 0.85;
    m.material.depthWrite = (opts.opacity ?? 0.85) >= 0.95;
  }
  m.position.set(x, y, z);
  m.rotation.set(rx, ry, rz);
  m.castShadow = opts.cast !== false;
  m.receiveShadow = true;
  parent.add(m);
  return m;
}

function finishProcBuilding(root, kind, unitHeight) {
  root.userData.building = true;
  root.userData.modelKind = kind;
  root.userData.isFallback = false;
  root.userData.keepMtlColors = true;
  root.userData.buildingFitVersion = BUILDING_FIT_VERSION;
  root.userData.unitHeight = unitHeight;
  root.userData.procBuilding = true;
  return root;
}

function milPalette(accent) {
  return {
    accent: accent ?? 0x556b2f,
    olive: 0x4a5538,
    oliveDark: 0x32382a,
    oliveLight: 0x5c6648,
    concrete: 0x6a675c,
    concreteDark: 0x4a4840,
    concreteLight: 0x7e7a6e,
    metal: 0x3a3c38,
    metalBright: 0x5c6058,
    rust: 0x5a4030,
    glass: 0x1a2830,
    sand: 0x6b6550,
    warning: 0xb8860b,
    black: 0x141210,
  };
}

/** Command Center — fortified HQ blockhouse + comms tower. */
function createCommandCenterMesh(fallbackMat) {
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();

  // Plinth / blast apron
  bldgPart(root, new THREE.BoxGeometry(1.95, 0.07, 1.75), p.concreteDark, 0, 0.035, 0, 0, 0, 0, {
    roughness: 0.92,
    metalness: 0.08,
    cast: false,
  });
  bldgPart(root, new THREE.BoxGeometry(1.82, 0.04, 1.62), p.concrete, 0, 0.08, 0, 0, 0, 0, {
    roughness: 0.88,
    metalness: 0.1,
  });

  // Main keep
  bldgPart(root, new THREE.BoxGeometry(1.35, 0.55, 1.05), p.olive, 0, 0.38, 0.02, 0, 0, 0, {
    roughness: 0.7,
  });
  bldgPart(root, new THREE.BoxGeometry(1.38, 0.06, 1.08), p.oliveDark, 0, 0.68, 0.02);
  // Upper CIC
  bldgPart(root, new THREE.BoxGeometry(0.95, 0.32, 0.72), p.oliveLight, 0.08, 0.9, 0.05);
  bldgPart(root, new THREE.BoxGeometry(0.98, 0.04, 0.75), p.metal, 0.08, 1.08, 0.05, 0, 0, 0, {
    metalness: 0.45,
    roughness: 0.45,
  });

  // Corner bastions
  for (const [x, z] of [
    [-0.62, -0.42],
    [0.62, -0.42],
    [-0.62, 0.48],
    [0.62, 0.48],
  ]) {
    bldgPart(root, new THREE.BoxGeometry(0.22, 0.42, 0.22), p.oliveDark, x, 0.35, z);
    bldgPart(root, new THREE.BoxGeometry(0.24, 0.04, 0.24), p.concreteLight, x, 0.57, z);
  }

  // Entry vestibule + door
  bldgPart(root, new THREE.BoxGeometry(0.42, 0.38, 0.28), p.oliveDark, 0, 0.3, 0.62);
  bldgPart(root, new THREE.BoxGeometry(0.22, 0.28, 0.04), p.black, 0, 0.28, 0.76, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });
  bldgPart(root, new THREE.BoxGeometry(0.55, 0.05, 0.35), p.concrete, 0, 0.1, 0.78, 0, 0, 0, {
    roughness: 0.9,
    cast: false,
  });

  // Window slits of glass
  for (const [x, y, z, w] of [
    [-0.38, 0.48, 0.56, 0.18],
    [0.38, 0.48, 0.56, 0.18],
    [-0.25, 0.92, 0.42, 0.28],
    [0.35, 0.92, 0.42, 0.22],
  ]) {
    bldgPart(root, new THREE.BoxGeometry(w, 0.1, 0.03), p.glass, x, y, z, 0, 0, 0, {
      metalness: 0.7,
      roughness: 0.2,
      transparent: true,
      opacity: 0.75,
    });
  }

  // Comms tower
  bldgPart(root, new THREE.BoxGeometry(0.28, 0.75, 0.28), p.metal, -0.45, 1.15, -0.25, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });
  bldgPart(root, new THREE.CylinderGeometry(0.03, 0.04, 0.55, 8), p.metalBright, -0.45, 1.75, -0.25, 0, 0, 0, {
    metalness: 0.65,
    roughness: 0.3,
  });
  // Dish
  bldgPart(
    root,
    new THREE.CylinderGeometry(0.16, 0.02, 0.05, 12),
    p.metalBright,
    -0.45,
    1.95,
    -0.15,
    Math.PI / 2.4,
    0.4,
    0,
    { metalness: 0.55, roughness: 0.35 },
  );
  bldgPart(root, new THREE.SphereGeometry(0.035, 8, 8), 0xaa2200, -0.45, 2.02, -0.25, 0, 0, 0, {
    emissive: 0x440000,
    emissiveIntensity: 0.35,
    cast: false,
  });

  // Side antenna farm
  for (let i = 0; i < 4; i++) {
    const ax = 0.35 + (i % 2) * 0.12;
    const az = -0.35 - Math.floor(i / 2) * 0.1;
    bldgPart(root, new THREE.CylinderGeometry(0.012, 0.015, 0.35 + i * 0.05, 6), p.metal, ax, 1.25, az, 0, 0, 0, {
      metalness: 0.6,
      roughness: 0.35,
    });
  }

  // Team stripe + sandbags
  bldgPart(root, new THREE.BoxGeometry(1.2, 0.05, 0.06), p.accent, 0, 0.55, 0.55, 0, 0, 0, {
    metalness: 0.15,
    roughness: 0.55,
  });
  for (const [sx, sz] of [
    [-0.85, 0.55],
    [0.85, 0.55],
    [-0.9, -0.55],
    [0.9, -0.55],
    [-0.95, 0],
    [0.95, 0],
  ]) {
    bldgPart(root, new THREE.BoxGeometry(0.16, 0.1, 0.12), p.sand, sx, 0.14, sz, 0, sx * 0.1, 0, {
      roughness: 0.95,
      cast: false,
    });
  }

  return finishProcBuilding(root, "hq", 2.05);
}

/** Barracks — Quonset hall + side annex. */
function createBarracksMesh(fallbackMat) {
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();

  bldgPart(root, new THREE.BoxGeometry(1.25, 0.05, 0.85), p.concreteDark, 0, 0.025, 0, 0, 0, 0, {
    roughness: 0.92,
    cast: false,
  });

  // Quonset vault (approx with box + roof wedges)
  bldgPart(root, new THREE.BoxGeometry(1.1, 0.32, 0.62), p.olive, 0, 0.24, 0);
  bldgPart(root, new THREE.BoxGeometry(1.12, 0.08, 0.64), p.metal, 0, 0.44, 0, 0, 0, 0, {
    metalness: 0.4,
    roughness: 0.5,
  });
  // Arched roof strips
  for (let i = -2; i <= 2; i++) {
    bldgPart(
      root,
      new THREE.BoxGeometry(0.08, 0.14, 0.64),
      p.oliveDark,
      i * 0.2,
      0.48,
      0,
      0,
      0,
      i * 0.12,
      { metalness: 0.35, roughness: 0.55 },
    );
  }

  // Side annex
  bldgPart(root, new THREE.BoxGeometry(0.35, 0.28, 0.4), p.oliveLight, 0.55, 0.22, 0.15);
  bldgPart(root, new THREE.BoxGeometry(0.12, 0.2, 0.03), p.black, 0.72, 0.2, 0.15, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });

  // Front door + steps
  bldgPart(root, new THREE.BoxGeometry(0.2, 0.26, 0.04), p.black, -0.25, 0.2, 0.33);
  bldgPart(root, new THREE.BoxGeometry(0.35, 0.04, 0.18), p.concrete, -0.25, 0.06, 0.4, 0, 0, 0, {
    roughness: 0.9,
    cast: false,
  });

  // Windows
  for (const x of [-0.35, 0.05, 0.35]) {
    bldgPart(root, new THREE.BoxGeometry(0.14, 0.1, 0.03), p.glass, x, 0.3, 0.32, 0, 0, 0, {
      metalness: 0.65,
      roughness: 0.25,
      transparent: true,
      opacity: 0.7,
    });
  }

  // Chimney / stove pipe
  bldgPart(root, new THREE.CylinderGeometry(0.035, 0.04, 0.28, 8), p.rust, 0.35, 0.62, -0.1, 0, 0, 0, {
    metalness: 0.45,
    roughness: 0.55,
  });

  // Accent stripe + sandbags
  bldgPart(root, new THREE.BoxGeometry(0.9, 0.04, 0.05), p.accent, 0, 0.38, 0.32);
  for (const [x, z] of [
    [-0.55, 0.35],
    [0.2, 0.38],
    [0.55, -0.25],
  ]) {
    bldgPart(root, new THREE.BoxGeometry(0.14, 0.08, 0.1), p.sand, x, 0.1, z, 0, 0, 0, {
      roughness: 0.95,
      cast: false,
    });
  }

  return finishProcBuilding(root, "barracks", 0.7);
}

/** Cold Fusion Reactor — containment drum + cooling stacks. */
function createPowerPlantMesh(fallbackMat) {
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();

  bldgPart(root, new THREE.CylinderGeometry(0.72, 0.78, 0.06, 16), p.concreteDark, 0, 0.03, 0, 0, 0, 0, {
    roughness: 0.92,
    cast: false,
  });

  // Reactor vessel
  bldgPart(root, new THREE.CylinderGeometry(0.42, 0.48, 0.55, 16), p.metal, 0, 0.35, 0, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.35,
  });
  bldgPart(root, new THREE.CylinderGeometry(0.38, 0.38, 0.12, 16), p.metalBright, 0, 0.68, 0, 0, 0, 0, {
    metalness: 0.6,
    roughness: 0.3,
  });
  // Dome
  bldgPart(root, new THREE.SphereGeometry(0.38, 16, 12, 0, Math.PI * 2, 0, Math.PI / 2), p.concreteLight, 0, 0.72, 0, 0, 0, 0, {
    metalness: 0.2,
    roughness: 0.55,
  });

  // Cooling fins
  for (let i = 0; i < 8; i++) {
    const a = (i / 8) * Math.PI * 2;
    bldgPart(
      root,
      new THREE.BoxGeometry(0.06, 0.4, 0.14),
      p.oliveDark,
      Math.cos(a) * 0.52,
      0.35,
      Math.sin(a) * 0.52,
      0,
      -a,
      0,
      { metalness: 0.4, roughness: 0.5 },
    );
  }

  // Exhaust stacks
  for (const [x, z] of [
    [-0.55, 0.45],
    [0.55, 0.45],
  ]) {
    bldgPart(root, new THREE.CylinderGeometry(0.07, 0.09, 0.7, 10), p.olive, x, 0.45, z);
    bldgPart(root, new THREE.CylinderGeometry(0.08, 0.08, 0.06, 10), p.warning, x, 0.82, z, 0, 0, 0, {
      metalness: 0.3,
      roughness: 0.5,
    });
  }

  // Control annex
  bldgPart(root, new THREE.BoxGeometry(0.45, 0.28, 0.35), p.olive, 0.55, 0.22, -0.35);
  bldgPart(root, new THREE.BoxGeometry(0.2, 0.1, 0.03), p.glass, 0.55, 0.28, -0.52, 0, 0, 0, {
    metalness: 0.65,
    roughness: 0.25,
    transparent: true,
    opacity: 0.7,
  });

  // Hazard rings + glow core hint
  bldgPart(root, new THREE.TorusGeometry(0.4, 0.025, 8, 24), p.warning, 0, 0.45, 0, Math.PI / 2, 0, 0, {
    metalness: 0.35,
    roughness: 0.45,
  });
  bldgPart(root, new THREE.SphereGeometry(0.08, 10, 10), 0x88ccff, 0, 0.55, 0, 0, 0, 0, {
    emissive: 0x226688,
    emissiveIntensity: 0.55,
    metalness: 0.2,
    roughness: 0.3,
    cast: false,
  });
  bldgPart(root, new THREE.BoxGeometry(0.5, 0.04, 0.05), p.accent, 0.55, 0.36, -0.35);

  return finishProcBuilding(root, "power_plant", 1.05);
}

/** Supply Center — warehouse + dock + crates. */
function createSupplyCenterMesh(fallbackMat) {
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();

  bldgPart(root, new THREE.BoxGeometry(1.55, 0.05, 1.15), p.concreteDark, 0, 0.025, 0, 0, 0, 0, {
    roughness: 0.92,
    cast: false,
  });

  // Warehouse hall
  bldgPart(root, new THREE.BoxGeometry(1.25, 0.55, 0.85), p.olive, 0, 0.35, -0.05);
  bldgPart(root, new THREE.BoxGeometry(1.28, 0.06, 0.88), p.metal, 0, 0.65, -0.05, 0, 0, 0, {
    metalness: 0.4,
    roughness: 0.5,
  });
  // Sawtooth roof ridges
  for (let i = -2; i <= 2; i++) {
    bldgPart(root, new THREE.BoxGeometry(0.18, 0.1, 0.88), p.oliveDark, i * 0.22, 0.72, -0.05, 0, 0, 0.25, {
      metalness: 0.3,
      roughness: 0.55,
    });
  }

  // Loading dock
  bldgPart(root, new THREE.BoxGeometry(0.7, 0.18, 0.35), p.concrete, 0, 0.14, 0.55, 0, 0, 0, {
    roughness: 0.88,
  });
  bldgPart(root, new THREE.BoxGeometry(0.55, 0.42, 0.04), p.black, 0, 0.38, 0.4, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });
  // Roll stripes on door
  for (let i = 0; i < 5; i++) {
    bldgPart(root, new THREE.BoxGeometry(0.52, 0.025, 0.02), p.metalBright, 0, 0.22 + i * 0.07, 0.42, 0, 0, 0, {
      metalness: 0.45,
      roughness: 0.45,
      cast: false,
    });
  }

  // Crane gantry
  bldgPart(root, new THREE.BoxGeometry(0.06, 0.55, 0.06), p.metal, 0.55, 0.55, 0.35, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.35,
  });
  bldgPart(root, new THREE.BoxGeometry(0.55, 0.05, 0.05), p.metalBright, 0.3, 0.82, 0.35, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.35,
  });
  bldgPart(root, new THREE.BoxGeometry(0.04, 0.2, 0.04), p.warning, 0.15, 0.7, 0.35);

  // Crate stacks
  const crate = 0x6b5a3a;
  for (const [x, y, z, s] of [
    [-0.55, 0.14, 0.45, 0.16],
    [-0.55, 0.28, 0.45, 0.14],
    [0.55, 0.12, 0.5, 0.18],
    [-0.65, 0.12, -0.35, 0.15],
  ]) {
    bldgPart(root, new THREE.BoxGeometry(s, s * 0.85, s), crate, x, y, z, 0, 0.2, 0, {
      roughness: 0.85,
      metalness: 0.1,
    });
  }

  bldgPart(root, new THREE.BoxGeometry(1.0, 0.04, 0.05), p.accent, 0, 0.5, 0.38);
  return finishProcBuilding(root, "supply", 0.85);
}

/** War Factory — tank hangar + assembly bay. */
function createWarFactoryMesh(fallbackMat) {
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();

  bldgPart(root, new THREE.BoxGeometry(1.95, 0.06, 1.45), p.concreteDark, 0, 0.03, 0, 0, 0, 0, {
    roughness: 0.92,
    cast: false,
  });

  // Main hangar
  bldgPart(root, new THREE.BoxGeometry(1.7, 0.7, 1.1), p.olive, 0, 0.42, 0);
  bldgPart(root, new THREE.BoxGeometry(1.74, 0.08, 1.14), p.metal, 0, 0.8, 0, 0, 0, 0, {
    metalness: 0.42,
    roughness: 0.48,
  });
  // Roof ridges
  for (let i = -3; i <= 3; i++) {
    bldgPart(root, new THREE.BoxGeometry(0.1, 0.12, 1.14), p.oliveDark, i * 0.22, 0.9, 0, 0, 0, 0.15, {
      metalness: 0.35,
      roughness: 0.5,
    });
  }

  // Giant bay door (front)
  bldgPart(root, new THREE.BoxGeometry(0.95, 0.55, 0.05), p.black, 0, 0.38, 0.58, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.38,
  });
  for (let i = 0; i < 6; i++) {
    bldgPart(root, new THREE.BoxGeometry(0.9, 0.03, 0.02), p.metalBright, 0, 0.16 + i * 0.09, 0.61, 0, 0, 0, {
      metalness: 0.5,
      cast: false,
    });
  }
  // Door frame
  bldgPart(root, new THREE.BoxGeometry(1.05, 0.06, 0.08), p.metal, 0, 0.68, 0.58, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });

  // Side workshop wing
  bldgPart(root, new THREE.BoxGeometry(0.45, 0.4, 0.7), p.oliveLight, 0.95, 0.28, -0.15);
  bldgPart(root, new THREE.BoxGeometry(0.03, 0.16, 0.28), p.glass, 1.17, 0.32, -0.15, 0, 0, 0, {
    metalness: 0.65,
    roughness: 0.25,
    transparent: true,
    opacity: 0.7,
  });

  // Smokestacks
  for (const [x, z] of [
    [-0.65, -0.4],
    [-0.4, -0.45],
  ]) {
    bldgPart(root, new THREE.CylinderGeometry(0.08, 0.1, 0.85, 10), p.rust, x, 0.95, z, 0, 0, 0, {
      metalness: 0.4,
      roughness: 0.55,
    });
    bldgPart(root, new THREE.CylinderGeometry(0.09, 0.09, 0.05, 10), p.warning, x, 1.38, z);
  }

  // Overhead crane
  bldgPart(root, new THREE.BoxGeometry(0.08, 0.7, 0.08), p.metal, -0.75, 0.85, 0.35, 0, 0, 0, {
    metalness: 0.55,
  });
  bldgPart(root, new THREE.BoxGeometry(0.08, 0.7, 0.08), p.metal, 0.75, 0.85, 0.35, 0, 0, 0, {
    metalness: 0.55,
  });
  bldgPart(root, new THREE.BoxGeometry(1.55, 0.06, 0.08), p.metalBright, 0, 1.18, 0.35, 0, 0, 0, {
    metalness: 0.6,
    roughness: 0.35,
  });
  bldgPart(root, new THREE.BoxGeometry(0.2, 0.12, 0.2), p.warning, 0.2, 1.1, 0.35);

  // Ramp out
  bldgPart(root, new THREE.BoxGeometry(0.9, 0.05, 0.35), p.concrete, 0, 0.08, 0.85, 0.15, 0, 0, {
    roughness: 0.9,
    cast: false,
  });

  bldgPart(root, new THREE.BoxGeometry(1.4, 0.05, 0.06), p.accent, 0, 0.55, 0.56);
  return finishProcBuilding(root, "war_factory", 1.35);
}

function createBuildingMesh(kind, fallbackMat) {
  if (kind === "turret" || kind === "stinger_site") return createPatriotBatteryMesh(fallbackMat);
  if (kind === "gatling_cannon") return createPatriotBatteryMesh(fallbackMat);
  if (kind === "bunker" || kind === "tunnel_network") return createBunkerMesh(fallbackMat);
  if (kind === "demo_trap") {
    const m = createBunkerMesh(fallbackMat);
    m.scale.setScalar(0.55);
    return m;
  }
  if (kind === "radar" || kind === "firebase") return createRadarStationMesh(fallbackMat);
  if (kind === "hq") return createCommandCenterMesh(fallbackMat);
  if (
    kind === "strategy_center" ||
    kind === "propaganda_center" ||
    kind === "palace" ||
    kind === "internet_center" ||
    kind === "black_market"
  ) {
    return createCommandCenterMesh(fallbackMat);
  }
  if (kind === "barracks") return createBarracksMesh(fallbackMat);
  if (
    kind === "power_plant" ||
    kind === "nuclear_reactor" ||
    kind === "particle_cannon" ||
    kind === "nuclear_silo" ||
    kind === "scud_storm"
  ) {
    const m = createPowerPlantMesh(fallbackMat);
    if (kind === "particle_cannon" || kind === "nuclear_silo" || kind === "scud_storm") {
      m.scale.setScalar(1.25);
    }
    return m;
  }
  if (kind === "supply" || kind === "supply_stash") return createSupplyCenterMesh(fallbackMat);
  if (kind === "war_factory" || kind === "arms_dealer") return createWarFactoryMesh(fallbackMat);
  if (kind === "airfield") {
    const m = createWarFactoryMesh(fallbackMat);
    m.scale.set(1.35, 1, 0.85);
    return m;
  }

  // Unknown kind — small procedural shed
  const p = milPalette(fallbackMat?.color?.getHex?.());
  const root = new THREE.Group();
  bldgPart(root, new THREE.BoxGeometry(0.9, 0.45, 0.7), p.olive, 0, 0.25, 0);
  return finishProcBuilding(root, kind, 0.5);
}

/** Hand-built MIM-104 Patriot — sized vs Crusader tank (~0.28 long) & infantry (~0.08 tall).
 *  Real launcher ~10 m → ~0.40 wu; elevated tubes ~4–5 m → ~0.35 wu. */
function createPatriotBatteryMesh(fallbackMat) {
  const accent = fallbackMat?.color?.getHex?.() ?? 0x556b2f;
  const olive = 0x4a5538;
  const oliveDark = 0x353c2c;
  const oliveLight = 0x5a6648;
  const desert = 0x6b6550;
  const metal = 0x3a3c38;
  const metalBright = 0x5c6058;
  const rubber = 0x141210;
  const glassCol = 0x1a2830;

  const root = new THREE.Group();
  root.userData.building = true;
  root.userData.isPatriot = true;
  root.userData.patriotRigVersion = PATRIOT_RIG_VERSION;
  root.userData.modelKind = "turret";
  root.userData.isFallback = false;
  root.userData.keepMtlColors = true;
  root.userData.buildingFitVersion = BUILDING_FIT_VERSION;
  root.userData.turretTurnRate = 0.95;
  root.userData.scanRate = 0.55;
  root.userData.aimYaw = 0;

  const add = (parent, geo, color, x, y, z, rx = 0, ry = 0, rz = 0, opts = {}) => {
    const m = new THREE.Mesh(
      geo,
      matStd(color, {
        metalness: opts.metalness ?? 0.35,
        roughness: opts.roughness ?? 0.55,
        transparent: opts.transparent,
        opacity: opts.opacity,
      }),
    );
    if (opts.transparent) {
      m.material.transparent = true;
      m.material.opacity = opts.opacity ?? 0.85;
      m.material.depthWrite = (opts.opacity ?? 0.85) >= 0.95;
    }
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    m.castShadow = opts.cast !== false;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  };

  // —— Soft gravel pad (tight footprint) ——
  add(root, new THREE.BoxGeometry(0.52, 0.018, 0.34), 0x3a3830, 0, 0.009, 0, 0, 0, 0, {
    metalness: 0.05,
    roughness: 0.92,
    cast: false,
  });
  add(root, new THREE.BoxGeometry(0.48, 0.006, 0.3), 0x2e2c26, 0, 0.02, 0, 0, 0, 0, {
    metalness: 0.08,
    roughness: 0.88,
    cast: false,
  });

  // —— M983-style tractor stub (short) ——
  const tractor = new THREE.Group();
  tractor.position.set(-0.16, 0, 0);
  root.add(tractor);
  add(tractor, new THREE.BoxGeometry(0.14, 0.055, 0.13), oliveDark, 0, 0.055, 0);
  add(tractor, new THREE.BoxGeometry(0.1, 0.07, 0.12), olive, -0.01, 0.11, 0);
  add(tractor, new THREE.BoxGeometry(0.08, 0.028, 0.01), glassCol, -0.01, 0.125, 0.062, 0, 0, 0, {
    metalness: 0.7,
    roughness: 0.18,
    transparent: true,
    opacity: 0.8,
  });
  // Cab wheels
  for (const z of [-0.055, 0.055]) {
    add(tractor, new THREE.CylinderGeometry(0.028, 0.028, 0.022, 12), rubber, 0.02, 0.028, z, 0, 0, Math.PI / 2, {
      metalness: 0.15,
      roughness: 0.85,
    });
    add(tractor, new THREE.CylinderGeometry(0.012, 0.012, 0.024, 8), metal, 0.02, 0.028, z, 0, 0, Math.PI / 2, {
      metalness: 0.6,
      roughness: 0.4,
    });
  }

  // —— Launcher trailer bed ——
  const bed = new THREE.Group();
  bed.position.set(0.08, 0, 0);
  root.add(bed);
  add(bed, new THREE.BoxGeometry(0.34, 0.04, 0.15), olive, 0, 0.048, 0);
  add(bed, new THREE.BoxGeometry(0.32, 0.012, 0.14), oliveDark, 0, 0.07, 0);
  // Side rails
  add(bed, new THREE.BoxGeometry(0.33, 0.018, 0.012), metalBright, 0, 0.08, -0.078, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });
  add(bed, new THREE.BoxGeometry(0.33, 0.018, 0.012), metalBright, 0, 0.08, 0.078, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });
  // Team ID stripe
  add(bed, new THREE.BoxGeometry(0.3, 0.008, 0.02), accent, 0, 0.078, -0.05, 0, 0, 0, {
    metalness: 0.2,
    roughness: 0.55,
  });
  // Trailer wheels (dual axle)
  for (const x of [-0.08, 0.1]) {
    for (const z of [-0.072, 0.072]) {
      add(bed, new THREE.CylinderGeometry(0.026, 0.026, 0.02, 12), rubber, x, 0.026, z, 0, 0, Math.PI / 2, {
        metalness: 0.12,
        roughness: 0.88,
      });
      add(bed, new THREE.CylinderGeometry(0.01, 0.01, 0.022, 8), metal, x, 0.026, z, 0, 0, Math.PI / 2, {
        metalness: 0.65,
        roughness: 0.35,
      });
    }
  }
  // Stabilizer jacks
  for (const [jx, jz] of [
    [-0.14, -0.09],
    [-0.14, 0.09],
    [0.14, -0.09],
    [0.14, 0.09],
  ]) {
    add(bed, new THREE.CylinderGeometry(0.006, 0.008, 0.04, 6), metal, jx, 0.02, jz, 0, 0, 0, {
      metalness: 0.7,
      roughness: 0.35,
    });
    add(bed, new THREE.CylinderGeometry(0.014, 0.014, 0.006, 8), desert, jx, 0.004, jz, 0, 0, 0, {
      metalness: 0.2,
      roughness: 0.8,
      cast: false,
    });
  }

  // —— Hydraulics / elevation base ——
  const launcher = new THREE.Group();
  launcher.name = "muzzleRoot";
  launcher.position.set(0.1, 0.075, 0);
  root.add(launcher);

  add(launcher, new THREE.CylinderGeometry(0.028, 0.034, 0.03, 14), metal, 0, 0.015, 0, 0, 0, 0, {
    metalness: 0.65,
    roughness: 0.35,
  });
  add(launcher, new THREE.BoxGeometry(0.06, 0.02, 0.06), oliveDark, 0, 0.032, 0);

  // Elevation cradle (~50°)
  const cradle = new THREE.Group();
  cradle.position.set(0, 0.04, 0);
  cradle.rotation.x = -0.88;
  launcher.add(cradle);

  add(cradle, new THREE.BoxGeometry(0.16, 0.028, 0.2), olive, 0, 0.02, 0.06);
  add(cradle, new THREE.BoxGeometry(0.14, 0.016, 0.18), oliveDark, 0, 0.038, 0.06);
  // Hydraulic ram
  add(cradle, new THREE.CylinderGeometry(0.008, 0.008, 0.14, 8), metalBright, -0.07, -0.02, 0.02, 0.9, 0, 0, {
    metalness: 0.75,
    roughness: 0.28,
  });
  add(cradle, new THREE.CylinderGeometry(0.006, 0.006, 0.1, 8), metal, 0.07, -0.015, 0.03, 0.9, 0, 0, {
    metalness: 0.75,
    roughness: 0.28,
  });

  // Four sealed canisters (2×2) — classic Patriot look
  const tubes = [
    [-0.038, 0.028, 0.0],
    [0.038, 0.028, 0.0],
    [-0.038, 0.028, 0.095],
    [0.038, 0.028, 0.095],
  ];
  for (const [tx, ty, tz] of tubes) {
    add(cradle, new THREE.CylinderGeometry(0.02, 0.022, 0.26, 12), oliveLight, tx, ty, tz + 0.02, Math.PI / 2, 0, 0, {
      metalness: 0.4,
      roughness: 0.45,
    });
    // Nose fairing / blast door
    add(cradle, new THREE.CylinderGeometry(0.018, 0.02, 0.012, 12), metal, tx, ty, tz + 0.155, Math.PI / 2, 0, 0, {
      metalness: 0.7,
      roughness: 0.3,
    });
    // Rear seal
    add(cradle, new THREE.CylinderGeometry(0.019, 0.019, 0.008, 10), oliveDark, tx, ty, tz - 0.115, Math.PI / 2, 0, 0, {
      metalness: 0.45,
      roughness: 0.5,
    });
    // Band clamp
    add(cradle, new THREE.TorusGeometry(0.021, 0.003, 6, 14), metalBright, tx, ty, tz + 0.04, 0, 0, Math.PI / 2, {
      metalness: 0.8,
      roughness: 0.25,
    });
  }

  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0.038, 0.028, 0.2);
  cradle.add(tip);

  // —— AN/MPQ-53 style phased-array (scans independently) ——
  const radar = new THREE.Group();
  radar.name = "radarRoot";
  radar.position.set(-0.02, 0, -0.12);
  root.add(radar);
  add(radar, new THREE.CylinderGeometry(0.012, 0.016, 0.2, 10), metal, 0, 0.12, 0, 0, 0, 0, {
    metalness: 0.7,
    roughness: 0.32,
  });
  add(radar, new THREE.BoxGeometry(0.04, 0.02, 0.04), oliveDark, 0, 0.03, 0);
  // Array face (slight tilt)
  const array = new THREE.Group();
  array.position.set(0, 0.24, 0);
  array.rotation.x = -0.35;
  radar.add(array);
  add(array, new THREE.BoxGeometry(0.14, 0.12, 0.018), metalBright, 0, 0, 0, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.35,
  });
  add(array, new THREE.BoxGeometry(0.12, 0.1, 0.006), 0x1e2a32, 0, 0, 0.012, 0, 0, 0, {
    metalness: 0.85,
    roughness: 0.12,
  });
  // Phase slots (detail lines)
  for (let i = -2; i <= 2; i++) {
    add(array, new THREE.BoxGeometry(0.11, 0.004, 0.004), 0x0e161c, 0, i * 0.018, 0.016, 0, 0, 0, {
      metalness: 0.5,
      roughness: 0.4,
      cast: false,
    });
  }
  add(array, new THREE.BoxGeometry(0.03, 0.03, 0.02), desert, 0, -0.08, -0.01, 0, 0, 0, {
    metalness: 0.3,
    roughness: 0.6,
  });

  // —— Small ECS / generator box ——
  add(root, new THREE.BoxGeometry(0.08, 0.055, 0.06), oliveDark, -0.18, 0.045, 0.1);
  add(root, new THREE.BoxGeometry(0.06, 0.012, 0.045), metal, -0.18, 0.075, 0.1, 0, 0, 0, {
    metalness: 0.6,
    roughness: 0.4,
  });
  add(root, new THREE.CylinderGeometry(0.01, 0.01, 0.03, 8), metalBright, -0.18, 0.095, 0.1, 0, 0, 0, {
    metalness: 0.7,
    roughness: 0.3,
  });

  // Cable run between radar and launcher
  add(root, new THREE.BoxGeometry(0.12, 0.008, 0.012), rubber, 0.02, 0.028, -0.06, 0, 0.4, 0, {
    metalness: 0.1,
    roughness: 0.9,
    cast: false,
  });

  return root;
}

/** Reinforced MG pillbox — crew silhouettes + rotating cupola. */
function createBunkerMesh(fallbackMat) {
  const accent = fallbackMat?.color?.getHex?.() ?? 0x556b2f;
  const concrete = 0x4e4c42;
  const concreteDark = 0x35342c;
  const dirt = 0x3a3428;
  const dirtLight = 0x4a4436;
  const sandbag = 0x5c5644;
  const metal = 0x1e201c;
  const slit = 0x0a0c08;

  const root = new THREE.Group();
  root.userData.building = true;
  root.userData.isBunker = true;
  root.userData.bunkerRigVersion = 2;
  root.userData.modelKind = "bunker";
  root.userData.isFallback = false;
  root.userData.keepMtlColors = true;
  root.userData.buildingFitVersion = BUILDING_FIT_VERSION;
  // Almost flush with ground — only the embrasure sticks up.
  root.userData.unitHeight = 0.14;
  root.userData.turretTurnRate = 1.85;
  root.userData.scanRate = 0.75;
  root.userData.aimYaw = 0;

  const add = (parent, geo, color, x, y, z, rx = 0, ry = 0, rz = 0, opts = {}) => {
    const m = new THREE.Mesh(
      geo,
      matStd(color, {
        metalness: opts.metalness ?? 0.12,
        roughness: opts.roughness ?? 0.85,
      }),
    );
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    m.castShadow = opts.cast !== false;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  };

  // Low earth mound — bunker is mostly buried.
  add(root, new THREE.CylinderGeometry(0.22, 0.28, 0.05, 12), dirt, 0, 0.02, 0, 0, 0, 0, {
    roughness: 0.96,
    cast: false,
  });
  add(root, new THREE.CylinderGeometry(0.16, 0.2, 0.035, 10), dirtLight, 0, 0.045, 0.02, 0, 0, 0, {
    roughness: 0.94,
    cast: false,
  });

  // Small concrete ring / roof slab at ground level
  add(root, new THREE.CylinderGeometry(0.11, 0.12, 0.035, 10), concrete, 0, 0.055, 0, 0, 0, 0, {
    roughness: 0.8,
  });
  add(root, new THREE.CylinderGeometry(0.09, 0.09, 0.02, 10), concreteDark, 0, 0.075, 0);

  // Tiny embrasure block — the only "building" you really see
  add(root, new THREE.BoxGeometry(0.16, 0.055, 0.1), concreteDark, 0, 0.095, 0.04);
  // Firing slit (dark recess)
  add(root, new THREE.BoxGeometry(0.1, 0.022, 0.04), slit, 0, 0.1, 0.08, 0, 0, 0, {
    metalness: 0.35,
    roughness: 0.45,
    cast: false,
  });
  // Slit lip / armor plate
  add(root, new THREE.BoxGeometry(0.12, 0.012, 0.02), metal, 0, 0.118, 0.09, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });

  // A couple of sandbags blending into the berm
  for (const [sx, sz, sy] of [
    [-0.12, 0.06, 0.06],
    [0.12, 0.05, 0.055],
    [-0.08, -0.1, 0.05],
    [0.09, -0.08, 0.05],
  ]) {
    add(root, new THREE.BoxGeometry(0.055, 0.028, 0.04), sandbag, sx, sy, sz, 0, Math.atan2(sx, sz) * 0.3, 0, {
      roughness: 0.92,
      cast: false,
    });
  }

  // Tiny team mark on the roof slab
  add(root, new THREE.BoxGeometry(0.05, 0.008, 0.02), accent, 0, 0.088, -0.05, 0, 0, 0, {
    metalness: 0.2,
    roughness: 0.55,
  });

  // —— MG in the slit (yaw only; stays low) ——
  const cupola = new THREE.Group();
  cupola.name = "muzzleRoot";
  cupola.position.set(0, 0.1, 0.06);
  root.add(cupola);

  // Gun mount barely visible in the embrasure
  add(cupola, new THREE.BoxGeometry(0.04, 0.02, 0.03), metal, 0, 0, 0, 0, 0, 0, {
    metalness: 0.6,
    roughness: 0.35,
  });
  // Single MG barrel poking out
  add(cupola, new THREE.CylinderGeometry(0.006, 0.007, 0.11, 6), metal, 0, 0.005, 0.07, Math.PI / 2, 0, 0, {
    metalness: 0.75,
    roughness: 0.28,
  });
  add(cupola, new THREE.CylinderGeometry(0.009, 0.009, 0.02, 6), metal, 0, 0.005, 0.04, Math.PI / 2, 0, 0, {
    metalness: 0.65,
    roughness: 0.35,
  });

  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0, 0.005, 0.13);
  cupola.add(tip);

  return root;
}


/** AN/TPS-style search radar — Generals / RA2 vibe, real rotating dish + ops hut. */
function createRadarStationMesh(fallbackMat) {
  const accent = fallbackMat?.color?.getHex?.() ?? 0x556b2f;
  const olive = 0x4a5538;
  const oliveDark = 0x343c2c;
  const oliveLight = 0x5a6648;
  const concrete = 0x5a5848;
  const concreteDark = 0x3e3c34;
  const metal = 0x2a2c28;
  const metalBright = 0x3a3c38;
  const dish = 0x6a7060;
  const dishDark = 0x3a4034;
  const panel = 0x1a2228;
  const sand = 0x6b6550;

  const root = new THREE.Group();
  root.userData.building = true;
  root.userData.isRadar = true;
  root.userData.radarRigVersion = 2;
  root.userData.modelKind = "radar";
  root.userData.isFallback = false;
  root.userData.keepMtlColors = true;
  root.userData.buildingFitVersion = BUILDING_FIT_VERSION;
  root.userData.unitHeight = 0.92;
  root.userData.scanRate = 0.7;

  const add = (parent, geo, color, x, y, z, rx = 0, ry = 0, rz = 0, opts = {}) => {
    const m = new THREE.Mesh(
      geo,
      matStd(color, {
        metalness: opts.metalness ?? 0.22,
        roughness: opts.roughness ?? 0.72,
      }),
    );
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    m.castShadow = opts.cast !== false;
    m.receiveShadow = true;
    parent.add(m);
    return m;
  };

  // Concrete pad + berm
  add(root, new THREE.CylinderGeometry(0.42, 0.46, 0.035, 16), concreteDark, 0, 0.015, 0, 0, 0, 0, {
    roughness: 0.95,
    cast: false,
  });
  add(root, new THREE.BoxGeometry(0.72, 0.025, 0.55), concrete, 0, 0.03, -0.02, 0, 0, 0, {
    roughness: 0.9,
    cast: false,
  });
  add(root, new THREE.CylinderGeometry(0.12, 0.14, 0.06, 12), concrete, 0.14, 0.055, 0.1);

  // Ops / equipment hut
  const hut = new THREE.Group();
  hut.position.set(-0.14, 0, -0.04);
  root.add(hut);
  add(hut, new THREE.BoxGeometry(0.36, 0.22, 0.3), olive, 0, 0.14, 0, 0, 0, 0);
  add(hut, new THREE.BoxGeometry(0.38, 0.03, 0.32), oliveDark, 0, 0.26, 0);
  add(hut, new THREE.BoxGeometry(0.4, 0.015, 0.34), oliveLight, 0, 0.275, 0, 0, 0, 0, {
    roughness: 0.8,
  });
  add(hut, new THREE.BoxGeometry(0.08, 0.12, 0.012), panel, 0.14, 0.12, 0.155, 0, 0, 0, {
    metalness: 0.35,
    roughness: 0.5,
  });
  for (const wx of [-0.1, 0.02]) {
    add(hut, new THREE.BoxGeometry(0.07, 0.05, 0.01), panel, wx, 0.16, 0.155, 0, 0, 0, {
      metalness: 0.45,
      roughness: 0.4,
    });
  }
  add(hut, new THREE.BoxGeometry(0.06, 0.05, 0.04), metalBright, -0.19, 0.14, 0.05, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.45,
  });
  add(hut, new THREE.BoxGeometry(0.05, 0.04, 0.08), metal, 0.1, 0.24, -0.08, 0, 0, 0, {
    metalness: 0.4,
    roughness: 0.55,
  });
  add(hut, new THREE.BoxGeometry(0.28, 0.025, 0.04), accent, 0, 0.255, 0.12, 0, 0, 0, {
    metalness: 0.2,
    roughness: 0.55,
  });
  add(hut, new THREE.BoxGeometry(0.1, 0.04, 0.06), oliveDark, 0.14, 0.04, 0.18, 0, 0, 0, {
    cast: false,
  });

  // Generator cart
  add(root, new THREE.BoxGeometry(0.14, 0.1, 0.12), oliveDark, -0.32, 0.07, 0.16);
  add(root, new THREE.CylinderGeometry(0.025, 0.025, 0.08, 8), metal, -0.32, 0.14, 0.16, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });

  // Sandbag ring
  for (let i = 0; i < 8; i++) {
    const a = (i / 8) * Math.PI * 2 + 0.2;
    if (a > 1.2 && a < 2.4) continue;
    add(
      root,
      new THREE.BoxGeometry(0.07, 0.035, 0.05),
      sand,
      Math.cos(a) * 0.38,
      0.04,
      Math.sin(a) * 0.32,
      0,
      -a,
      0,
      { roughness: 0.92, cast: false },
    );
  }

  // Lattice mast
  const mastX = 0.16;
  const mastZ = 0.1;
  add(root, new THREE.CylinderGeometry(0.032, 0.042, 0.52, 10), metalBright, mastX, 0.32, mastZ, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });
  for (const [lx, lz] of [
    [0.055, 0.055],
    [-0.055, 0.055],
    [0.055, -0.055],
    [-0.055, -0.055],
  ]) {
    add(root, new THREE.BoxGeometry(0.012, 0.48, 0.012), metal, mastX + lx, 0.3, mastZ + lz, 0, 0, 0, {
      metalness: 0.5,
      roughness: 0.45,
    });
  }
  for (let i = 0; i < 4; i++) {
    const y = 0.14 + i * 0.11;
    add(root, new THREE.BoxGeometry(0.11, 0.01, 0.01), metal, mastX, y, mastZ, 0, 0.4, 0, {
      metalness: 0.45,
      roughness: 0.5,
      cast: false,
    });
    add(root, new THREE.BoxGeometry(0.01, 0.01, 0.11), metal, mastX, y + 0.04, mastZ, 0, 0, 0, {
      metalness: 0.45,
      roughness: 0.5,
      cast: false,
    });
  }
  add(root, new THREE.BoxGeometry(0.22, 0.02, 0.025), metal, 0.02, 0.06, 0.02, 0, 0.35, 0, {
    metalness: 0.4,
    roughness: 0.6,
    cast: false,
  });

  // Turntable
  add(root, new THREE.CylinderGeometry(0.07, 0.08, 0.04, 14), metal, mastX, 0.58, mastZ, 0, 0, 0, {
    metalness: 0.6,
    roughness: 0.35,
  });
  add(root, new THREE.CylinderGeometry(0.05, 0.05, 0.03, 12), oliveDark, mastX, 0.61, mastZ, 0, 0, 0, {
    metalness: 0.4,
    roughness: 0.5,
  });

  // Rotating dish assembly
  const dishRoot = new THREE.Group();
  dishRoot.name = "radarDish";
  dishRoot.position.set(mastX, 0.64, mastZ);
  root.add(dishRoot);

  add(dishRoot, new THREE.BoxGeometry(0.2, 0.035, 0.06), metalBright, 0, 0.02, 0, 0, 0, 0, {
    metalness: 0.55,
    roughness: 0.4,
  });
  for (const sx of [-0.09, 0.09]) {
    add(dishRoot, new THREE.BoxGeometry(0.025, 0.12, 0.04), metal, sx, 0.08, -0.02, 0.15, 0, 0, {
      metalness: 0.5,
      roughness: 0.42,
    });
  }

  const elev = new THREE.Group();
  elev.position.set(0, 0.12, 0);
  elev.rotation.x = -0.48;
  dishRoot.add(elev);

  const bowl = new THREE.Mesh(
    new THREE.SphereGeometry(0.26, 20, 14, 0, Math.PI * 2, 0, Math.PI * 0.52),
    matStd(dish, { metalness: 0.4, roughness: 0.48 }),
  );
  bowl.rotation.x = Math.PI * 0.52;
  bowl.position.set(0, 0.02, 0.04);
  bowl.castShadow = true;
  bowl.receiveShadow = true;
  elev.add(bowl);

  const rim = new THREE.Mesh(
    new THREE.TorusGeometry(0.255, 0.012, 8, 28),
    matStd(dishDark, { metalness: 0.45, roughness: 0.5 }),
  );
  rim.rotation.x = Math.PI / 2;
  rim.position.set(0, 0.02, 0.12);
  rim.castShadow = true;
  elev.add(rim);

  for (let i = 0; i < 6; i++) {
    const a = (i / 6) * Math.PI;
    const rib = new THREE.Mesh(
      new THREE.BoxGeometry(0.008, 0.22, 0.006),
      matStd(metalBright, { metalness: 0.5, roughness: 0.4 }),
    );
    rib.position.set(Math.cos(a) * 0.08, 0.02, 0.05 + Math.sin(a) * 0.04);
    rib.rotation.set(0.35, a, Math.sin(a) * 0.4);
    rib.castShadow = false;
    elev.add(rib);
  }

  add(elev, new THREE.CylinderGeometry(0.01, 0.01, 0.22, 6), metal, 0, 0.02, 0.18, Math.PI / 2, 0, 0, {
    metalness: 0.65,
    roughness: 0.32,
  });
  add(elev, new THREE.ConeGeometry(0.035, 0.05, 8), dishDark, 0, 0.02, 0.3, Math.PI / 2, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });
  add(elev, new THREE.CylinderGeometry(0.018, 0.022, 0.03, 8), metalBright, 0, 0.02, 0.26, Math.PI / 2, 0, 0, {
    metalness: 0.55,
    roughness: 0.35,
  });
  add(elev, new THREE.BoxGeometry(0.1, 0.06, 0.08), oliveDark, 0, 0.0, -0.12, 0, 0, 0, {
    metalness: 0.35,
    roughness: 0.55,
  });

  // Secondary IFF antenna
  add(root, new THREE.CylinderGeometry(0.008, 0.008, 0.28, 6), metal, -0.22, 0.42, -0.12, 0.15, 0, 0.1, {
    metalness: 0.6,
    roughness: 0.35,
  });
  add(root, new THREE.BoxGeometry(0.06, 0.015, 0.015), metalBright, -0.22, 0.55, -0.12, 0, 0.4, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });
  add(root, new THREE.BoxGeometry(0.015, 0.015, 0.06), metalBright, -0.22, 0.55, -0.12, 0, 0, 0, {
    metalness: 0.5,
    roughness: 0.4,
  });

  add(root, new THREE.SphereGeometry(0.018, 8, 8), 0xaa3310, mastX, 0.9, mastZ, 0, 0, 0, {
    metalness: 0.3,
    roughness: 0.4,
    cast: false,
  });

  return root;
}

function setBuildingOpacity(root, opacity) {
  root.traverse((child) => {
    if (!child.isMesh || !child.material) return;
    // Don't ghost name sprites / cosmetic markers.
    if (child.userData?.skipBuildingOpacity) return;
    if (child.isSprite) return;
    const mats = Array.isArray(child.material) ? child.material : [child.material];
    for (const mat of mats) {
      if (!mat) continue;
      // Preserve MTL alpha for glass etc. when fully built.
      if (opacity >= 1) {
        const base =
          child.userData.baseOpacity != null ? child.userData.baseOpacity : 1;
        mat.opacity = base;
        mat.transparent = base < 1;
        mat.depthWrite = base >= 1;
      } else {
        if (child.userData.baseOpacity == null) {
          child.userData.baseOpacity = mat.opacity ?? 1;
        }
        mat.transparent = true;
        mat.opacity = opacity * (child.userData.baseOpacity ?? 1);
        mat.depthWrite = false;
      }
      mat.needsUpdate = true;
    }
  });
}

function rememberBaseOpacities(root) {
  root.traverse((child) => {
    if (!child.isMesh || !child.material) return;
    const mats = Array.isArray(child.material) ? child.material : [child.material];
    const mat = mats[0];
    if (mat && child.userData.baseOpacity == null) {
      child.userData.baseOpacity = mat.opacity ?? 1;
    }
  });
}

async function ensureBuildingModel() {
  buildingModelsReady = true;
  return true;
}

async function loadTerrainTexture(mapSize, seed = 1) {
  try {
    const loader = new THREE.TextureLoader();
    const base = await loader.loadAsync("/assets/terrain.jpg");
    return bakeBiomeTerrainTexture(base.image, mapSize, seed);
  } catch (error) {
    console.error(error);
    return bakeBiomeTerrainTexture(null, mapSize, seed);
  }
}

/** Deterministic 0..1 hash for biome noise. */
function hash2(x, y, seed) {
  let n = Math.imul(x | 0, 374761393) ^ Math.imul(y | 0, 668265263) ^ (seed | 0);
  n = Math.imul(n ^ (n >>> 13), 1274126177);
  return ((n ^ (n >>> 16)) >>> 0) / 4294967296;
}

function valueNoise2(x, y, seed) {
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const fx = x - x0;
  const fy = y - y0;
  const u = fx * fx * (3 - 2 * fx);
  const v = fy * fy * (3 - 2 * fy);
  const a = hash2(x0, y0, seed);
  const b = hash2(x0 + 1, y0, seed);
  const c = hash2(x0, y0 + 1, seed);
  const d = hash2(x0 + 1, y0 + 1, seed);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

function fbm2(x, y, seed, octaves = 4) {
  let amp = 0.5;
  let freq = 1;
  let sum = 0;
  let norm = 0;
  for (let i = 0; i < octaves; i++) {
    sum += amp * valueNoise2(x * freq, y * freq, seed + i * 101);
    norm += amp;
    amp *= 0.5;
    freq *= 2.05;
  }
  return sum / norm;
}

/** Biome field cached for decor placement: 0 desert · 0.5 scrub · 1 forest. */
let terrainBiomeField = null;
let terrainBiomeSize = 0;
let terrainSeed = 1;

function sampleBiome(wx, wz, mapSize) {
  if (!terrainBiomeField || !terrainBiomeSize) return 0.5;
  const u = Math.max(0, Math.min(0.999, wx / mapSize));
  const v = Math.max(0, Math.min(0.999, wz / mapSize));
  const x = Math.floor(u * terrainBiomeSize);
  const y = Math.floor(v * terrainBiomeSize);
  return terrainBiomeField[y * terrainBiomeSize + x];
}

function lerpColor(a, b, t) {
  return [
    a[0] + (b[0] - a[0]) * t,
    a[1] + (b[1] - a[1]) * t,
    a[2] + (b[2] - a[2]) * t,
  ];
}

/** Painted biome atlas: desert dunes, olive scrub, deep forest canopy — soft blends. */
function bakeBiomeTerrainTexture(image, mapSize, seed = 1) {
  const size = 1280;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  terrainSeed = seed | 0 || 1;
  terrainBiomeSize = 96;
  terrainBiomeField = new Float32Array(terrainBiomeSize * terrainBiomeSize);

  // Warm shared palette so biomes sit in one world, not clashing themes.
  const desert = [194, 158, 98];
  const desertDeep = [168, 128, 72];
  const scrub = [110, 118, 62];
  const scrubLight = [132, 128, 70];
  const forest = [52, 78, 42];
  const forestDeep = [38, 62, 34];
  const dust = [150, 132, 88];

  const scale = 2.4 + (terrainSeed % 7) * 0.12;
  const ox = (terrainSeed % 97) * 0.37;
  const oy = ((terrainSeed * 3) % 89) * 0.41;

  for (let by = 0; by < terrainBiomeSize; by++) {
    for (let bx = 0; bx < terrainBiomeSize; bx++) {
      const nx = bx / terrainBiomeSize;
      const ny = by / terrainBiomeSize;
      // Large continents + mid ridges — not salt-and-pepper noise.
      let n =
        fbm2(nx * scale + ox, ny * scale + oy, terrainSeed, 5) * 0.72 +
        fbm2(nx * scale * 0.35 + 20, ny * scale * 0.35 - 11, terrainSeed + 17, 3) * 0.28;
      // Gentle warp so borders feel organic.
      const warp = fbm2(nx * 1.6 + 40, ny * 1.6, terrainSeed + 33, 2);
      n = Math.max(0, Math.min(1, n + (warp - 0.5) * 0.12));
      terrainBiomeField[by * terrainBiomeSize + bx] = n;
    }
  }

  const img = ctx.createImageData(size, size);
  const px = img.data;
  const cell = size / terrainBiomeSize;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const bx = Math.min(terrainBiomeSize - 1, Math.floor(x / cell));
      const by = Math.min(terrainBiomeSize - 1, Math.floor(y / cell));
      // Bilinear biome for soft edges
      const fx = x / cell - bx;
      const fy = y / cell - by;
      const bx1 = Math.min(terrainBiomeSize - 1, bx + 1);
      const by1 = Math.min(terrainBiomeSize - 1, by + 1);
      const b00 = terrainBiomeField[by * terrainBiomeSize + bx];
      const b10 = terrainBiomeField[by * terrainBiomeSize + bx1];
      const b01 = terrainBiomeField[by1 * terrainBiomeSize + bx];
      const b11 = terrainBiomeField[by1 * terrainBiomeSize + bx1];
      const biome =
        b00 * (1 - fx) * (1 - fy) +
        b10 * fx * (1 - fy) +
        b01 * (1 - fx) * fy +
        b11 * fx * fy;

      const detail = fbm2(x * 0.035 + ox, y * 0.035 + oy, terrainSeed + 9, 3);
      const ridge = fbm2(x * 0.012 - oy, y * 0.012 + ox, terrainSeed + 21, 2);

      let col;
      if (biome < 0.38) {
        const t = biome / 0.38;
        col = lerpColor(desertDeep, desert, t * 0.65 + detail * 0.35);
        // Dune streaks
        const dune = Math.sin((x * 0.04 + y * 0.01) + ridge * 6) * 0.5 + 0.5;
        col = lerpColor(col, dust, dune * 0.18 * (1 - t));
      } else if (biome < 0.62) {
        const t = (biome - 0.38) / 0.24;
        col = lerpColor(desert, scrubLight, Math.min(1, t * 1.2));
        col = lerpColor(col, scrub, 0.35 + detail * 0.4);
      } else {
        const t = (biome - 0.62) / 0.38;
        col = lerpColor(scrub, forest, Math.min(1, t * 1.1));
        col = lerpColor(col, forestDeep, t * 0.45 + (1 - detail) * 0.2);
      }

      // Micro variation / soil grain
      const grain = (detail - 0.5) * 22;
      const o = (y * size + x) * 4;
      px[o] = Math.max(0, Math.min(255, col[0] + grain));
      px[o + 1] = Math.max(0, Math.min(255, col[1] + grain * 1.05));
      px[o + 2] = Math.max(0, Math.min(255, col[2] + grain * 0.75));
      px[o + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);

  // Soft photo texture wash — tinted so it doesn't fight biome colors.
  if (image) {
    ctx.save();
    ctx.globalCompositeOperation = "soft-light";
    for (let i = 0; i < 28; i++) {
      const x = Math.random() * size;
      const y = Math.random() * size;
      const bx = Math.min(terrainBiomeSize - 1, Math.floor((x / size) * terrainBiomeSize));
      const by = Math.min(terrainBiomeSize - 1, Math.floor((y / size) * terrainBiomeSize));
      const biome = terrainBiomeField[by * terrainBiomeSize + bx];
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(Math.random() * Math.PI * 2);
      ctx.globalAlpha = 0.14 + Math.random() * 0.22;
      if (biome < 0.4) {
        ctx.filter = "hue-rotate(-12deg) saturate(0.85) brightness(1.15)";
      } else if (biome > 0.65) {
        ctx.filter = "hue-rotate(28deg) saturate(1.1) brightness(0.88)";
      } else {
        ctx.filter = "hue-rotate(8deg) saturate(0.95) brightness(1.0)";
      }
      const w = 120 + Math.random() * 280;
      const h = 100 + Math.random() * 240;
      ctx.drawImage(image, -w / 2, -h / 2, w, h);
      ctx.restore();
    }
    ctx.restore();
  }

  // Soft vignette at map feel — slight cooler edges
  const vig = ctx.createRadialGradient(
    size * 0.5,
    size * 0.5,
    size * 0.25,
    size * 0.5,
    size * 0.5,
    size * 0.72,
  );
  vig.addColorStop(0, "rgba(0,0,0,0)");
  vig.addColorStop(1, "rgba(20,28,14,0.22)");
  ctx.fillStyle = vig;
  ctx.fillRect(0, 0, size, size);

  const tex = new THREE.CanvasTexture(canvas);
  tex.wrapS = THREE.ClampToEdgeWrapping;
  tex.wrapT = THREE.ClampToEdgeWrapping;
  tex.anisotropy = 8;
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.needsUpdate = true;
  return tex;
}

function makeFallbackTerrainTexture(mapSize) {
  return bakeBiomeTerrainTexture(null, mapSize, terrainSeed);
}

function visionRadiusFor(entity) {
  if (!entity || (entity.hp != null && entity.hp <= 0)) return 0;
  const kind = String(entity.kind || "");
  if (kind === "hq") return 20;
  if (kind === "radar") {
    // Under construction: short local vision; finished dish lights a huge sector.
    const building = entity.progress != null && entity.progress < 1;
    return building ? 6 : 38;
  }
  if (entity.building) return 13;
  if (entity.unit) return 11;
  return 0;
}

function unpackExploredBytes(bytes, size) {
  const cells = size * size;
  const out = new Uint8Array(cells);
  if (!bytes || !bytes.length) return out;
  // Server sends little-endian u64 words.
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  for (let i = 0; i < cells; i++) {
    const word = Math.floor(i / 64);
    const bit = i % 64;
    const byteIndex = word * 8 + Math.floor(bit / 8);
    const bitInByte = bit % 8;
    if (byteIndex >= view.length) break;
    if ((view[byteIndex] >> bitInByte) & 1) out[i] = 255;
  }
  return out;
}

function applyExploredNew(indices) {
  if (!fogExploredData || !indices?.length) return;
  for (const idx of indices) {
    if (idx >= 0 && idx < fogExploredData.length) fogExploredData[idx] = 255;
  }
}

function stampVisionCircle(data, size, cx, cy, radius) {
  const r = Math.ceil(radius);
  const ix = Math.floor(cx);
  const iy = Math.floor(cy);
  const r2 = radius * radius;
  for (let dy = -r; dy <= r; dy++) {
    for (let dx = -r; dx <= r; dx++) {
      const x = ix + dx;
      const y = iy + dy;
      if (x < 0 || y < 0 || x >= size || y >= size) continue;
      const fx = x + 0.5 - cx;
      const fy = y + 0.5 - cy;
      if (fx * fx + fy * fy > r2) continue;
      data[y * size + x] = 255;
    }
  }
}

let lastVisionAt = 0;

function startVisionLoop() {
  if (startVisionLoop.timer) return;
  startVisionLoop.timer = setInterval(() => {
    if (!state.match || $("#match-screen")?.hidden) return;
    refreshLiveVision();
  }, 320);
}

function refreshLiveVision() {
  if (!fogVisionData || !fogExploredData || !fogDataTexture) return;
  const now = performance.now();
  if (now - lastVisionAt < 280) return;
  lastVisionAt = now;

  const tex = fogDataTexture.image?.data;
  if (!tex) return;
  const cells = mapSize * mapSize;

  if (globalVision) {
    fogVisionData.fill(255);
    fogExploredData.fill(255);
    for (let i = 0; i < cells; i++) {
      const o = i * 4;
      tex[o] = 255;
      tex[o + 1] = 255;
      tex[o + 2] = 0;
      tex[o + 3] = 255;
    }
    fogDataTexture.needsUpdate = true;
    return;
  }

  fogVisionData.fill(0);
  const you = state.match?.you;
  const myTeam = state.match?.team;
  const shareAllies = state.match && !state.match.ffa;
  const stamped = [];
  for (const entity of state.entities.values()) {
    const mine = entity.owner === you;
    const ally = shareAllies && Number(entity.team) === Number(myTeam);
    if (!mine && !ally) continue;
    const radius = visionRadiusFor(entity);
    if (!radius) continue;
    // Infantry blobs overlap — one stamp covers a squad and saves ~50× circle fills.
    if (entity.unit && !entity.building) {
      let covered = false;
      for (let i = 0; i < stamped.length; i++) {
        const s = stamped[i];
        const dx = entity.x - s[0];
        const dy = entity.y - s[1];
        if (dx * dx + dy * dy < 16) {
          covered = true;
          break;
        }
      }
      if (covered) continue;
      stamped.push([entity.x, entity.y]);
    }
    stampVisionCircle(fogVisionData, mapSize, entity.x, entity.y, radius);
    stampVisionCircle(fogExploredData, mapSize, entity.x, entity.y, radius);
  }

  // Pack into RGBA texture: R=explored, G=visible
  for (let i = 0; i < cells; i++) {
    const o = i * 4;
    tex[o] = fogExploredData[i];
    tex[o + 1] = fogVisionData[i];
    tex[o + 2] = 0;
    tex[o + 3] = 255;
  }
  fogDataTexture.needsUpdate = true;
}

function createFogOfWar(size) {
  fogExploredData = new Uint8Array(size * size);
  fogVisionData = new Uint8Array(size * size);
  const rgba = new Uint8Array(size * size * 4);
  fogDataTexture = new THREE.DataTexture(rgba, size, size, THREE.RGBAFormat);
  fogDataTexture.magFilter = THREE.NearestFilter;
  fogDataTexture.minFilter = THREE.NearestFilter;
  fogDataTexture.flipY = false;
  fogDataTexture.needsUpdate = true;

  const geo = new THREE.PlaneGeometry(size, size, 1, 1);
  // Three r170 + WebGL2 uses GLSL3 — avoid texture2D/gl_FragColor and reserved `sample`.
  const mat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    glslVersion: THREE.GLSL3,
    uniforms: {
      uMap: { value: fogDataTexture },
      uSize: { value: size },
    },
    vertexShader: `
      out vec3 vWorldPos;
      void main() {
        vec4 world = modelMatrix * vec4(position, 1.0);
        vWorldPos = world.xyz;
        gl_Position = projectionMatrix * viewMatrix * world;
      }
    `,
    fragmentShader: `
      uniform sampler2D uMap;
      uniform float uSize;
      in vec3 vWorldPos;
      out vec4 fragColor;
      void main() {
        vec2 uv = vec2(vWorldPos.x, vWorldPos.z) / uSize;
        if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
          fragColor = vec4(0.02, 0.03, 0.02, 0.92);
          return;
        }
        vec4 texel = texture(uMap, uv);
        float explored = texel.r;
        float visible = texel.g;
        if (explored < 0.5) {
          fragColor = vec4(0.02, 0.03, 0.02, 0.92);
          return;
        }
        if (visible < 0.5) {
          fragColor = vec4(0.05, 0.07, 0.04, 0.62);
          return;
        }
        discard;
      }
    `,
  });
  const mesh = new THREE.Mesh(geo, mat);
  mesh.rotation.x = -Math.PI / 2;
  mesh.position.set(size / 2, 0.2, size / 2);
  mesh.renderOrder = 8;
  mesh.name = "fogOfWar";
  return mesh;
}

function loadExploredFromSnapshot(snapshot) {
  const size = snapshot.map_size || mapSize;
  let bytes = snapshot.explored;
  if (typeof bytes === "string") {
    // unlikely; ignore
    bytes = [];
  }
  fogExploredData = unpackExploredBytes(bytes, size);
  if (fogDataTexture) fogDataTexture.needsUpdate = true;
  refreshLiveVision();
}

function scatterGroundDecor(scene, size) {
  const group = new THREE.Group();
  group.name = "groundDecor";

  const desertRock = new THREE.MeshStandardMaterial({
    color: 0x8a7a5a,
    roughness: 0.96,
    metalness: 0.04,
    flatShading: true,
  });
  const scrubBush = new THREE.MeshStandardMaterial({
    color: 0x5a6a38,
    roughness: 0.92,
    metalness: 0,
    flatShading: true,
  });
  const forestTrunk = new THREE.MeshStandardMaterial({
    color: 0x4a3a28,
    roughness: 0.95,
    metalness: 0,
  });
  const forestCanopy = new THREE.MeshStandardMaterial({
    color: 0x2f4a28,
    roughness: 0.88,
    metalness: 0,
    flatShading: true,
  });
  const forestCanopyDeep = new THREE.MeshStandardMaterial({
    color: 0x243a20,
    roughness: 0.9,
    metalness: 0,
    flatShading: true,
  });

  const maxTrees = Math.min(70, Math.floor(size * 0.28));
  const maxRocks = Math.min(40, Math.floor(size * 0.18));
  const maxBushes = Math.min(50, Math.floor(size * 0.22));
  const tries = Math.min(280, Math.floor(size * 0.9));
  let trees = 0;
  let rocks = 0;
  let bushes = 0;
  for (let i = 0; i < tries; i++) {
    const x = 5 + Math.random() * (size - 10);
    const z = 5 + Math.random() * (size - 10);
    const biome = sampleBiome(x, z, size);

    if (biome > 0.68 && trees < maxTrees) {
      // Simple pine / canopy tree
      const h = 0.55 + Math.random() * 0.85;
      const tree = new THREE.Group();
      const trunk = new THREE.Mesh(
        new THREE.CylinderGeometry(0.04, 0.07, h * 0.55, 5),
        forestTrunk,
      );
      trunk.position.y = h * 0.22;
      tree.add(trunk);
      const canopy = new THREE.Mesh(
        new THREE.ConeGeometry(0.28 + Math.random() * 0.22, h * 0.85, 6),
        Math.random() > 0.45 ? forestCanopy : forestCanopyDeep,
      );
      canopy.position.y = h * 0.7;
      tree.add(canopy);
      if (Math.random() > 0.55) {
        const mid = new THREE.Mesh(
          new THREE.ConeGeometry(0.2 + Math.random() * 0.12, h * 0.45, 6),
          forestCanopyDeep,
        );
        mid.position.y = h * 0.95;
        tree.add(mid);
      }
      tree.position.set(x, 0, z);
      tree.rotation.y = Math.random() * Math.PI * 2;
      group.add(tree);
      trees += 1;
      continue;
    }

    if (biome < 0.36 && rocks < maxRocks) {
      const s = 0.18 + Math.random() * 0.5;
      const rock = new THREE.Mesh(new THREE.DodecahedronGeometry(s, 0), desertRock);
      rock.position.set(x, s * 0.28, z);
      rock.rotation.set(Math.random(), Math.random(), Math.random());
      rock.scale.set(1 + Math.random() * 0.4, 0.55 + Math.random() * 0.35, 1 + Math.random() * 0.35);
      group.add(rock);
      rocks += 1;
      continue;
    }

    if (biome >= 0.36 && biome <= 0.72 && bushes < maxBushes) {
      const s = 0.22 + Math.random() * 0.35;
      const bush = new THREE.Mesh(new THREE.IcosahedronGeometry(s, 0), scrubBush);
      bush.position.set(x, s * 0.35, z);
      bush.scale.set(1.2, 0.55 + Math.random() * 0.35, 1.1);
      bush.rotation.y = Math.random() * Math.PI;
      group.add(bush);
      bushes += 1;
    }
  }

  scene.add(group);
}

function initThree(size, terrainTexture, home) {
  mapSize = size;
  const canvas = $("#viewport");
  ghostMesh = null;
  fogOfWar = null;

  if (renderer) {
    // Re-entering a match: dispose previous GL context lightly by clearing scene refs.
    controls?.dispose();
  }

  renderer = new THREE.WebGLRenderer({ canvas, antialias: false, powerPreference: "high-performance" });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 1.25));
  renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);

  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1a2218);
  // Warm distant haze that sits between desert sand and forest green.
  scene.fog = new THREE.Fog(0x2a3224, Math.max(80, aoiRadius * 2.4), Math.max(140, aoiRadius * 5));

  camera = new THREE.PerspectiveCamera(
    CAMERA_FOV,
    canvas.clientWidth / canvas.clientHeight,
    0.1,
    Math.max(800, size * 4),
  );

  const lookX = home?.x ?? size / 2;
  const lookZ = home?.z ?? size / 2;
  const cx = size / 2;
  const cz = size / 2;
  const dist = CAMERA_DIST;
  camera.position.set(
    lookX,
    Math.sin(CAMERA_PITCH) * dist,
    lookZ + Math.cos(CAMERA_PITCH) * dist,
  );

  controls = new OrbitControls(camera, canvas);
  controls.target.set(lookX, 0, lookZ);
  // Fixed Generals angle: no free rotate.
  controls.enableRotate = false;
  controls.enablePan = false;
  controls.enableZoom = true;
  controls.minDistance = CAMERA_DIST_MIN;
  controls.maxDistance = CAMERA_DIST_MAX;
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.zoomSpeed = 0.55;
  controls.minPolarAngle = CAMERA_PITCH;
  controls.maxPolarAngle = CAMERA_PITCH;
  controls.update();

  const hemi = new THREE.HemisphereLight(0xe8d8b0, 0x2a3018, 1.05);
  scene.add(hemi);
  const sun = new THREE.DirectionalLight(0xffe2b8, 1.05);
  sun.position.set(55, 70, 28);
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0xa8c090, 0.22);
  fill.position.set(-30, 25, -40);
  scene.add(fill);

  const geo = new THREE.PlaneGeometry(size, size, 1, 1);
  const mat = new THREE.MeshStandardMaterial({
    map: terrainTexture || null,
    color: terrainTexture ? 0xffffff : 0x5a6840,
    roughness: 0.94,
    metalness: 0.02,
    flatShading: false,
  });
  ground = new THREE.Mesh(geo, mat);
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(cx, 0, cz);
  ground.receiveShadow = true;
  scene.add(ground);

  // Very faint placement hint — not a loud square grid over the map.
  const grid = new THREE.GridHelper(
    size,
    Math.min(size, 64),
    0x000000,
    0x3a4030,
  );
  grid.material.opacity = 0.05;
  grid.material.transparent = true;
  grid.position.set(cx, 0.02, cz);
  scene.add(grid);

  scatterGroundDecor(scene, size);

  fogOfWar = createFogOfWar(size);
  scene.add(fogOfWar);

  // Dark underlay past the playable map edge.
  const underlay = new THREE.Mesh(
    new THREE.PlaneGeometry(size + 48, size + 48),
    new THREE.MeshStandardMaterial({
      color: 0x141810,
      roughness: 1,
      metalness: 0,
    }),
  );
  underlay.rotation.x = -Math.PI / 2;
  underlay.position.set(cx, -0.04, cz);
  scene.add(underlay);

  raycaster = new THREE.Raycaster();
  pointer = new THREE.Vector2();
  state.meshes.clear();

  window.addEventListener("resize", onResize);
  canvas.addEventListener("pointerdown", (e) => {
    Sfx.ensure();
    onPointerDown(e);
  });
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  if (!onEdgePointerMove._bound) {
    window.addEventListener("pointermove", onEdgePointerMove);
    onEdgePointerMove._bound = true;
  }
  if (!animate.running) {
    animate.running = true;
    animate();
  }
}

function onEdgePointerMove(event) {
  edgeMouse.x = event.clientX;
  edgeMouse.y = event.clientY;
  edgeMouse.w = window.innerWidth;
  edgeMouse.h = window.innerHeight;
  edgeMouse.inside = Boolean(state.match) && !$("#match-screen")?.hidden;
  edgeMouse.overUi = Boolean(
    event.target?.closest?.("#radar, .build-rail, .unit-rail"),
  );
  updateGhostPreview(event);
}

function myColors() {
  for (const entity of state.entities.values()) {
    if (entity.owner === state.match?.you && Array.isArray(entity.colors)) {
      return entityColors(entity);
    }
  }
  return [0x88aa66, 0xf5f5f5, 0x1565c0];
}

function clearGhost() {
  if (!ghostMesh || !scene) return;
  scene.remove(ghostMesh);
  ghostMesh.traverse((obj) => {
    if (!obj.isMesh) return;
    obj.geometry?.dispose?.();
    if (ghostMesh.userData.disposeMaterials && obj.material) {
      if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose?.());
      else obj.material.dispose?.();
    }
  });
  ghostMesh = null;
}

function ensureGhost(kind) {
  if (!scene) return null;
  if (ghostMesh?.userData.kind === kind) return ghostMesh;
  clearGhost();

  const colors = myColors();
  const mat = new THREE.MeshStandardMaterial({
    color: colors[0],
    transparent: true,
    opacity: 0.38,
    depthWrite: false,
    metalness: 0.05,
    roughness: 0.8,
  });

  const mesh = createBuildingMesh(kind, mat);
  setBuildingOpacity(mesh, 0.38);
  // Materials were detached for this ghost — safe to dispose on clear.
  mesh.userData.disposeMaterials = true;

  // Soft tile footprint so placement cell is obvious.
  const pad = new THREE.Mesh(
    new THREE.PlaneGeometry(0.95, 0.95),
    new THREE.MeshBasicMaterial({
      color: colors[1],
      transparent: true,
      opacity: 0.35,
      depthWrite: false,
    }),
  );
  pad.rotation.x = -Math.PI / 2;
  pad.position.y = 0.04;
  mesh.add(pad);

  mesh.userData.kind = kind;
  mesh.userData.isGhost = true;
  mesh.visible = false;
  scene.add(mesh);
  ghostMesh = mesh;
  return mesh;
}

function buildingRadius(kind) {
  // Half-footprint from BUILDING_VISUAL (must match server building_radius).
  const visual = BUILDING_VISUAL[kind] ?? 1.35;
  return visual * 0.42;
}

function enemyBuildBlockRadius(kind) {
  const k = String(kind || "");
  if (k === "hq") return 14;
  if (k === "war_factory" || k === "barracks") return 10;
  return 8;
}

function canPlaceBuildingAt(kind, tileX, tileY) {
  const fx = tileX + 0.5;
  const fy = tileY + 0.5;
  const placeR = buildingRadius(kind);
  const myTeam = state.match?.team;
  for (const entity of state.entities.values()) {
    if (!entity.building) continue;
    const dx = entity.x - fx;
    const dy = entity.y - fy;
    const otherR = buildingRadius(entity.kind);
    const minDist = placeR + otherR + 0.04;
    if (dx * dx + dy * dy < minDist * minDist) return false;
    if (
      myTeam != null &&
      entity.team !== myTeam &&
      (entity.hp ?? 1) > 0
    ) {
      const block = enemyBuildBlockRadius(entity.kind) + placeR;
      if (dx * dx + dy * dy < block * block) return false;
    }
  }
  return true;
}

function tintGhost(valid) {
  if (!ghostMesh) return;
  const pad = ghostMesh.children.find((c) => c.isMesh && c.geometry?.type === "PlaneGeometry");
  if (pad?.material) {
    pad.material.color.setHex(valid ? 0x7cfc00 : 0xff3333);
  }
  setBuildingOpacity(ghostMesh, valid ? 0.42 : 0.28);
}

function updateGhostPreview(event) {
  if (!state.selectedBuild || !ground || !camera) {
    if (ghostMesh) ghostMesh.visible = false;
    return;
  }
  const ghost = ensureGhost(state.selectedBuild);
  if (!ghost) return;
  const point = worldFromEvent(event);
  if (!point) {
    ghost.visible = false;
    return;
  }
  const tileX = Math.floor(point.x);
  const tileZ = Math.floor(point.z);
  const x = tileX + 0.5;
  const z = tileZ + 0.5;
  ghost.position.set(x, 0, z);
  ghost.visible = true;
  const valid = canPlaceBuildingAt(state.selectedBuild, tileX, tileZ);
  ghost.userData.placeValid = valid;
  tintGhost(valid);
}

function setBuildPlacement(kind) {
  state.selectedBuild = kind;
  document.querySelectorAll(".build-item").forEach((el) => {
    el.classList.toggle("on", el.dataset.kind === kind);
  });
  if (kind) {
    ensureGhost(kind);
    $("#build-detail").textContent =
      `Placing ${kind} — drag to preview, click to build (Esc cancel)`;
  } else {
    clearGhost();
    $("#build-detail").textContent = "Select a building to place";
    document.querySelectorAll(".build-item").forEach((el) => el.classList.remove("on"));
  }
}

function applyEdgePan() {
  if (!controls || !camera || !edgeMouse.inside) return;
  // Top resource bar is see-through — still pan when the cursor is along the screen edge.
  const topStrip = edgeMouse.y < 64;
  if (edgeMouse.overUi && !topStrip) return;

  let dx = 0;
  let dz = 0;
  const e = EDGE_SCROLL_PX;
  const edgeSpeed = 1.05 * (controls.getDistance() / 26);

  if (edgeMouse.x < e) {
    dx -= edgeSpeed * (1 - edgeMouse.x / e);
  } else if (edgeMouse.x > edgeMouse.w - e) {
    dx += edgeSpeed * (1 - (edgeMouse.w - edgeMouse.x) / e);
  }
  // Leave bottom HUD mostly alone — prefer side/top edge scroll.
  const bottomDead = 150;
  if (edgeMouse.y < e) {
    dz -= edgeSpeed * (1 - edgeMouse.y / e);
  } else if (edgeMouse.y > edgeMouse.h - e && edgeMouse.y > edgeMouse.h - bottomDead) {
    // only scroll down when truly near bottom edge strip
    const edgeY = edgeMouse.h - edgeMouse.y;
    if (edgeY < 56) {
      dz += edgeSpeed * (1 - edgeY / 56);
    }
  }

  if (!dx && !dz) return;

  const offset = new THREE.Vector3().subVectors(camera.position, controls.target);
  const margin = 4;
  controls.target.x = Math.max(margin, Math.min(mapSize - margin, controls.target.x + dx));
  controls.target.z = Math.max(margin, Math.min(mapSize - margin, controls.target.z + dz));
  camera.position.copy(controls.target).add(offset);
}

function onResize() {
  const canvas = $("#viewport");
  if (!renderer || !camera) return;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  renderer.setSize(w, h, false);
}

function worldFromEvent(event) {
  const canvas = $("#viewport");
  const rect = canvas.getBoundingClientRect();
  pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  raycaster.setFromCamera(pointer, camera);
  const hits = raycaster.intersectObject(ground);
  if (!hits.length) return null;
  return hits[0].point;
}

const boxSelect = {
  active: false,
  startX: 0,
  startY: 0,
  curX: 0,
  curY: 0,
  additive: false,
};

function setSelectBoxEl(x0, y0, x1, y1, show) {
  const el = $("#select-box");
  if (!el) return;
  if (!show) {
    el.hidden = true;
    return;
  }
  const left = Math.min(x0, x1);
  const top = Math.min(y0, y1);
  const w = Math.abs(x1 - x0);
  const h = Math.abs(y1 - y0);
  el.hidden = false;
  el.style.left = `${left}px`;
  el.style.top = `${top}px`;
  el.style.width = `${w}px`;
  el.style.height = `${h}px`;
}

function worldToClient(x, y, z = 0.08) {
  const canvas = $("#viewport");
  if (!canvas || !camera) return null;
  const rect = canvas.getBoundingClientRect();
  const v = new THREE.Vector3(x, z, y);
  v.project(camera);
  if (v.z > 1) return null;
  return {
    x: (v.x * 0.5 + 0.5) * rect.width + rect.left,
    y: (-v.y * 0.5 + 0.5) * rect.height + rect.top,
  };
}

function unitsInScreenBox(x0, y0, x1, y1) {
  const left = Math.min(x0, x1);
  const right = Math.max(x0, x1);
  const top = Math.min(y0, y1);
  const bottom = Math.max(y0, y1);
  const you = state.match?.you;
  const ids = [];
  for (const entity of state.entities.values()) {
    if (!entity.unit || entity.owner !== you) continue;
    const p = worldToClient(entity.x, entity.y, 0.12);
    if (!p) continue;
    if (p.x >= left && p.x <= right && p.y >= top && p.y <= bottom) {
      ids.push(entity.id);
    }
  }
  return ids;
}

function clearUnitSelection() {
  for (const id of state.selectedUnits) Sfx.stopEngine(id);
  state.selectedUnits = [];
}

function setSelectedUnits(ids, toastMsg) {
  const next = new Set(ids);
  for (const id of state.selectedUnits) {
    if (!next.has(id)) Sfx.stopEngine(id);
  }
  state.selectedUnits = ids;
  state.selectedBuilding = null;
  refreshTrainablePanel();
  syncSelectionMarkers();
  if (toastMsg) toast(toastMsg);
}

function syncSelectionMarkers() {
  const selected = new Set(state.selectedUnits);
  for (const [id, mesh] of state.meshes.entries()) {
    const on = selected.has(id);
    let ring = mesh.userData.selRing;
    if (on && !ring) {
      const tank = !!mesh.userData.isTank;
      const building = !!mesh.userData.building;
      const inner = building ? 0.55 : tank ? 0.08 : 0.035;
      const outer = building ? 0.68 : tank ? 0.1 : 0.05;
      const geo = new THREE.RingGeometry(inner, outer, 20);
      const mat = new THREE.MeshBasicMaterial({
        color: 0x9fef4a,
        transparent: true,
        opacity: 0.85,
        side: THREE.DoubleSide,
        depthWrite: false,
      });
      ring = new THREE.Mesh(geo, mat);
      ring.rotation.x = -Math.PI / 2;
      ring.position.y = 0.03;
      ring.name = "selRing";
      mesh.add(ring);
      mesh.userData.selRing = ring;
    } else if (!on && ring) {
      mesh.remove(ring);
      ring.geometry.dispose();
      ring.material.dispose();
      mesh.userData.selRing = null;
    }
  }
}

function pickOwnAtPoint(point) {
  let best = null;
  let bestDist = 1.4;
  for (const entity of state.entities.values()) {
    if (entity.owner !== state.match?.you) continue;
    const dx = entity.x - point.x;
    const dy = entity.y - point.z;
    const d = Math.hypot(dx, dy);
    if (d < bestDist) {
      best = entity;
      bestDist = d;
    }
  }
  return best;
}

function finishBoxSelect(event) {
  if (!boxSelect.active) return;
  boxSelect.active = false;
  setSelectBoxEl(0, 0, 0, 0, false);
  window.removeEventListener("pointermove", onBoxSelectMove);
  window.removeEventListener("pointerup", onBoxSelectUp);
  window.removeEventListener("pointercancel", onBoxSelectUp);

  const dx = boxSelect.curX - boxSelect.startX;
  const dy = boxSelect.curY - boxSelect.startY;
  const dragDist = Math.hypot(dx, dy);

  // Small drag = click select
  if (dragDist < 6) {
    const point = worldFromEvent(event);
    if (!point) return;
    const best = pickOwnAtPoint(point);
    if (best?.unit) {
      if (boxSelect.additive) {
        const set = new Set(state.selectedUnits);
        if (set.has(best.id)) set.delete(best.id);
        else set.add(best.id);
        setSelectedUnits([...set], `${set.size} selected`);
      } else {
        setSelectedUnits([best.id], `Selected ${best.kind}`);
      }
    } else if (best?.building) {
      state.selectedBuilding = best.id;
      clearUnitSelection();
      syncSelectionMarkers();
      refreshTrainablePanel();
      toast(`Selected ${best.kind}`);
    } else if (!boxSelect.additive) {
      clearUnitSelection();
      state.selectedBuilding = null;
      syncSelectionMarkers();
      refreshTrainablePanel();
      send({ t: "set_focus", x: point.x, y: point.z });
    }
    return;
  }

  const boxed = unitsInScreenBox(
    boxSelect.startX,
    boxSelect.startY,
    boxSelect.curX,
    boxSelect.curY,
  );
  if (boxSelect.additive) {
    const set = new Set(state.selectedUnits);
    for (const id of boxed) set.add(id);
    setSelectedUnits([...set], `${set.size} selected`);
  } else if (boxed.length) {
    setSelectedUnits(boxed, `${boxed.length} units selected`);
  } else {
    setSelectedUnits([], "Nothing selected");
  }
}

function onBoxSelectMove(event) {
  if (!boxSelect.active) return;
  boxSelect.curX = event.clientX;
  boxSelect.curY = event.clientY;
  setSelectBoxEl(
    boxSelect.startX,
    boxSelect.startY,
    boxSelect.curX,
    boxSelect.curY,
    true,
  );
}

function onBoxSelectUp(event) {
  if (event.button !== 0 && event.type === "pointerup") return;
  finishBoxSelect(event);
}

function enemyUnderPointer(event, groundPoint) {
  const youTeam = state.match?.team;
  if (raycaster && camera && state.meshes.size) {
    const canvas = $("#viewport");
    const rect = canvas.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const roots = [];
    for (const mesh of state.meshes.values()) {
      const ent = state.entities.get(mesh.userData.id);
      if (!ent || ent.team === youTeam) continue;
      if (ent.hp != null && ent.hp <= 0) continue;
      roots.push(mesh);
    }
    const hits = raycaster.intersectObjects(roots, true);
    for (const hit of hits) {
      let obj = hit.object;
      while (obj) {
        const id = obj.userData?.id;
        if (id && state.entities.has(id)) {
          const ent = state.entities.get(id);
          if (ent && ent.team !== youTeam) return ent;
        }
        obj = obj.parent;
      }
    }
  }
  if (!groundPoint) return null;
  let enemy = null;
  let best = Infinity;
  for (const entity of state.entities.values()) {
    if (entity.team === youTeam) continue;
    if (entity.hp != null && entity.hp <= 0) continue;
    const reach = entity.building
      ? 0.55
      : String(entity.kind || "").includes("tank") ||
          String(entity.kind || "").includes("mlrs")
        ? 0.28
        : 0.12;
    const d = Math.hypot(entity.x - groundPoint.x, entity.y - groundPoint.z);
    if (d <= reach && d < best) {
      enemy = entity;
      best = d;
    }
  }
  return enemy;
}

function onPointerDown(event) {
  void enterGameFullscreen();

  if (event.button === 2) {
    event.preventDefault();
    boxSelect.active = false;
    setSelectBoxEl(0, 0, 0, 0, false);
    const point = worldFromEvent(event);
    if (!point || !state.selectedUnits.length) return;
    const enemy = enemyUnderPointer(event, point);
    if (enemy) {
      send({ t: "attack", ids: state.selectedUnits, target_id: enemy.id });
      toast(`Attacking ${enemy.kind} (${state.selectedUnits.length})`);
    } else {
      send({ t: "move_units", ids: state.selectedUnits, x: point.x, y: point.z });
      toast(`Moving ${state.selectedUnits.length}`);
    }
    return;
  }

  if (event.button !== 0) return;

  if (state.selectedBuild) {
    const point = worldFromEvent(event);
    if (!point) return;
    const x = Math.floor(point.x);
    const y = Math.floor(point.z);
    if (!canPlaceBuildingAt(state.selectedBuild, x, y)) {
      toast("Buraya bina kurulamaz — yer dolu veya düşman bölgesi");
      return;
    }
    send({
      t: "place_building",
      kind: state.selectedBuild,
      x,
      y,
    });
    setBuildPlacement(null);
    return;
  }

  // Start Generals-style drag box (Shift adds to selection).
  boxSelect.active = true;
  boxSelect.startX = event.clientX;
  boxSelect.startY = event.clientY;
  boxSelect.curX = event.clientX;
  boxSelect.curY = event.clientY;
  boxSelect.additive = event.shiftKey;
  setSelectBoxEl(0, 0, 0, 0, false);
  window.addEventListener("pointermove", onBoxSelectMove);
  window.addEventListener("pointerup", onBoxSelectUp);
  window.addEventListener("pointercancel", onBoxSelectUp);
}

function entityColors(entity) {
  const fallback = [0x888888, 0x555555, 0x333333];
  const c = entity.colors;
  if (!Array.isArray(c) || c.length < 3) return fallback;
  return [c[0] >>> 0, c[1] >>> 0, c[2] >>> 0];
}

function makeNameSprite(text, colors) {
  const canvas = document.createElement("canvas");
  canvas.width = 192;
  canvas.height = 48;
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);

  // Compact tricolor strip above the name
  const barW = 54;
  const barH = 3;
  const barX = (canvas.width - barW) / 2;
  const bandW = barW / 3;
  for (let i = 0; i < 3; i++) {
    ctx.fillStyle = `#${(colors[i] >>> 0).toString(16).padStart(6, "0")}`;
    ctx.fillRect(barX + i * bandW, 4, bandW, barH);
  }

  ctx.font = "bold 16px Segoe UI, Tahoma, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineWidth = 3;
  ctx.strokeStyle = "rgba(0,0,0,0.85)";
  ctx.fillStyle = "#f4f1e8";
  const label = String(text || "?").slice(0, 16);
  ctx.strokeText(label, canvas.width / 2, 28);
  ctx.fillText(label, canvas.width / 2, 28);

  const texture = new THREE.CanvasTexture(canvas);
  texture.needsUpdate = true;
  const mat = new THREE.SpriteMaterial({
    map: texture,
    transparent: true,
    depthTest: false,
  });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(1.85, 0.46, 1);
  sprite.center.set(0.5, 0);
  return sprite;
}

function attachOwnerMarkings(mesh, entity) {
  const colors = entityColors(entity);
  const name = entity.owner_name || "Player";
  const sprite = makeNameSprite(name, colors);
  if (!entity.building) {
    const tank = String(entity.kind || "").includes("tank");
    sprite.scale.set(tank ? 0.32 : 0.28, tank ? 0.085 : 0.07, 1);
  }
  sprite.position.set(0, labelHeightFor(entity), 0);
  sprite.name = "ownerLabel";
  mesh.add(sprite);
  mesh.userData.ownerLabel = sprite;
  mesh.userData.labelKey = `${name}|${colors.join(",")}`;
}

function labelHeightFor(entity) {
  if (entity.building) {
    if (entity.kind === "hq") return 2.05;
    if (entity.kind === "bunker") return 0.42;
    if (entity.kind === "radar") return 1.05;
    if (entity.kind === "turret") return 0.85;
    return 1.5;
  }
  const h = unitDims(entity.kind).h || 0.08;
  return h + (String(entity.kind || "").includes("tank") || String(entity.kind || "").includes("mlrs") ? 0.1 : 0.05);
}

function activeLoadProgress(entity) {
  if (entity.progress != null && entity.progress < 1) {
    return { pct: entity.progress, label: "BUILD" };
  }
  if (entity.train_progress != null && entity.train_progress < 1) {
    return { pct: entity.train_progress, label: "TRAIN" };
  }
  return null;
}

function makeProgressSprite(pct, label, compact = false) {
  const percent = Math.max(0, Math.min(100, Math.round(pct * 100)));
  const scale = 4; // hi-res canvas so zoom stays sharp
  const canvas = document.createElement("canvas");
  canvas.width = 160 * scale;
  canvas.height = 36 * scale;
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.scale(scale, scale);
  ctx.imageSmoothingEnabled = true;

  const barX = 18;
  const barY = 16;
  const barW = 124;
  const barH = 6;
  const r = 3;

  const roundRect = (x, y, w, h, rad) => {
    const rr = Math.min(rad, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + rr, y);
    ctx.arcTo(x + w, y, x + w, y + h, rr);
    ctx.arcTo(x + w, y + h, x, y + h, rr);
    ctx.arcTo(x, y + h, x, y, rr);
    ctx.arcTo(x, y, x + w, y, rr);
    ctx.closePath();
  };

  // Soft backdrop
  ctx.fillStyle = "rgba(0,0,0,0.55)";
  roundRect(barX - 3, 2, barW + 6, 30, 5);
  ctx.fill();

  // Track
  ctx.fillStyle = "rgba(18,22,16,0.92)";
  roundRect(barX, barY, barW, barH, r);
  ctx.fill();

  const fillW = Math.max(2, (barW * percent) / 100);
  let c0 = "#5f9234";
  let c1 = "#b6dc62";
  if (label === "HP") {
    if (percent <= 30) {
      c0 = "#7a1c1c";
      c1 = "#e24d3f";
    } else if (percent <= 60) {
      c0 = "#7a6214";
      c1 = "#e0bf3d";
    } else {
      c0 = "#287034";
      c1 = "#6fc252";
    }
  } else if (label === "TRAIN") {
    c0 = "#2f6a8a";
    c1 = "#6ec4e8";
  }
  const grad = ctx.createLinearGradient(barX, 0, barX + barW, 0);
  grad.addColorStop(0, c0);
  grad.addColorStop(1, c1);
  ctx.fillStyle = grad;
  roundRect(barX, barY, fillW, barH, r);
  ctx.fill();

  // Thin highlight on the fill
  ctx.fillStyle = "rgba(255,255,255,0.22)";
  roundRect(barX, barY, fillW, 2, 1);
  ctx.fill();

  ctx.font = "600 11px Rajdhani, Segoe UI, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineWidth = 2.5;
  ctx.strokeStyle = "rgba(0,0,0,0.75)";
  ctx.fillStyle = "#f6f3ea";
  const text = `${label} ${percent}%`;
  ctx.strokeText(text, 80, 9);
  ctx.fillText(text, 80, 9);

  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.generateMipmaps = true;
  texture.minFilter = THREE.LinearMipmapLinearFilter;
  texture.magFilter = THREE.LinearFilter;
  texture.anisotropy = 4;
  texture.needsUpdate = true;
  const mat = new THREE.SpriteMaterial({
    map: texture,
    transparent: true,
    depthTest: false,
    sizeAttenuation: true,
  });
  const sprite = new THREE.Sprite(mat);
  // Smaller in-world footprint; texture is high-res so it stays crisp.
  if (compact) {
    sprite.scale.set(0.72, 0.16, 1);
  } else {
    sprite.scale.set(0.95, 0.21, 1);
  }
  sprite.center.set(0.5, 0);
  sprite.name = "progressBar";
  return sprite;
}

function clearSpriteBar(mesh, key) {
  const existing = mesh.userData[key];
  if (!existing) return;
  mesh.remove(existing);
  existing.material.map?.dispose();
  existing.material.dispose();
  mesh.userData[key] = null;
  mesh.userData[`${key}Key`] = null;
}

function updateProgressBar(mesh, entity) {
  const load = activeLoadProgress(entity);
  const existing = mesh.userData.progressBar;

  if (!load) {
    clearSpriteBar(mesh, "progressBar");
    return;
  }

  const percent = Math.round(load.pct * 100);
  const key = `${load.label}:${percent}`;
  if (mesh.userData.progressBarKey === key && existing) return;

  clearSpriteBar(mesh, "progressBar");

  const sprite = makeProgressSprite(load.pct, load.label, false);
  const baseH = labelHeightFor(entity);
  sprite.position.set(0, Math.max(0.5, baseH - 0.36), 0);
  mesh.add(sprite);
  mesh.userData.progressBar = sprite;
  mesh.userData.progressBarKey = key;
}

function updateHpBar(mesh, entity) {
  const maxHp = Number(entity.max_hp) || 0;
  const hp = Number(entity.hp) || 0;
  const ratio = maxHp > 0 ? hp / maxHp : 1;

  // Only show when damaged — full HP stays clean.
  if (!(ratio < 1) || maxHp <= 0) {
    clearSpriteBar(mesh, "hpBar");
    return;
  }

  const percent = Math.max(0, Math.min(100, Math.round(ratio * 100)));
  const key = `HP:${percent}`;
  const existing = mesh.userData.hpBar;
  if (mesh.userData.hpBarKey === key && existing) return;

  clearSpriteBar(mesh, "hpBar");

  const compact = !entity.building;
  const sprite = makeProgressSprite(ratio, "HP", compact);
  if (!entity.building) {
    const tank = String(entity.kind || "").includes("tank");
    sprite.scale.set(tank ? 0.24 : 0.2, tank ? 0.055 : 0.045, 1);
  }
  sprite.name = "hpBar";
  const baseH = labelHeightFor(entity);
  const load = activeLoadProgress(entity);
  const y = entity.building
    ? load
      ? Math.max(0.5, baseH - 0.62)
      : Math.max(0.55, baseH - 0.36)
    : load
      ? baseH - 0.04
      : baseH - 0.02;
  sprite.position.set(0, y, 0);
  mesh.add(sprite);
  mesh.userData.hpBar = sprite;
  mesh.userData.hpBarKey = key;
}

function unitDims(kind) {
  const k = String(kind || "");
  // Scale: HQ ~2.15 wu ≈ 22–28 m → infantry ~1.8 m ≈ 0.08 wu tall (≈1/3 prior).
  if (
    k.includes("mlrs") ||
    k.includes("tomahawk") ||
    k.includes("inferno") ||
    k.includes("scud")
  ) {
    return { w: 0.2, h: 0.16, d: 0.32 };
  }
  if (
    k.includes("abrams") ||
    k.includes("paladin") ||
    k.includes("marauder") ||
    k.includes("overlord")
  ) {
    return { w: 0.21, h: 0.14, d: 0.33 };
  }
  if (
    k.includes("tank") ||
    k.includes("vehicle") ||
    k.includes("truck") ||
    k.includes("humvee") ||
    k.includes("technical") ||
    k.includes("buggy") ||
    k.includes("radar_van") ||
    k.includes("cannon") ||
    k.includes("microwave")
  ) {
    // Half prior tank size — closer to infantry / building proportions.
    return { w: 0.18, h: 0.12, d: 0.28 };
  }
  if (
    k.includes("raptor") ||
    k.includes("mig") ||
    k.includes("comanche") ||
    k.includes("helix") ||
    k.includes("chinook")
  ) {
    return { w: 0.22, h: 0.08, d: 0.28 };
  }
  if (k.includes("mortar")) {
    return { w: 0.036, h: 0.08, d: 0.036 };
  }
  if (k.includes("missile") || k.includes("rpg") || k.includes("tank_hunter")) {
    return { w: 0.03, h: 0.08, d: 0.03 };
  }
  return { w: 0.033, h: 0.08, d: 0.033 };
}

function matStd(color, opts = {}) {
  return new THREE.MeshStandardMaterial({
    color,
    metalness: opts.metalness ?? 0.2,
    roughness: opts.roughness ?? 0.7,
    emissive: opts.emissive ?? 0x000000,
    emissiveIntensity: opts.emissiveIntensity ?? 0,
  });
}

function createRangerMesh(teamColor, opts = {}) {
  const style = opts.style || "usa";
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.isInfantry = true;
  g.userData.rigVersion = 5;
  g.userData.tintParts = [];
  g.userData.walkPhase = Math.random() * Math.PI * 2;
  g.userData.moving = false;
  g.userData.factionStyle = style;
  g.rotation.order = "YXZ";

  let camo = 0x4f6340;
  let camoDark = 0x3a4a30;
  let vest = 0x2c3326;
  if (style === "china") {
    camo = 0x6a4030;
    camoDark = 0x4a2818;
    vest = 0x3a2018;
  } else if (style === "gla") {
    camo = 0x8a7a50;
    camoDark = 0x5a4a30;
    vest = 0x4a3a28;
  }
  const boot = 0x1a1612;
  const leather = 0x3b2a1c;
  const skin = 0xc9a882;
  const gun = 0x2a2a28;
  const gunMetal = 0x4a4a46;
  const plastic = 0x1c1c1a;
  const accent = teamColor >>> 0;

  const add = (parent, geo, mat, x, y, z, rx = 0, ry = 0, rz = 0, tint = false) => {
    const m = new THREE.Mesh(geo, mat);
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    if (tint) g.userData.tintParts.push(m);
    parent.add(m);
    return m;
  };

  // —— Legs (hip pivots for walk) ——
  const makeLeg = (name, hx) => {
    const leg = new THREE.Group();
    leg.name = name;
    leg.position.set(hx, 0.078, 0);
    // thigh
    add(leg, new THREE.BoxGeometry(0.03, 0.045, 0.034), matStd(camo), 0, -0.022, 0.002);
    // knee pad
    add(leg, new THREE.BoxGeometry(0.032, 0.016, 0.028), matStd(vest), 0, -0.042, 0.01);
    // calf
    add(leg, new THREE.BoxGeometry(0.028, 0.04, 0.03), matStd(camoDark), 0, -0.062, 0);
    // boot
    add(leg, new THREE.BoxGeometry(0.032, 0.018, 0.046), matStd(boot), 0, -0.082, 0.008);
    add(leg, new THREE.BoxGeometry(0.03, 0.01, 0.018), matStd(0x0e0c0a), 0, -0.088, 0.02);
    g.add(leg);
    return leg;
  };
  makeLeg("leftLeg", -0.02);
  makeLeg("rightLeg", 0.02);

  // —— Torso ——
  const torso = new THREE.Group();
  torso.name = "torso";

  // hips / belt
  add(torso, new THREE.BoxGeometry(0.078, 0.022, 0.05), matStd(camoDark), 0, 0.09, 0);
  add(torso, new THREE.BoxGeometry(0.082, 0.012, 0.052), matStd(leather), 0, 0.1, 0.002);
  // belt pouches
  add(torso, new THREE.BoxGeometry(0.018, 0.02, 0.014), matStd(vest), -0.03, 0.095, 0.028);
  add(torso, new THREE.BoxGeometry(0.018, 0.02, 0.014), matStd(vest), 0.03, 0.095, 0.028);
  add(torso, new THREE.BoxGeometry(0.022, 0.018, 0.012), matStd(0x24301c), 0, 0.094, 0.03);

  // chest / jacket
  add(torso, new THREE.BoxGeometry(0.076, 0.07, 0.048), matStd(camo), 0, 0.138, 0);
  // plate carrier
  add(torso, new THREE.BoxGeometry(0.07, 0.055, 0.03), matStd(vest), 0, 0.14, 0.018);
  // mag pouches row
  for (let i = -1; i <= 1; i++) {
    add(
      torso,
      new THREE.BoxGeometry(0.018, 0.028, 0.016),
      matStd(0x232820),
      i * 0.022,
      0.132,
      0.038,
    );
  }
  // team ID stripe
  add(
    torso,
    new THREE.BoxGeometry(0.078, 0.01, 0.052),
    matStd(accent, { roughness: 0.45 }),
    0,
    0.162,
    0.004,
    0,
    0,
    0,
    true,
  );
  // collar
  add(torso, new THREE.BoxGeometry(0.05, 0.012, 0.04), matStd(camoDark), 0, 0.175, -0.002);

  // backpack
  add(torso, new THREE.BoxGeometry(0.05, 0.055, 0.028), matStd(vest), 0, 0.14, -0.036);
  add(torso, new THREE.BoxGeometry(0.04, 0.02, 0.02), matStd(0x1e2418), 0, 0.165, -0.04);
  // radio brick + antenna
  add(torso, new THREE.BoxGeometry(0.018, 0.028, 0.016), matStd(plastic), 0.028, 0.15, -0.048);
  add(
    torso,
    new THREE.CylinderGeometry(0.003, 0.003, 0.07, 5),
    matStd(gunMetal, { metalness: 0.6, roughness: 0.35 }),
    0.028,
    0.195,
    -0.048,
  );

  // —— Arms ——
  const makeArm = (name, ax, az) => {
    const arm = new THREE.Group();
    arm.name = name;
    arm.position.set(ax, 0.165, az);
    // shoulder pad
    add(arm, new THREE.BoxGeometry(0.028, 0.022, 0.03), matStd(vest), 0, 0, 0);
    // upper arm
    add(arm, new THREE.BoxGeometry(0.024, 0.04, 0.024), matStd(camo), 0, -0.028, 0.004);
    // elbow pad
    add(arm, new THREE.BoxGeometry(0.026, 0.014, 0.022), matStd(vest), 0, -0.046, 0.008);
    // forearm
    add(arm, new THREE.BoxGeometry(0.022, 0.036, 0.022), matStd(camoDark), 0, -0.066, 0.01);
    // glove
    add(arm, new THREE.BoxGeometry(0.02, 0.016, 0.024), matStd(boot), 0, -0.088, 0.014);
    torso.add(arm);
    return arm;
  };
  makeArm("leftArm", -0.052, 0.008);
  makeArm("rightArm", 0.052, 0.016);

  // —— Head ——
  add(torso, new THREE.BoxGeometry(0.04, 0.04, 0.038), matStd(skin), 0, 0.198, 0.002);
  // balaclava / neck
  add(torso, new THREE.BoxGeometry(0.036, 0.016, 0.034), matStd(0x2a2824), 0, 0.182, 0.004);
  // helmet shell
  add(torso, new THREE.BoxGeometry(0.05, 0.024, 0.052), matStd(camoDark), 0, 0.218, 0);
  add(torso, new THREE.BoxGeometry(0.046, 0.014, 0.048), matStd(vest), 0, 0.23, -0.002);
  // helmet brim / goggles mount
  add(torso, new THREE.BoxGeometry(0.048, 0.01, 0.016), matStd(plastic), 0, 0.21, 0.024);
  add(torso, new THREE.BoxGeometry(0.036, 0.01, 0.012), matStd(0x66aacc, { metalness: 0.3, roughness: 0.25 }), 0, 0.206, 0.03);
  // team helmet band
  add(
    torso,
    new THREE.BoxGeometry(0.052, 0.008, 0.018),
    matStd(accent, { roughness: 0.4 }),
    0,
    0.214,
    0.02,
    0,
    0,
    0,
    true,
  );
  // chin strap
  add(torso, new THREE.BoxGeometry(0.008, 0.02, 0.004), matStd(boot), -0.018, 0.195, 0.016);
  add(torso, new THREE.BoxGeometry(0.008, 0.02, 0.004), matStd(boot), 0.018, 0.195, 0.016);

  // —— Detailed rifle (muzzle toward +Z) ——
  const rifle = new THREE.Group();
  rifle.name = "muzzleRoot";
  // stock
  add(rifle, new THREE.BoxGeometry(0.016, 0.022, 0.036), matStd(plastic), 0.032, 0.118, -0.02);
  add(rifle, new THREE.BoxGeometry(0.014, 0.012, 0.02), matStd(leather), 0.032, 0.11, -0.038);
  // receiver
  add(
    rifle,
    new THREE.BoxGeometry(0.018, 0.02, 0.055),
    matStd(gunMetal, { metalness: 0.55, roughness: 0.4 }),
    0.034,
    0.122,
    0.02,
  );
  // carry handle / optic
  add(rifle, new THREE.BoxGeometry(0.012, 0.014, 0.03), matStd(plastic), 0.034, 0.138, 0.015);
  add(rifle, new THREE.BoxGeometry(0.01, 0.008, 0.016), matStd(0x111110), 0.034, 0.146, 0.02);
  // magazine
  add(rifle, new THREE.BoxGeometry(0.014, 0.032, 0.018), matStd(plastic), 0.034, 0.1, 0.018);
  // handguard
  add(
    rifle,
    new THREE.BoxGeometry(0.02, 0.018, 0.05),
    matStd(gun, { metalness: 0.4, roughness: 0.45 }),
    0.034,
    0.122,
    0.068,
  );
  // barrel
  add(
    rifle,
    new THREE.CylinderGeometry(0.005, 0.006, 0.07, 6),
    matStd(0x151514, { metalness: 0.75, roughness: 0.3 }),
    0.034,
    0.124,
    0.12,
    Math.PI / 2,
    0,
    0,
  );
  // front sight / flash hider
  add(rifle, new THREE.BoxGeometry(0.008, 0.014, 0.008), matStd(gunMetal), 0.034, 0.134, 0.145);
  add(
    rifle,
    new THREE.CylinderGeometry(0.007, 0.008, 0.016, 6),
    matStd(0x0c0c0c, { metalness: 0.7, roughness: 0.35 }),
    0.034,
    0.124,
    0.158,
    Math.PI / 2,
    0,
    0,
  );
  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0.034, 0.124, 0.168);
  rifle.add(tip);
  torso.add(rifle);

  g.add(torso);
  g.userData.unitHeight = 0.08;
  g.userData.walk = {
    leftLeg: g.getObjectByName("leftLeg"),
    rightLeg: g.getObjectByName("rightLeg"),
    leftArm: torso.getObjectByName("leftArm"),
    rightArm: torso.getObjectByName("rightArm"),
    torso,
  };
  // Real-scale infantry vs buildings (~1/3 of previous size).
  g.scale.setScalar(1 / 3);
  return g;
}

function createMortarMesh(teamColor) {
  const g = createRangerMesh(teamColor);
  g.userData.isMortar = true;
  g.userData.rigVersion = 5;
  // Swap rifle for a bipod mortar tube (elevated lob).
  const old = g.getObjectByName("muzzleRoot");
  if (old) old.parent?.remove(old);
  const mortar = new THREE.Group();
  mortar.name = "muzzleRoot";
  mortar.position.set(0.02, 0.02, 0.02);

  // Baseplate
  const plate = new THREE.Mesh(
    new THREE.BoxGeometry(0.05, 0.008, 0.05),
    matStd(0x2a2c28, { metalness: 0.45, roughness: 0.5 }),
  );
  plate.position.set(0.01, 0.004, 0.04);
  mortar.add(plate);

  // Bipod legs
  for (const side of [-1, 1]) {
    const leg = new THREE.Mesh(
      new THREE.CylinderGeometry(0.004, 0.005, 0.07, 5),
      matStd(0x3a3c38, { metalness: 0.55, roughness: 0.4 }),
    );
    leg.position.set(side * 0.028, 0.028, 0.055);
    leg.rotation.z = side * 0.55;
    leg.rotation.x = 0.35;
    mortar.add(leg);
  }

  // Tube elevated ~55°
  const tubeGroup = new THREE.Group();
  tubeGroup.position.set(0.01, 0.02, 0.03);
  tubeGroup.rotation.x = -0.95;
  mortar.add(tubeGroup);

  const tube = new THREE.Mesh(
    new THREE.CylinderGeometry(0.012, 0.014, 0.11, 8),
    matStd(0x4a553c, { metalness: 0.4, roughness: 0.45 }),
  );
  tube.position.y = 0.055;
  tubeGroup.add(tube);
  const breech = new THREE.Mesh(
    new THREE.CylinderGeometry(0.015, 0.015, 0.018, 8),
    matStd(0x2a2e28, { metalness: 0.5, roughness: 0.4 }),
  );
  breech.position.y = 0.008;
  tubeGroup.add(breech);
  const muzzleRing = new THREE.Mesh(
    new THREE.CylinderGeometry(0.013, 0.015, 0.01, 8),
    matStd(0x1a1c18, { metalness: 0.65, roughness: 0.35 }),
  );
  muzzleRing.position.y = 0.11;
  tubeGroup.add(muzzleRing);

  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0, 0.12, 0);
  tubeGroup.add(tip);

  // Ammo satchel on left hip
  const torso = g.getObjectByName("torso");
  if (torso) {
    const satchel = new THREE.Mesh(
      new THREE.BoxGeometry(0.028, 0.032, 0.02),
      matStd(0x3b2a1c, { metalness: 0.1, roughness: 0.75 }),
    );
    satchel.position.set(-0.055, 0.1, -0.01);
    torso.add(satchel);
  }

  (torso || g).add(mortar);
  return g;
}

function createTankMesh(teamColor, opts = {}) {
  const heavy = !!opts.heavy;
  const style = opts.style || "usa";
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.tintParts = [];
  g.userData.tankRigVersion = 7;
  g.userData.isAbrams = heavy;
  g.userData.factionStyle = style;

  // USA olive · China olive-red · GLA desert scrap
  let hull = heavy ? 0x3f4634 : 0x4a5538;
  let hullDark = heavy ? 0x2c3224 : 0x353c2c;
  let hullLight = heavy ? 0x525a42 : 0x5a6648;
  if (style === "china") {
    hull = heavy ? 0x4a3a28 : 0x5a4830;
    hullDark = 0x322818;
    hullLight = 0x6a5840;
  } else if (style === "gla") {
    hull = heavy ? 0x6a5a38 : 0x7a6a48;
    hullDark = 0x4a3e28;
    hullLight = 0x8a7a58;
  }
  const track = 0x1a1814;
  const rubber = 0x11100e;
  const metal = 0x2a2a26;
  const rust = style === "gla" ? 0x5a4030 : 0x3a3228;
  const accent = teamColor >>> 0;

  const add = (parent, geo, mat, x, y, z, rx = 0, ry = 0, rz = 0, tint = false) => {
    const m = new THREE.Mesh(geo, mat);
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    m.castShadow = true;
    m.receiveShadow = true;
    if (tint) g.userData.tintParts.push(m);
    parent.add(m);
    return m;
  };

  // —— Tracks, wheels, return rollers ——
  for (const side of [-1, 1]) {
    const x = side * 0.175;
    // Track armor / link band
    add(g, new THREE.BoxGeometry(0.058, 0.078, 0.54), matStd(track), x, 0.042, 0);
    add(g, new THREE.BoxGeometry(0.042, 0.028, 0.52), matStd(rubber), x, 0.01, 0);
    // Road wheels
    for (let i = -2; i <= 2; i++) {
      const w = add(
        g,
        new THREE.CylinderGeometry(0.026, 0.026, 0.042, 10),
        matStd(metal, { metalness: 0.55, roughness: 0.42 }),
        x,
        0.03,
        i * 0.095,
        0,
        0,
        Math.PI / 2,
      );
      w.userData.roadWheel = true;
      add(
        g,
        new THREE.CylinderGeometry(0.012, 0.012, 0.044, 6),
        matStd(0x0e0e0c),
        x,
        0.03,
        i * 0.095,
        0,
        0,
        Math.PI / 2,
      );
    }
    // Drive sprocket (rear) + idler (front)
    const sprocket = add(
      g,
      new THREE.CylinderGeometry(0.032, 0.032, 0.04, 10),
      matStd(metal, { metalness: 0.6 }),
      x,
      0.04,
      -0.255,
      0,
      0,
      Math.PI / 2,
    );
    sprocket.userData.roadWheel = true;
    const idler = add(
      g,
      new THREE.CylinderGeometry(0.028, 0.028, 0.038, 10),
      matStd(metal, { metalness: 0.55 }),
      x,
      0.038,
      0.255,
      0,
      0,
      Math.PI / 2,
    );
    idler.userData.roadWheel = true;
    // Side skirts / schürzen
    add(g, new THREE.BoxGeometry(0.018, 0.055, 0.5), matStd(hullDark), x * 1.22, 0.078, 0);
    add(g, new THREE.BoxGeometry(0.014, 0.02, 0.12), matStd(hull), x * 1.24, 0.095, 0.12);
    add(g, new THREE.BoxGeometry(0.014, 0.02, 0.12), matStd(hull), x * 1.24, 0.095, -0.12);
  }

  // —— Hull ——
  add(g, new THREE.BoxGeometry(0.32, 0.085, 0.5), matStd(hull), 0, 0.085, 0);
  add(g, new THREE.BoxGeometry(0.3, 0.05, 0.38), matStd(hullDark), 0, 0.14, -0.02);
  // Front glacis plate
  add(g, new THREE.BoxGeometry(0.28, 0.035, 0.12), matStd(hullLight), 0, 0.118, 0.21, -0.38, 0, 0);
  add(g, new THREE.BoxGeometry(0.22, 0.02, 0.06), matStd(hullDark), 0, 0.135, 0.18, -0.2, 0, 0);
  // Rear engine deck
  add(g, new THREE.BoxGeometry(0.26, 0.03, 0.1), matStd(metal), 0, 0.132, -0.24);
  add(g, new THREE.BoxGeometry(0.1, 0.012, 0.06), matStd(0x1e1e1a), -0.06, 0.148, -0.24);
  add(g, new THREE.BoxGeometry(0.1, 0.012, 0.06), matStd(0x1e1e1a), 0.06, 0.148, -0.24);
  // Exhausts
  add(g, new THREE.CylinderGeometry(0.016, 0.018, 0.05, 8), matStd(0x1a1a18), -0.09, 0.148, -0.265, Math.PI / 2, 0, 0);
  add(g, new THREE.CylinderGeometry(0.016, 0.018, 0.05, 8), matStd(0x1a1a18), 0.09, 0.148, -0.265, Math.PI / 2, 0, 0);
  // Team stripe on rear deck
  add(
    g,
    new THREE.BoxGeometry(0.24, 0.014, 0.055),
    matStd(accent, { roughness: 0.45 }),
    0,
    0.152,
    -0.14,
    0,
    0,
    0,
    true,
  );
  // Driver hatch + vision block
  add(g, new THREE.BoxGeometry(0.065, 0.022, 0.065), matStd(hullDark), -0.075, 0.158, 0.1);
  add(g, new THREE.BoxGeometry(0.04, 0.012, 0.02), matStd(0x111110), -0.075, 0.168, 0.125);
  // Co-driver MG mount bump
  add(g, new THREE.BoxGeometry(0.05, 0.018, 0.05), matStd(hullDark), 0.08, 0.155, 0.12);
  // Front headlights
  add(g, new THREE.BoxGeometry(0.028, 0.022, 0.02), matStd(metal), -0.11, 0.11, 0.26);
  add(g, new THREE.BoxGeometry(0.02, 0.016, 0.012), matStd(0xfff2a8, { emissive: 0xaa8800, emissiveIntensity: 0.35, roughness: 0.3 }), -0.11, 0.11, 0.272);
  add(g, new THREE.BoxGeometry(0.028, 0.022, 0.02), matStd(metal), 0.11, 0.11, 0.26);
  add(g, new THREE.BoxGeometry(0.02, 0.016, 0.012), matStd(0xfff2a8, { emissive: 0xaa8800, emissiveIntensity: 0.35, roughness: 0.3 }), 0.11, 0.11, 0.272);
  // Fuel / stowage boxes on fenders
  add(g, new THREE.BoxGeometry(0.04, 0.035, 0.1), matStd(rust), -0.2, 0.12, -0.05);
  add(g, new THREE.BoxGeometry(0.04, 0.035, 0.1), matStd(rust), 0.2, 0.12, -0.05);
  // Front mud flaps
  add(g, new THREE.BoxGeometry(0.05, 0.04, 0.01), matStd(rubber), -0.175, 0.06, 0.28);
  add(g, new THREE.BoxGeometry(0.05, 0.04, 0.01), matStd(rubber), 0.175, 0.06, 0.28);

  // —— Turret ——
  const turret = new THREE.Group();
  turret.name = "muzzleRoot";
  // Main turret body (slightly tapered look via stacked boxes)
  add(turret, new THREE.BoxGeometry(0.2, 0.095, 0.24), matStd(0x3d4730), 0, 0.21, -0.02);
  add(turret, new THREE.BoxGeometry(0.17, 0.045, 0.14), matStd(hullDark), 0, 0.268, -0.04);
  // Angled cheek armor
  add(turret, new THREE.BoxGeometry(0.04, 0.07, 0.16), matStd(hullLight), -0.11, 0.215, 0.02, 0, 0, 0.25);
  add(turret, new THREE.BoxGeometry(0.04, 0.07, 0.16), matStd(hullLight), 0.11, 0.215, 0.02, 0, 0, -0.25);
  // Bustle / ammo rack rear
  add(turret, new THREE.BoxGeometry(0.14, 0.05, 0.08), matStd(0x2f3528), 0, 0.22, -0.16);
  add(turret, new THREE.BoxGeometry(0.1, 0.03, 0.05), matStd(rust), 0, 0.245, -0.18);
  // Commander cupola
  add(turret, new THREE.CylinderGeometry(0.038, 0.044, 0.032, 10), matStd(metal), 0.045, 0.295, -0.02);
  add(turret, new THREE.BoxGeometry(0.032, 0.014, 0.032), matStd(0x222018), 0.045, 0.312, -0.02);
  // Roof pintle MG (fires independently of the main gun)
  const mg = new THREE.Group();
  mg.name = "tankMg";
  mg.position.set(0.045, 0.328, -0.02);
  add(mg, new THREE.CylinderGeometry(0.01, 0.01, 0.022, 6), matStd(metal), 0, 0.012, 0);
  add(mg, new THREE.BoxGeometry(0.016, 0.012, 0.028), matStd(0x1a1a16), 0, 0.02, 0.01);
  const mgBarrel = new THREE.Mesh(
    new THREE.CylinderGeometry(0.0045, 0.0055, 0.09, 6),
    matStd(0x111110, { metalness: 0.65 }),
  );
  mgBarrel.rotation.x = Math.PI / 2;
  mgBarrel.position.set(0, 0.022, 0.052);
  mgBarrel.castShadow = true;
  mg.add(mgBarrel);
  add(mg, new THREE.BoxGeometry(0.012, 0.008, 0.018), matStd(metal), 0, 0.03, 0.02);
  const mgTip = new THREE.Object3D();
  mgTip.name = "mgMuzzle";
  mgTip.position.set(0, 0.022, 0.1);
  mg.add(mgTip);
  turret.add(mg);
  // Smoke grenade launchers
  for (let i = 0; i < 3; i++) {
    add(turret, new THREE.CylinderGeometry(0.008, 0.008, 0.03, 6), matStd(metal), -0.09, 0.24, 0.06 + i * 0.025, 0.6, 0, 0.4);
    add(turret, new THREE.CylinderGeometry(0.008, 0.008, 0.03, 6), matStd(metal), 0.09, 0.24, 0.06 + i * 0.025, 0.6, 0, -0.4);
  }
  // Antenna
  add(turret, new THREE.CylinderGeometry(0.003, 0.003, 0.22, 4), matStd(0x222220, { metalness: 0.5 }), -0.08, 0.36, -0.12);
  // Team turret band
  add(
    turret,
    new THREE.BoxGeometry(0.18, 0.012, 0.04),
    matStd(accent, { roughness: 0.4 }),
    0,
    0.255,
    0.08,
    0,
    0,
    0,
    true,
  );
  // Mantlet
  add(turret, new THREE.BoxGeometry(0.09, 0.07, 0.06), matStd(metal, { metalness: 0.55 }), 0, 0.215, 0.11);

  const barrelGroup = new THREE.Group();
  barrelGroup.name = "tankBarrel";
  barrelGroup.position.set(0, 0.215, 0.11);
  // Thermal sleeve segments
  const barrel = new THREE.Mesh(
    new THREE.CylinderGeometry(0.017, 0.022, 0.28, 10),
    matStd(0x141412, { metalness: 0.7, roughness: 0.35 }),
  );
  barrel.rotation.x = Math.PI / 2;
  barrel.position.set(0, 0, 0.16);
  barrel.castShadow = true;
  barrelGroup.add(barrel);
  add(barrelGroup, new THREE.CylinderGeometry(0.019, 0.019, 0.08, 10), matStd(0x1a1a16, { metalness: 0.65 }), 0, 0, 0.34, Math.PI / 2, 0, 0);
  // Fume extractor
  add(barrelGroup, new THREE.CylinderGeometry(0.026, 0.026, 0.05, 10), matStd(0x1c1c18, { metalness: 0.6 }), 0, 0, 0.28, Math.PI / 2, 0, 0);
  // Muzzle brake
  add(barrelGroup, new THREE.CylinderGeometry(0.02, 0.028, 0.035, 10), matStd(metal, { metalness: 0.7 }), 0, 0, 0.42, Math.PI / 2, 0, 0);
  add(barrelGroup, new THREE.BoxGeometry(0.04, 0.018, 0.02), matStd(metal), 0, 0, 0.435);
  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0, 0, 0.46);
  barrelGroup.add(tip);
  turret.add(barrelGroup);
  // Coaxial MG
  add(turret, new THREE.CylinderGeometry(0.006, 0.006, 0.09, 5), matStd(0x111110), 0.045, 0.2, 0.155, Math.PI / 2, 0, 0);

  g.add(turret);
  g.userData.unitHeight = heavy ? 0.19 : 0.16;
  g.userData.isTank = true;
  g.userData.hullTurnRate = heavy ? 0.85 : 1.05;
  g.userData.turretTurnRate = heavy ? 1.05 : 1.25;
  g.userData.barrelRecoil = 0;
  g.userData.tankRigVersion = 6;
  // Half visual size vs prior rig; Abrams slightly larger silhouette.
  g.scale.setScalar(heavy ? 0.59 : 0.5);
  if (heavy) {
    // Reactive armor bricks on the turret cheeks
    add(turret, new THREE.BoxGeometry(0.06, 0.04, 0.1), matStd(0x5a5038, { metalness: 0.35 }), -0.12, 0.22, 0.02, 0, 0, 0, true);
    add(turret, new THREE.BoxGeometry(0.06, 0.04, 0.1), matStd(0x5a5038, { metalness: 0.35 }), 0.12, 0.22, 0.02, 0, 0, 0, true);
    tip.position.set(0, 0, 0.52);
  }
  return g;
}

/** M270 MLRS — tracked launcher with elevating dual rocket pods (not a tank). */
function createMlrsMesh(teamColor) {
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.isTank = true; // hull drive + pod yaw reuse tank motion path
  g.userData.isMlrs = true;
  g.userData.tintParts = [];
  g.userData.mlrsRigVersion = 1;
  g.userData.tankRigVersion = 6;

  const hull = 0x4a5240;
  const hullDark = 0x32382c;
  const hullLight = 0x5c6650;
  const track = 0x1a1814;
  const rubber = 0x11100e;
  const metal = 0x2c2c28;
  const pod = 0x3e4536;
  const podDark = 0x2a2f24;
  const tube = 0x1e2018;
  const glass = 0x1a2228;
  const accent = teamColor >>> 0;

  const add = (parent, geo, mat, x, y, z, rx = 0, ry = 0, rz = 0, tint = false) => {
    const m = new THREE.Mesh(geo, typeof mat === "number" ? matStd(mat) : mat);
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    m.castShadow = true;
    m.receiveShadow = true;
    if (tint) g.userData.tintParts.push(m);
    parent.add(m);
    return m;
  };

  // —— Tracks (Bradley-derived, longer wheelbase) ——
  for (const side of [-1, 1]) {
    const x = side * 0.168;
    add(g, new THREE.BoxGeometry(0.052, 0.072, 0.58), track, x, 0.04, -0.02);
    add(g, new THREE.BoxGeometry(0.038, 0.024, 0.56), rubber, x, 0.01, -0.02);
    for (let i = 0; i < 6; i++) {
      const z = -0.22 + i * 0.088;
      const wheel = add(
        g,
        new THREE.CylinderGeometry(0.028, 0.028, 0.034, 10),
        rubber,
        x,
        0.028,
        z,
        0,
        0,
        Math.PI / 2,
      );
      wheel.userData.roadWheel = true;
    }
    const sprocket = add(
      g,
      new THREE.CylinderGeometry(0.032, 0.032, 0.036, 12),
      metal,
      x,
      0.034,
      -0.28,
      0,
      0,
      Math.PI / 2,
    );
    sprocket.userData.roadWheel = true;
    add(g, new THREE.CylinderGeometry(0.026, 0.026, 0.034, 10), metal, x, 0.03, 0.26, 0, 0, Math.PI / 2);
  }

  // —— Lower hull / chassis ——
  add(g, new THREE.BoxGeometry(0.28, 0.07, 0.52), hull, 0, 0.075, -0.01, 0, 0, 0, true);
  add(g, new THREE.BoxGeometry(0.26, 0.04, 0.48), hullDark, 0, 0.12, -0.01);
  // Side skirts
  for (const side of [-1, 1]) {
    add(g, new THREE.BoxGeometry(0.018, 0.055, 0.5), hullLight, side * 0.148, 0.08, -0.02, 0, 0, 0, true);
  }

  // —— Cab (front) ——
  const cab = new THREE.Group();
  cab.position.set(0, 0.14, 0.18);
  g.add(cab);
  add(cab, new THREE.BoxGeometry(0.22, 0.14, 0.2), hull, 0, 0.07, 0, 0, 0, 0, true);
  add(cab, new THREE.BoxGeometry(0.2, 0.06, 0.04), glass, 0, 0.1, 0.09);
  add(cab, new THREE.BoxGeometry(0.04, 0.05, 0.02), glass, -0.095, 0.09, 0.02);
  add(cab, new THREE.BoxGeometry(0.04, 0.05, 0.02), glass, 0.095, 0.09, 0.02);
  // Team stripe on cab roof
  add(cab, new THREE.BoxGeometry(0.16, 0.012, 0.06), accent, 0, 0.145, -0.02, 0, 0, 0, true);
  // Bumper / light bar
  add(cab, new THREE.BoxGeometry(0.2, 0.025, 0.03), metal, 0, 0.02, 0.11);
  add(cab, new THREE.BoxGeometry(0.03, 0.02, 0.015), 0xc8c090, -0.07, 0.035, 0.12);
  add(cab, new THREE.BoxGeometry(0.03, 0.02, 0.015), 0xc8c090, 0.07, 0.035, 0.12);

  // —— Elevating rocket pod (yaw + elevation) ——
  const turret = new THREE.Group();
  turret.name = "muzzleRoot";
  turret.position.set(0, 0.155, -0.12);
  g.add(turret);

  // Traversing ring / base
  add(turret, new THREE.CylinderGeometry(0.08, 0.09, 0.03, 12), metal, 0, 0.01, 0);
  add(turret, new THREE.BoxGeometry(0.12, 0.04, 0.14), hullDark, 0, 0.035, 0);

  const elev = new THREE.Group();
  elev.name = "mlrsElev";
  elev.position.set(0, 0.055, 0);
  // Default elevation ~25° like a loaded launch posture
  elev.rotation.x = -0.42;
  turret.add(elev);

  // Dual M269 pods side-by-side
  for (const side of [-1, 1]) {
    const bay = new THREE.Group();
    bay.position.set(side * 0.072, 0.06, 0);
    elev.add(bay);
    add(bay, new THREE.BoxGeometry(0.11, 0.12, 0.32), pod, 0, 0, 0);
    add(bay, new THREE.BoxGeometry(0.1, 0.02, 0.3), podDark, 0, 0.065, 0);
    add(bay, new THREE.BoxGeometry(0.1, 0.02, 0.3), podDark, 0, -0.065, 0);
    // 3×2 tube mouths facing +Z (forward when elevated)
    for (let row = 0; row < 2; row++) {
      for (let col = 0; col < 3; col++) {
        const tx = (col - 1) * 0.028;
        const ty = (row - 0.5) * 0.04;
        add(bay, new THREE.CylinderGeometry(0.011, 0.011, 0.3, 8), tube, tx, ty, 0, Math.PI / 2, 0, 0);
        add(bay, new THREE.CylinderGeometry(0.012, 0.012, 0.012, 8), metal, tx, ty, 0.155, Math.PI / 2, 0, 0);
      }
    }
  }

  // Center spine between pods
  add(elev, new THREE.BoxGeometry(0.03, 0.08, 0.28), metal, 0, 0.05, 0);
  // Hydraulic ram suggestion
  add(elev, new THREE.CylinderGeometry(0.008, 0.008, 0.14, 6), metal, 0.02, -0.02, -0.08, 0.6, 0, 0);

  // Muzzle tip at pod array face (world FX)
  const muzzle = new THREE.Object3D();
  muzzle.name = "muzzle";
  muzzle.position.set(0, 0.06, 0.18);
  elev.add(muzzle);

  // Dummy barrel name so remesh checks that look for tankBarrel pass
  const dummyBarrel = new THREE.Object3D();
  dummyBarrel.name = "tankBarrel";
  dummyBarrel.position.set(0, 0.06, 0.1);
  elev.add(dummyBarrel);

  g.userData.unitHeight = 0.28;
  g.userData.hullTurnRate = 0.95;
  g.userData.turretTurnRate = 0.85;
  g.userData.barrelRecoil = 0;
  g.scale.setScalar(0.5);
  return g;
}

function isAirUnitKind(kind) {
  const k = String(kind || "");
  return (
    k.includes("raptor") ||
    k.includes("mig") ||
    k.includes("comanche") ||
    k.includes("helix") ||
    k.includes("chinook")
  );
}

function isHeavyTankKind(kind) {
  const k = String(kind || "");
  return (
    k.includes("abrams") ||
    k.includes("paladin") ||
    k.includes("marauder") ||
    k.includes("overlord")
  );
}

function createLightVehicleMesh(teamColor, opts = {}) {
  const style = opts.style || "usa";
  const m = createTankMesh(teamColor, { heavy: false, style });
  m.scale.setScalar(style === "gla" ? 0.68 : 0.72);
  m.userData.isLightVehicle = true;
  return m;
}

function createAirMesh(teamColor) {
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.isAir = true;
  g.userData.tintParts = [];
  g.userData.unitHeight = 0.35;
  const accent = teamColor >>> 0;
  const body = new THREE.Mesh(
    new THREE.BoxGeometry(0.1, 0.04, 0.28),
    matStd(0x3a3c38, { metalness: 0.45, roughness: 0.4 }),
  );
  body.position.y = 0.02;
  g.add(body);
  g.userData.tintParts.push(body);
  const wing = new THREE.Mesh(
    new THREE.BoxGeometry(0.36, 0.012, 0.08),
    matStd(accent, { metalness: 0.35, roughness: 0.5 }),
  );
  wing.position.set(0, 0.025, -0.02);
  g.add(wing);
  g.userData.tintParts.push(wing);
  const tail = new THREE.Mesh(
    new THREE.BoxGeometry(0.04, 0.06, 0.06),
    matStd(0x2a2c28, { metalness: 0.4, roughness: 0.45 }),
  );
  tail.position.set(0, 0.05, -0.12);
  g.add(tail);
  return g;
}

function createUnitMesh(kind, teamColor) {
  const k = String(kind || "");
  // Infer Generals faction silhouette from unit id (rosters never share ids).
  const style =
    k.includes("battlemaster") ||
    k.includes("overlord") ||
    k.includes("gatling") ||
    k.includes("inferno") ||
    k.includes("troop_crawler") ||
    k.includes("listening_outpost") ||
    k.includes("ecm") ||
    k.includes("red_guard") ||
    k.includes("tank_hunter") ||
    k.includes("hacker") ||
    k.includes("lotus") ||
    k.includes("mig") ||
    k.includes("helix")
      ? "china"
      : k.includes("scorpion") ||
          k.includes("marauder") ||
          k.includes("technical") ||
          k.includes("buggy") ||
          k.includes("quad") ||
          k.includes("bomb_truck") ||
          k.includes("scud") ||
          k.includes("radar_van") ||
          k.includes("battle_bus") ||
          k.includes("rebel") ||
          k.includes("rpg") ||
          k.includes("terrorist") ||
          k.includes("hijacker") ||
          k.includes("jarmen")
        ? "gla"
        : "usa";

  // Air — temporary simple elevated mesh
  if (isAirUnitKind(k)) return createAirMesh(teamColor);

  // Artillery / rocket vehicles
  if (
    k.includes("mlrs") ||
    k.includes("tomahawk") ||
    k.includes("inferno") ||
    k.includes("scud")
  ) {
    return createMlrsMesh(teamColor);
  }

  // Heavy / medium tanks
  if (
    k.includes("abrams") ||
    k.includes("paladin") ||
    k.includes("marauder") ||
    k.includes("overlord")
  ) {
    return createTankMesh(teamColor, { heavy: true, style });
  }
  if (
    k.includes("battlemaster") ||
    k.includes("scorpion") ||
    k.includes("gatling_tank") ||
    k.includes("quad_cannon") ||
    k.includes("microwave") ||
    (k.includes("tank") && !k.includes("hunter"))
  ) {
    return createTankMesh(teamColor, { style });
  }

  // Light vehicles
  if (
    k.includes("humvee") ||
    k.includes("technical") ||
    k.includes("rocket_buggy") ||
    k.includes("radar_van") ||
    k.includes("buggy") ||
    k.includes("troop_crawler") ||
    k.includes("battle_bus") ||
    k.includes("bomb_truck")
  ) {
    return createLightVehicleMesh(teamColor, { style });
  }

  // Rocket / AT infantry → mortar pose
  if (
    k.includes("mortar") ||
    k.includes("missile_defender") ||
    k.includes("tank_hunter") ||
    k.includes("rpg")
  ) {
    return createMortarMesh(teamColor);
  }

  // Infantry / heroes / specialists
  if (
    k.includes("ranger") ||
    k.includes("red_guard") ||
    k.includes("rebel") ||
    k.includes("pathfinder") ||
    k.includes("terrorist") ||
    k.includes("hacker") ||
    k.includes("hijacker") ||
    k.includes("colonel") ||
    k.includes("lotus") ||
    k.includes("jarmen") ||
    k.includes("burton")
  ) {
    return createRangerMesh(teamColor, { style });
  }

  // Fallback
  if (k.includes("missile") || k.includes("mortar")) return createMortarMesh(teamColor);
  if (k.includes("tank") || k.includes("cannon")) return createTankMesh(teamColor, { style });
  return createRangerMesh(teamColor, { style });
}


function tintUnitMesh(mesh, colors) {
  const parts = mesh.userData.tintParts;
  if (!parts?.length) return;
  const c = colors[0] >>> 0;
  for (const p of parts) {
    if (p.material?.color) p.material.color.setHex(c);
  }
}

function colorFor(entity) {
  return entityColors(entity)[0];
}

function disposeMeshTree(mesh) {
  if (!mesh) return;
  mesh.traverse((obj) => {
    if (obj.geometry) obj.geometry.dispose?.();
    if (obj.material) {
      const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
      for (const mat of mats) {
        mat.map?.dispose?.();
        mat.dispose?.();
      }
    }
  });
}

function buildingHasProperModel(kind) {
  return (
    kind === "turret" ||
    kind === "stinger_site" ||
    kind === "gatling_cannon" ||
    kind === "bunker" ||
    kind === "tunnel_network" ||
    kind === "demo_trap" ||
    kind === "radar" ||
    kind === "firebase" ||
    kind === "hq" ||
    kind === "barracks" ||
    kind === "power_plant" ||
    kind === "nuclear_reactor" ||
    kind === "supply" ||
    kind === "supply_stash" ||
    kind === "war_factory" ||
    kind === "arms_dealer" ||
    kind === "airfield" ||
    kind === "strategy_center" ||
    kind === "propaganda_center" ||
    kind === "palace" ||
    kind === "internet_center" ||
    kind === "black_market" ||
    kind === "particle_cannon" ||
    kind === "nuclear_silo" ||
    kind === "scud_storm"
  );
}

function upsertMesh(entity) {
  if (!scene) return;

  let mesh = state.meshes.get(entity.id);
  const colors = entityColors(entity);

  // Replace old OBJ/STL or outdated procedural buildings.
  if (mesh && entity.building && mesh.userData.isFallback && buildingHasProperModel(entity.kind)) {
    scene.remove(mesh);
    disposeMeshTree(mesh);
    state.meshes.delete(entity.id);
    mesh = null;
  }

  // Swap / upgrade procedural Patriot batteries.
  if (
    mesh &&
    (entity.kind === "turret" ||
      entity.kind === "stinger_site" ||
      entity.kind === "gatling_cannon") &&
    (!mesh.userData.isPatriot || (mesh.userData.patriotRigVersion || 0) < PATRIOT_RIG_VERSION)
  ) {
    scene.remove(mesh);
    disposeMeshTree(mesh);
    state.meshes.delete(entity.id);
    mesh = null;
  }

  if (
    mesh &&
    entity.kind === "bunker" &&
    (!mesh.userData.isBunker || (mesh.userData.bunkerRigVersion || 0) < 2)
  ) {
    scene.remove(mesh);
    disposeMeshTree(mesh);
    state.meshes.delete(entity.id);
    mesh = null;
  }

  if (
    mesh &&
    entity.kind === "radar" &&
    (!mesh.userData.isRadar || (mesh.userData.radarRigVersion || 0) < 2)
  ) {
    scene.remove(mesh);
    disposeMeshTree(mesh);
    state.meshes.delete(entity.id);
    mesh = null;
  }

  // Refit buildings after scale pass.
  if (
    mesh &&
    entity.building &&
    !mesh.userData.isFallback &&
    (mesh.userData.buildingFitVersion || 0) < BUILDING_FIT_VERSION &&
    buildingHasProperModel(entity.kind)
  ) {
    scene.remove(mesh);
    disposeMeshTree(mesh);
    state.meshes.delete(entity.id);
    mesh = null;
  }

  // Upgrade old unit boxes / static infantry to walk-capable / tank turret rigs.
  if (mesh && entity.unit) {
    const kind = String(entity.kind || "");
    const isMlrs =
      kind.includes("mlrs") ||
      kind.includes("tomahawk") ||
      kind.includes("inferno") ||
      kind.includes("scud");
    const isAir = isAirUnitKind(kind);
    const isHeavy = isHeavyTankKind(kind);
    const isTank =
      !isMlrs &&
      !isAir &&
      (kind.includes("tank") ||
        kind.includes("battlemaster") ||
        kind.includes("scorpion") ||
        kind.includes("quad_cannon") ||
        kind.includes("microwave") ||
        kind.includes("humvee") ||
        kind.includes("technical") ||
        kind.includes("buggy") ||
        kind.includes("radar_van"));
    const isMortar =
      kind.includes("mortar") ||
      kind.includes("missile_defender") ||
      kind.includes("tank_hunter") ||
      kind.includes("rpg");
    const needsWalkRig =
      !mesh.userData.isUnitRig ||
      (isAir && !mesh.userData.isAir) ||
      (isMlrs &&
        (!mesh.userData.isMlrs || (mesh.userData.mlrsRigVersion || 0) < 1)) ||
      (isTank &&
        (!mesh.userData.isTank ||
          !mesh.getObjectByName("tankBarrel") ||
          (mesh.userData.tankRigVersion || 0) < 7 ||
          (isHeavy && !mesh.userData.isAbrams) ||
          (!isHeavy && mesh.userData.isAbrams))) ||
      (isMortar && !mesh.userData.isMortar) ||
      (!isTank &&
        !isMlrs &&
        !isMortar &&
        !isAir &&
        (!mesh.userData.isInfantry || (mesh.userData.rigVersion || 0) < 5));
    if (needsWalkRig) {
      Sfx.stopEngine(entity.id);
      scene.remove(mesh);
      disposeMeshTree(mesh);
      state.meshes.delete(entity.id);
      mesh = null;
    }
  }

  if (!mesh) {
    if (entity.building && !buildingHasProperModel(entity.kind) && !buildingModelsReady) {
      return;
    }

    const mat = new THREE.MeshStandardMaterial({
      color: colors[0],
      metalness: 0.15,
      roughness: 0.72,
    });

    if (entity.building) {
      mesh = createBuildingMesh(entity.kind, mat);
    } else {
      mesh = createUnitMesh(entity.kind, colors[0]);
    }

    mesh.userData.id = entity.id;
    mesh.userData.building = !!entity.building;
    mesh.userData.unit = !!entity.unit;
    mesh.userData.kind = entity.kind;
    scene.add(mesh);
    state.meshes.set(entity.id, mesh);

    attachOwnerMarkings(mesh, entity);
    if (entity.unit && state.selectedUnits.includes(entity.id)) {
      syncSelectionMarkers();
    }
  }

  if (mesh.userData.isInfantry) {
    mesh.userData.wantProne = !!entity.prone;
  }

  if (mesh.userData.building) {
    mesh.position.set(entity.x, 0, entity.y);
    if (mesh.userData.isPatriot || mesh.userData.isBunker) {
      mesh.userData.aimAt = entity.aim_at || null;
      if (entity.aim_yaw != null && Number.isFinite(entity.aim_yaw)) {
        mesh.userData.aimYawTarget = entity.aim_yaw;
      }
    }
  } else if (mesh.userData.isUnitRig) {
    applyUnitMotion(mesh, entity);
    if (mesh.userData.isAir || isAirUnitKind(entity.kind)) {
      mesh.position.y = 0.55;
    }
    if (mesh.userData.lastTint !== colors[0]) {
      tintUnitMesh(mesh, colors);
      mesh.userData.lastTint = colors[0];
    }
  } else {
    mesh.position.set(entity.x, unitDims(entity.kind).h * 0.5, entity.y);
  }

  const syncKey = [
    Math.round(entity.hp || 0),
    entity.progress == null ? "-" : Math.round(entity.progress * 100),
    entity.train_progress == null ? "-" : Math.round(entity.train_progress * 100),
    colors[0],
  ].join("|");
  if (mesh.userData.syncKey === syncKey) return;
  mesh.userData.syncKey = syncKey;

  const constructing = entity.progress != null && entity.progress < 1;
  const opacity = constructing ? 0.55 : 1;
  if (mesh.userData.keepMtlColors) {
    if (mesh.userData.lastOpacity !== opacity) {
      setBuildingOpacity(mesh, opacity);
      mesh.userData.lastOpacity = opacity;
    }
  } else if (mesh.material) {
    mesh.material.color.setHex(colors[0]);
    mesh.material.opacity = opacity;
    mesh.material.transparent = opacity < 1;
    mesh.material.depthWrite = opacity >= 1;
  }

  updateProgressBar(mesh, entity);
  updateHpBar(mesh, entity);
  if (entity.building && !constructing) {
    syncBuildingDamageFire(mesh, entity);
  } else if (entity.building && constructing) {
    clearDamageFire(mesh);
  }

  // Refresh label if owner name/colors changed (rare).
  const label = mesh.userData.ownerLabel;
  if (label) {
    const key = `${entity.owner_name}|${colors.join(",")}`;
    if (mesh.userData.labelKey !== key) {
      mesh.remove(label);
      label.material.map?.dispose();
      label.material.dispose();
      const sprite = makeNameSprite(entity.owner_name || "Player", colors);
      if (!entity.building) {
        const tank =
          String(entity.kind || "").includes("tank") ||
          String(entity.kind || "").includes("mlrs");
        sprite.scale.set(tank ? 0.32 : 0.28, tank ? 0.085 : 0.07, 1);
      }
      sprite.position.set(0, labelHeightFor(entity), 0);
      sprite.name = "ownerLabel";
      mesh.add(sprite);
      mesh.userData.ownerLabel = sprite;
      mesh.userData.labelKey = key;
    }
  }
}

function rebuildMeshes() {
  for (const entity of state.entities.values()) {
    upsertMesh(entity);
  }
}

/* ---------- Combat FX ---------- */

const activeFx = [];

function worldMuzzlePoint(mesh, name = "muzzle") {
  const tip = mesh?.getObjectByName?.(name);
  if (tip) {
    const p = new THREE.Vector3();
    tip.getWorldPosition(p);
    return p;
  }
  return new THREE.Vector3(
    mesh.position.x,
    (mesh.userData.unitHeight || 0.15) * 0.7,
    mesh.position.z,
  );
}

function faceMeshToward(mesh, x1, z1) {
  if (!mesh) return;
  const dx = x1 - mesh.position.x;
  const dz = z1 - mesh.position.z;
  if (dx * dx + dz * dz < 1e-6) return;
  const yaw = Math.atan2(dx, dz);
  if (mesh.userData.isPatriot) {
    // Soft target only — smoothPatriotFacing slews the tubes.
    mesh.userData.aimYawTarget = yaw;
    mesh.userData.aimAt = mesh.userData.aimAt || "shot";
    return;
  }
  if (mesh.userData.isBunker) {
    mesh.userData.aimYawTarget = yaw;
    mesh.userData.aimAt = mesh.userData.aimAt || "shot";
    return;
  }
  if (mesh.userData.building) return;
  if (mesh.userData.isTank) {
    // Aim with the turret; hull stays on its travel heading.
    mesh.userData.aimYaw = yaw;
  } else {
    mesh.userData.faceYaw = yaw;
    // Infantry snap onto the shot line quickly.
    mesh.rotation.y = yaw;
  }
}

function shortestAngle(from, to) {
  let diff = to - from;
  while (diff > Math.PI) diff -= Math.PI * 2;
  while (diff < -Math.PI) diff += Math.PI * 2;
  return diff;
}

function applyUnitMotion(mesh, entity) {
  if (mesh.userData.knock) {
    mesh.userData.knock.originX = entity.x;
    mesh.userData.knock.originZ = entity.y;
    return;
  }
  const now = performance.now();
  const kind = String(entity.kind || mesh.userData.kind || "");
  mesh.userData.moveSpeed = kind.includes("tank")
    ? 0.58
    : kind.includes("mlrs")
      ? 0.42
      : kind.includes("mortar") || kind.includes("missile")
        ? 0.14
        : 0.2;
  const prevX = mesh.userData.lastX;
  const prevZ = mesh.userData.lastZ;
  const prevAt = mesh.userData.snapAt;
  if (prevX == null || prevZ == null) {
    mesh.position.set(entity.x, mesh.position.y, entity.y);
    mesh.userData.velX = 0;
    mesh.userData.velZ = 0;
  } else if (prevAt) {
    const dtNet = Math.max(0.05, (now - prevAt) / 1000);
    const dx = entity.x - prevX;
    const dz = entity.y - prevZ;
    const dist = Math.hypot(dx, dz);
    if (dist > 0.004) {
      let nvx = dx / dtNet;
      let nvz = dz / dtNet;
      const sp = Math.hypot(nvx, nvz);
      const maxS = mesh.userData.moveSpeed * 1.35;
      if (sp > maxS) {
        nvx = (nvx / sp) * maxS;
        nvz = (nvz / sp) * maxS;
      }
      mesh.userData.velX = (mesh.userData.velX || 0) * 0.4 + nvx * 0.6;
      mesh.userData.velZ = (mesh.userData.velZ || 0) * 0.4 + nvz * 0.6;
      mesh.userData.moving = true;
      mesh.userData.faceYaw = Math.atan2(dx, dz);
      mesh.userData.moveSeenAt = now;
      mesh.userData.lastMoveDist = dist;
    } else {
      mesh.userData.velX = (mesh.userData.velX || 0) * 0.28;
      mesh.userData.velZ = (mesh.userData.velZ || 0) * 0.28;
      if (Math.hypot(mesh.userData.velX, mesh.userData.velZ) < 0.025) {
        mesh.userData.velX = 0;
        mesh.userData.velZ = 0;
      }
    }
  }
  mesh.userData.destX = entity.x;
  mesh.userData.destZ = entity.y;
  mesh.userData.snapAt = now;
  mesh.userData.lastX = entity.x;
  mesh.userData.lastZ = entity.y;
  if (mesh.userData.isTank) {
    mesh.userData.aimAt = entity.aim_at || null;
  }
}

function predictedPos(mesh) {
  const age = Math.min(0.18, (performance.now() - (mesh.userData.snapAt || performance.now())) / 1000);
  return [
    (mesh.userData.destX || 0) + (mesh.userData.velX || 0) * age,
    (mesh.userData.destZ || 0) + (mesh.userData.velZ || 0) * age,
  ];
}

function slideToward(mesh, dt) {
  if (mesh.userData.destX == null || mesh.userData.destZ == null) return 0;
  const [px, pz] = predictedPos(mesh);
  const dx = px - mesh.position.x;
  const dz = pz - mesh.position.z;
  const dist = Math.hypot(dx, dz);
  if (dist < 1e-5) return 0;
  const cruise = mesh.userData.moveSpeed || 0.22;
  const catchup = dist / 0.4;
  const speed = Math.min(cruise * 2.1, Math.max(cruise, catchup));
  const step = Math.min(dist, speed * dt);
  mesh.position.x += (dx / dist) * step;
  mesh.position.z += (dz / dist) * step;
  return dist;
}

function updateTankDrive(mesh, dt) {
  if (!mesh?.userData?.isTank || mesh.userData.knock) return;
  const id = mesh.userData.id;
  if (mesh.userData.destX == null) {
    mesh.userData.velX = 0;
    mesh.userData.velZ = 0;
    mesh.userData.moving = false;
    Sfx.stopEngine(id);
    return;
  }
  const beforeX = mesh.position.x;
  const beforeZ = mesh.position.z;
  slideToward(mesh, dt);
  const step = Math.hypot(mesh.position.x - beforeX, mesh.position.z - beforeZ);
  const now = performance.now();
  if (step < 0.00035) {
    mesh.userData.stillFrames = (mesh.userData.stillFrames || 0) + 1;
  } else {
    mesh.userData.stillFrames = 0;
    mesh.userData.engineHeardAt = now;
  }
  if (mesh.userData.stillFrames > 6) {
    mesh.userData.velX = 0;
    mesh.userData.velZ = 0;
    mesh.userData.moving = false;
    Sfx.stopEngine(id);
    return;
  }

  const speed = Math.hypot(mesh.userData.velX || 0, mesh.userData.velZ || 0);
  if (speed > 0.04) {
    mesh.userData.faceYaw = Math.atan2(mesh.userData.velX, mesh.userData.velZ);
    mesh.userData.moving = true;
    const spin = step * 28;
    if (spin > 0.0002) {
      mesh.traverse((obj) => {
        if (obj.userData?.roadWheel) obj.rotation.x += spin;
      });
    }
  }

  // Palet sesi sadece seçili tank hareket ederken — tüm harita gürültü yapmasın.
  const selected = state.selectedUnits.includes(id);
  if (selected && now - (mesh.userData.engineHeardAt || 0) < 90) {
    Sfx.setEngine(id, mesh.position.x, mesh.position.z, Math.min(1, 0.45 + speed));
  } else {
    Sfx.stopEngine(id);
  }
}

function smoothPatriotFacing(mesh, dt) {
  if (!mesh?.userData?.isPatriot && !mesh?.userData?.isBunker) return;
  const launcher = mesh.getObjectByName("muzzleRoot");
  const radar = mesh.getObjectByName("radarRoot");
  const rate = mesh.userData.turretTurnRate || 0.95;
  const scan = mesh.userData.scanRate || 0.55;
  const tid = mesh.userData.aimAt;

  if (!tid || tid === "shot") {
    // Idle / post-shot: keep sweeping the sector.
    if (!tid) {
      mesh.userData.aimYaw = (mesh.userData.aimYaw || 0) + scan * dt;
    } else if (mesh.userData.aimYawTarget != null) {
      const cur = mesh.userData.aimYaw || 0;
      const diff = shortestAngle(cur, mesh.userData.aimYawTarget);
      const step = rate * dt;
      mesh.userData.aimYaw = cur + Math.max(-step, Math.min(step, diff));
      if (Math.abs(diff) < 0.05) mesh.userData.aimAt = null;
    }
    if (launcher) launcher.rotation.y = mesh.userData.aimYaw || 0;
    if (radar) radar.rotation.y = (radar.rotation.y || 0) + scan * 1.7 * dt;
    return;
  }

  let desired = mesh.userData.aimYawTarget;
  const ent = state.entities.get(tid);
  const other = state.meshes.get(tid);
  const tx = ent ? ent.x : other?.position.x;
  const tz = ent ? ent.y : other?.position.z;
  if (tx != null && tz != null) {
    desired = Math.atan2(tx - mesh.position.x, tz - mesh.position.z);
    mesh.userData.aimYawTarget = desired;
  }
  if (desired == null && mesh.userData.aimYawTarget != null) {
    desired = mesh.userData.aimYawTarget;
  }
  if (desired == null) return;

  const cur = mesh.userData.aimYaw || 0;
  const diff = shortestAngle(cur, desired);
  const step = rate * dt;
  mesh.userData.aimYaw = cur + Math.max(-step, Math.min(step, diff));
  if (launcher) launcher.rotation.y = mesh.userData.aimYaw;
  if (radar) {
    const radCur = radar.rotation.y || 0;
    const rDiff = shortestAngle(radCur, desired + 0.12);
    radar.rotation.y = radCur + Math.max(-scan * 1.8 * dt, Math.min(scan * 1.8 * dt, rDiff));
  }
}

function smoothUnitFacing(mesh, dt) {
  if (!mesh?.userData?.isUnitRig) return;

  if (mesh.userData.isTank) {
    const hullRate = mesh.userData.hullTurnRate || 2.0;
    const turretRate = mesh.userData.turretTurnRate || 1.35;
    const turret = mesh.getObjectByName("muzzleRoot");

    // Hull slowly follows travel direction.
    if (mesh.userData.faceYaw != null) {
      const diff = shortestAngle(mesh.rotation.y, mesh.userData.faceYaw);
      const maxStep = hullRate * dt;
      mesh.rotation.y += Math.max(-maxStep, Math.min(maxStep, diff));
    }

    // Turret independently tracks aim (or hull heading if no aim yet).
    if (turret) {
      const tid = mesh.userData.aimAt;
      if (tid) {
        const ent = state.entities.get(tid);
        const other = state.meshes.get(tid);
        const tx = ent ? ent.x : other?.position.x;
        const tz = ent ? ent.y : other?.position.z;
        if (tx != null && tz != null) {
          mesh.userData.aimYaw = Math.atan2(tx - mesh.position.x, tz - mesh.position.z);
        }
      }
      const aim =
        mesh.userData.aimYaw != null ? mesh.userData.aimYaw : mesh.userData.faceYaw;
      if (aim != null) {
        const desiredLocal = shortestAngle(0, aim - mesh.rotation.y);
        const cur = turret.rotation.y;
        const diff = shortestAngle(cur, desiredLocal);
        const maxStep = turretRate * dt;
        turret.rotation.y = cur + Math.max(-maxStep, Math.min(maxStep, diff));
      }
      const mg = turret.getObjectByName("tankMg");
      if (mg) {
        const mgAim =
          mesh.userData.mgAimYaw != null ? mesh.userData.mgAimYaw : aim;
        if (mgAim != null) {
          const desiredLocal = shortestAngle(0, mgAim - mesh.rotation.y - turret.rotation.y);
          const cur = mg.rotation.y;
          const diff = shortestAngle(cur, desiredLocal);
          mg.rotation.y = cur + Math.max(-2.8 * dt, Math.min(2.8 * dt, diff));
        }
      }
    }
    return;
  }

  if (mesh.userData.faceYaw == null) return;
  const diff = shortestAngle(mesh.rotation.y, mesh.userData.faceYaw);
  const turn = Math.min(1, dt * 12);
  mesh.rotation.y += diff * turn;
}

function updateInfantryDrive(mesh, dt) {
  if (!mesh?.userData?.isInfantry || mesh.userData.knock) return;
  if (mesh.userData.destX == null || mesh.userData.destZ == null) return;
  slideToward(mesh, dt);
  const speed = Math.hypot(mesh.userData.velX || 0, mesh.userData.velZ || 0);
  if (speed > 0.03) {
    mesh.userData.faceYaw = Math.atan2(mesh.userData.velX, mesh.userData.velZ);
    mesh.userData.moving = true;
  } else if (performance.now() - (mesh.userData.moveSeenAt || 0) > 220) {
    mesh.userData.moving = false;
  }
}

function updateInfantryWalk(mesh, dt, now) {
  if (!mesh?.userData?.isInfantry) return;
  // Far from camera: skip skeletal swing (big win with many soldiers).
  if (camera) {
    const dx = mesh.position.x - camera.position.x;
    const dz = mesh.position.z - camera.position.z;
    if (dx * dx + dz * dz > 55 * 55) {
      mesh.userData.walkLodSkip = true;
      return;
    }
  }
  mesh.userData.walkLodSkip = false;
  let walk = mesh.userData.walk;
  if (!walk?.leftLeg || !walk?.rightLeg) {
    walk = {
      leftLeg: mesh.getObjectByName("leftLeg"),
      rightLeg: mesh.getObjectByName("rightLeg"),
      leftArm: mesh.getObjectByName("leftArm"),
      rightArm: mesh.getObjectByName("rightArm"),
      torso: mesh.getObjectByName("torso"),
    };
    mesh.userData.walk = walk;
  }
  const { leftLeg, rightLeg, leftArm, rightArm, torso } = walk;
  if (!leftLeg || !rightLeg) return;

  // Drop / stand — lerp so it doesn't pop.
  const want = mesh.userData.wantProne ? 1 : 0;
  let blend = mesh.userData.proneBlend || 0;
  const dropRate = want > blend ? 7 : 5;
  blend += (want - blend) * Math.min(1, dt * dropRate);
  if (Math.abs(blend - want) < 0.01) blend = want;
  mesh.userData.proneBlend = blend;
  mesh.rotation.order = "YXZ";
  mesh.rotation.x = blend * 1.28;
  if (!mesh.userData.knock) {
    mesh.position.y = blend * 0.022;
  }

  // Keep the cycle alive across 10 Hz snapshots and brief packet jitter.
  const lastDist = mesh.userData.lastMoveDist || 0;
  const recentlyMoved =
    mesh.userData.moving === true ||
    Math.hypot(mesh.userData.velX || 0, mesh.userData.velZ || 0) > 0.03 ||
    (lastDist > 0.008 &&
      mesh.userData.moveSeenAt != null &&
      mesh.userData.moveSeenAt > 0 &&
      now - mesh.userData.moveSeenAt < 280);

  if (recentlyMoved) {
    const crawl = 0.22 + (1 - blend) * 0.78;
    mesh.userData.walkPhase = (mesh.userData.walkPhase || 0) + dt * (8 + 6 * crawl);
    const swing = Math.sin(mesh.userData.walkPhase) * 0.55 * crawl;
    leftLeg.rotation.x = swing;
    rightLeg.rotation.x = -swing;
    if (leftArm) leftArm.rotation.x = -swing * 0.45;
    if (rightArm) rightArm.rotation.x = swing * 0.35;
    if (torso) {
      torso.position.y = Math.abs(Math.sin(mesh.userData.walkPhase * 2)) * 0.008 * crawl;
      torso.rotation.z = Math.sin(mesh.userData.walkPhase) * 0.04 * crawl;
    }
  } else {
    leftLeg.rotation.x = 0;
    rightLeg.rotation.x = 0;
    if (leftArm) leftArm.rotation.x = blend * 0.55;
    if (rightArm) rightArm.rotation.x = blend * -0.15;
    if (torso) {
      torso.position.y = 0;
      torso.rotation.z = 0;
    }
  }
}

function spawnShotFx(shot) {
  if (!scene) return;
  const fromMesh = state.meshes.get(shot.from);
  const toMesh = state.meshes.get(shot.to);
  const didHit = shot.hit !== false;
  const kind = String(shot.kind || fromMesh?.userData?.kind || "");
  const isTankMg = kind.includes("mg") && !!fromMesh?.userData?.isTank && !fromMesh?.userData?.isMlrs;
  const isBunkerMg = kind.includes("bunker");
  const isMlrs = kind.includes("mlrs") || !!fromMesh?.userData?.isMlrs;
  const isTankCannon = kind.includes("tank") && !kind.includes("mg") && !isMlrs;
  const isMortar = kind.includes("mortar");
  const isMissile =
    !isMortar &&
    !isBunkerMg &&
    !isMlrs &&
    (kind.includes("missile") || kind.includes("patriot") || kind === "turret");

  if (isTankMg && fromMesh) {
    const dx = shot.x1 - fromMesh.position.x;
    const dz = shot.y1 - fromMesh.position.z;
    if (dx * dx + dz * dz > 1e-6) {
      fromMesh.userData.mgAimYaw = Math.atan2(dx, dz);
    }
  } else {
    faceMeshToward(fromMesh, shot.x1, shot.y1);
  }

  const start = fromMesh
    ? worldMuzzlePoint(
        fromMesh,
        isTankMg && fromMesh.getObjectByName("mgMuzzle") ? "mgMuzzle" : "muzzle",
      )
    : new THREE.Vector3(shot.x0, isMortar ? 0.1 : isBunkerMg ? 0.12 : isMlrs ? 0.22 : 0.12, shot.y0);
  // Always use server impact point so misses fly wide of the mesh.
  const endY = didHit
    ? (toMesh?.userData?.unitHeight || (toMesh?.userData?.building ? 0.6 : 0.12) || 0.12) * 0.55
    : 0.04;
  const end = new THREE.Vector3(shot.x1, endY, shot.y1);

  const dir = new THREE.Vector3().subVectors(end, start);
  const dist = Math.max(0.05, dir.length());
  dir.normalize();

  const now = performance.now();
  const fx = {
    type: isTankCannon
      ? "shell"
      : isMlrs
        ? "mlrs"
        : isMortar
          ? "mortar"
          : isMissile
            ? "missile"
            : "bullet",
    born: now,
    life: isTankCannon ? 380 : isMlrs ? 720 : isMortar ? 780 : isMissile ? 520 : 90,
    start: start.clone(),
    end: end.clone(),
    dir: dir.clone(),
    dist,
    arc: isMortar ? Math.max(0.45, dist * 0.28) : isMlrs ? Math.max(0.55, dist * 0.18) : 0,
    fromMesh: fromMesh || null,
    hit: didHit,
    parts: [],
  };

  if (isMlrs) {
    // Exhaust bloom from the pod face
    const blast = new THREE.Mesh(
      new THREE.SphereGeometry(0.045, 8, 8),
      new THREE.MeshBasicMaterial({
        color: 0xffaa55,
        transparent: true,
        opacity: 0.95,
        depthWrite: false,
      }),
    );
    blast.position.copy(start);
    scene.add(blast);
    fx.parts.push({ mesh: blast, role: "blast" });

    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.05, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x9a9688,
        transparent: true,
        opacity: 0.7,
        depthWrite: false,
      }),
    );
    smoke.position.copy(start).addScaledVector(dir, -0.03);
    scene.add(smoke);
    fx.parts.push({ mesh: smoke, role: "smoke" });

    // Thick artillery rocket body + darker nose
    const rocket = new THREE.Group();
    const body = new THREE.Mesh(
      new THREE.CylinderGeometry(0.014, 0.016, 0.12, 7),
      new THREE.MeshBasicMaterial({ color: 0xb8a878 }),
    );
    const nose = new THREE.Mesh(
      new THREE.ConeGeometry(0.014, 0.04, 7),
      new THREE.MeshBasicMaterial({ color: 0x3a3828 }),
    );
    nose.position.y = 0.08;
    const finMat = new THREE.MeshBasicMaterial({ color: 0x2a2820 });
    for (let f = 0; f < 4; f++) {
      const fin = new THREE.Mesh(new THREE.BoxGeometry(0.004, 0.03, 0.022), finMat);
      const a = (f / 4) * Math.PI * 2;
      fin.position.set(Math.cos(a) * 0.016, -0.05, Math.sin(a) * 0.016);
      rocket.add(fin);
    }
    rocket.add(body);
    rocket.add(nose);
    rocket.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
    rocket.position.copy(start);
    scene.add(rocket);
    fx.parts.push({ mesh: rocket, role: "projectile" });

    // Exhaust trail puff
    const trail = new THREE.Mesh(
      new THREE.SphereGeometry(0.02, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xc8c4b0,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
      }),
    );
    trail.position.copy(start);
    scene.add(trail);
    fx.parts.push({ mesh: trail, role: "ring" });

    if (fromMesh) {
      fromMesh.userData.hullKick = 1;
      const elev = fromMesh.getObjectByName("mlrsElev");
      if (elev) elev.userData.kick = 1;
    }
  } else if (isTankCannon) {
    // Heavy muzzle blast
    const blast = new THREE.Mesh(
      new THREE.SphereGeometry(0.05, 8, 8),
      new THREE.MeshBasicMaterial({
        color: 0xffcc66,
        transparent: true,
        opacity: 1,
        depthWrite: false,
      }),
    );
    blast.position.copy(start);
    scene.add(blast);
    fx.parts.push({ mesh: blast, role: "blast" });

    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.04, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x9a9a88,
        transparent: true,
        opacity: 0.65,
        depthWrite: false,
      }),
    );
    smoke.position.copy(start).addScaledVector(dir, 0.04);
    scene.add(smoke);
    fx.parts.push({ mesh: smoke, role: "smoke" });

    // Visible AP shell
    const shell = new THREE.Mesh(
      new THREE.CylinderGeometry(0.012, 0.016, 0.07, 6),
      new THREE.MeshBasicMaterial({ color: 0xffdd88 }),
    );
    shell.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
    shell.position.copy(start);
    scene.add(shell);
    fx.parts.push({ mesh: shell, role: "projectile" });

    // Barrel kick + slight hull shudder
    if (fromMesh) {
      fromMesh.userData.barrelRecoil = 1;
      fromMesh.userData.hullKick = 1;
      const barrel = fromMesh.getObjectByName("tankBarrel");
      if (barrel) barrel.position.z = -0.055;
    }
  } else if (isMortar) {
    const puff = new THREE.Mesh(
      new THREE.SphereGeometry(0.028, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xc8b890,
        transparent: true,
        opacity: 0.8,
        depthWrite: false,
      }),
    );
    puff.position.copy(start);
    scene.add(puff);
    fx.parts.push({ mesh: puff, role: "blast" });

    const bomb = new THREE.Mesh(
      new THREE.SphereGeometry(0.016, 7, 7),
      new THREE.MeshBasicMaterial({ color: 0x3a3828 }),
    );
    bomb.position.copy(start);
    scene.add(bomb);
    fx.parts.push({ mesh: bomb, role: "projectile" });

    const fin = new THREE.Mesh(
      new THREE.CylinderGeometry(0.004, 0.01, 0.028, 5),
      new THREE.MeshBasicMaterial({ color: 0x5a5848 }),
    );
    fin.position.copy(start);
    scene.add(fin);
    fx.parts.push({ mesh: fin, role: "tracer" });
  } else if (isMissile) {
    const isPatriot = kind.includes("patriot") || kind === "turret";
    if (isPatriot) {
      // MIM-104 style: white body, ogive nose, cruciform fins, boost plume + trail.
      fx.life = Math.min(1100, 420 + dist * 55);
      fx.arc = Math.max(0.55, dist * 0.18);
      fx.type = "patriot";

      const launchSmoke = new THREE.Mesh(
        new THREE.SphereGeometry(0.04, 8, 8),
        new THREE.MeshBasicMaterial({
          color: 0xd8d0c0,
          transparent: true,
          opacity: 0.85,
          depthWrite: false,
        }),
      );
      launchSmoke.position.copy(start);
      scene.add(launchSmoke);
      fx.parts.push({ mesh: launchSmoke, role: "blast" });

      const missile = new THREE.Group();
      const body = new THREE.Mesh(
        new THREE.CylinderGeometry(0.009, 0.011, 0.11, 8),
        new THREE.MeshBasicMaterial({ color: 0xf2f0e8 }),
      );
      body.position.y = 0.02;
      missile.add(body);
      const band = new THREE.Mesh(
        new THREE.CylinderGeometry(0.0115, 0.0115, 0.012, 8),
        new THREE.MeshBasicMaterial({ color: 0x2a2c28 }),
      );
      band.position.y = -0.01;
      missile.add(band);
      const nose = new THREE.Mesh(
        new THREE.ConeGeometry(0.009, 0.032, 8),
        new THREE.MeshBasicMaterial({ color: 0xe8e4d8 }),
      );
      nose.position.y = 0.09;
      missile.add(nose);
      for (let i = 0; i < 4; i++) {
        const fin = new THREE.Mesh(
          new THREE.BoxGeometry(0.002, 0.028, 0.018),
          new THREE.MeshBasicMaterial({ color: 0xc8c4b8 }),
        );
        const ang = (i * Math.PI) / 2;
        fin.position.set(Math.cos(ang) * 0.012, -0.028, Math.sin(ang) * 0.012);
        fin.rotation.y = ang;
        missile.add(fin);
      }
      missile.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
      missile.position.copy(start);
      scene.add(missile);
      fx.parts.push({ mesh: missile, role: "projectile" });

      const plume = new THREE.Mesh(
        new THREE.SphereGeometry(0.018, 6, 6),
        new THREE.MeshBasicMaterial({
          color: 0xff8833,
          transparent: true,
          opacity: 0.95,
          depthWrite: false,
        }),
      );
      plume.position.copy(start).addScaledVector(dir, -0.04);
      scene.add(plume);
      fx.parts.push({ mesh: plume, role: "smoke" });

      const trail = new THREE.Mesh(
        new THREE.SphereGeometry(0.014, 6, 6),
        new THREE.MeshBasicMaterial({
          color: 0xbbb8a8,
          transparent: true,
          opacity: 0.55,
          depthWrite: false,
        }),
      );
      trail.position.copy(start);
      scene.add(trail);
      fx.parts.push({ mesh: trail, role: "ring" });
    } else {
      const smoke = new THREE.Mesh(
        new THREE.SphereGeometry(0.03, 6, 6),
        new THREE.MeshBasicMaterial({
          color: 0xaaccee,
          transparent: true,
          opacity: 0.75,
          depthWrite: false,
        }),
      );
      smoke.position.copy(start);
      scene.add(smoke);
      fx.parts.push({ mesh: smoke, role: "blast" });

      const rocket = new THREE.Mesh(
        new THREE.CylinderGeometry(0.01, 0.014, 0.08, 6),
        new THREE.MeshBasicMaterial({ color: 0x88ddff }),
      );
      rocket.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
      rocket.position.copy(start);
      scene.add(rocket);
      fx.parts.push({ mesh: rocket, role: "projectile" });
    }
  } else {
    // Rifle: brief muzzle flash + fast thin tracer bullet
    const flash = new THREE.Mesh(
      new THREE.SphereGeometry(0.018, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xfff6c8,
        transparent: true,
        opacity: 1,
        depthWrite: false,
      }),
    );
    flash.position.copy(start);
    scene.add(flash);
    fx.parts.push({ mesh: flash, role: "blast" });

    const tip = start.clone().addScaledVector(dir, Math.min(0.55, dist * 0.35));
    const tracerGeo = new THREE.BufferGeometry().setFromPoints([start, tip]);
    const tracer = new THREE.Line(
      tracerGeo,
      new THREE.LineBasicMaterial({
        color: 0xffe8a0,
        transparent: true,
        opacity: 0.95,
        depthWrite: false,
      }),
    );
    scene.add(tracer);
    fx.parts.push({ mesh: tracer, role: "tracer", tipLen: Math.min(0.55, dist * 0.35) });

    const bullet = new THREE.Mesh(
      new THREE.SphereGeometry(0.008, 5, 5),
      new THREE.MeshBasicMaterial({ color: 0xfff0b0 }),
    );
    bullet.position.copy(start);
    scene.add(bullet);
    fx.parts.push({ mesh: bullet, role: "projectile" });
  }

  activeFx.push(fx);
}

function disposeFxPart(part) {
  if (!part?.mesh) return;
  scene?.remove(part.mesh);
  part.mesh.traverse?.((obj) => {
    obj.geometry?.dispose?.();
    if (obj.material) {
      if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose?.());
      else obj.material.dispose?.();
    }
  });
  if (!part.mesh.traverse) {
    part.mesh.geometry?.dispose?.();
    if (part.mesh.material) {
      if (Array.isArray(part.mesh.material)) part.mesh.material.forEach((m) => m.dispose?.());
      else part.mesh.material.dispose?.();
    }
  }
}

function updateCombatFx(now) {
  // Tank barrel spring-back + hull kick settle
  for (const mesh of state.meshes.values()) {
    if (!mesh.userData?.isTank) continue;
    const barrel = mesh.getObjectByName("tankBarrel");
    if (mesh.userData.barrelRecoil > 0) {
      mesh.userData.barrelRecoil = Math.max(0, mesh.userData.barrelRecoil - 0.045);
      if (barrel) {
        barrel.position.z = -0.055 * mesh.userData.barrelRecoil;
      }
    } else if (barrel && barrel.position.z !== 0) {
      barrel.position.z *= 0.7;
      if (Math.abs(barrel.position.z) < 0.001) barrel.position.z = 0;
    }
    if (mesh.userData.hullKick > 0) {
      mesh.userData.hullKick = Math.max(0, mesh.userData.hullKick - 0.06);
      if (mesh.userData.isMlrs) {
        const elev = mesh.getObjectByName("mlrsElev");
        if (elev) {
          elev.rotation.x = -0.42 - 0.06 * mesh.userData.hullKick;
        }
      } else {
        // Visual shudder: tiny pitch on turret
        const turret = mesh.getObjectByName("muzzleRoot");
        if (turret) {
          turret.rotation.x = -0.08 * mesh.userData.hullKick;
        }
      }
    } else if (!mesh.userData.isMlrs) {
      const turret = mesh.getObjectByName("muzzleRoot");
      if (turret && turret.rotation.x) {
        turret.rotation.x *= 0.75;
        if (Math.abs(turret.rotation.x) < 0.001) turret.rotation.x = 0;
      }
    } else {
      const elev = mesh.getObjectByName("mlrsElev");
      if (elev && elev.rotation.x < -0.42) {
        elev.rotation.x += (-0.42 - elev.rotation.x) * 0.15;
      }
    }
  }

  for (let i = activeFx.length - 1; i >= 0; i--) {
    const fx = activeFx[i];
    const t = Math.min(1, (now - fx.born) / fx.life);
    const pos = fx.start.clone().lerp(fx.end, t);
    if ((fx.type === "mortar" || fx.type === "patriot" || fx.type === "mlrs") && fx.arc) {
      // Patriot: boost loft early then flatten onto the intercept.
      // MLRS rockets: ballistic loft then flatten into the beaten zone.
      const loft =
        fx.type === "patriot"
          ? 4 * t * (1 - t) * (1 - t * 0.35)
          : fx.type === "mlrs"
            ? 4 * t * (1 - t) * (0.85 + t * 0.15)
            : 4 * t * (1 - t);
      pos.y += loft * fx.arc;
    }

    for (const part of fx.parts) {
      if (part.role === "projectile") {
        part.mesh.position.copy(pos);
        if (fx.type === "mortar" || fx.type === "patriot" || fx.type === "mlrs") {
          const t2 = Math.min(1, t + 0.025);
          const next = fx.start.clone().lerp(fx.end, t2);
          const loft2 =
            fx.type === "patriot"
              ? 4 * t2 * (1 - t2) * (1 - t2 * 0.35)
              : fx.type === "mlrs"
                ? 4 * t2 * (1 - t2) * (0.85 + t2 * 0.15)
                : 4 * t2 * (1 - t2);
          next.y += loft2 * (fx.arc || 0);
          const v = next.sub(pos);
          if (v.lengthSq() > 1e-8) {
            v.normalize();
            part.mesh.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), v);
          }
        }
      } else if (part.role === "tracer") {
        if (fx.type === "mortar") {
          part.mesh.position.copy(pos);
          part.mesh.material.opacity = 0.7 * (1 - t);
        } else {
          const tip = fx.start.clone().addScaledVector(fx.dir, part.tipLen || 0.4);
          const head = pos.clone();
          const tail = head.clone().addScaledVector(fx.dir, -(part.tipLen || 0.4));
          const a = t < 0.15 ? fx.start : tail;
          const b = head;
          part.mesh.geometry.setFromPoints([a, b]);
          part.mesh.geometry.attributes.position.needsUpdate = true;
          part.mesh.material.opacity = 0.95 * (1 - t * 0.5);
        }
      } else if (part.role === "blast") {
        const fade = Math.max(
          0,
          1 -
            t *
              (fx.type === "tank_boom" || fx.type === "patriot_boom" || fx.type === "building_boom"
                ? 1.35
                : 4),
        );
        part.mesh.material.opacity = fade;
        const grow =
          fx.type === "shell" || fx.type === "tank_boom"
            ? 1 + t * 8
            : fx.type === "patriot_boom"
              ? 1 + t * 11
              : fx.type === "building_boom"
                ? 1 + t * 10
                : 1 + t * 3;
        part.mesh.scale.setScalar(grow);
        if (fx.type === "tank_boom" || fx.type === "patriot_boom" || fx.type === "building_boom") {
          part.mesh.position.y =
            fx.start.y +
            t *
              (fx.type === "building_boom" ? 0.55 : fx.type === "patriot_boom" ? 0.35 : 0.25);
        }
      } else if (part.role === "ring") {
        if (fx.type === "patriot") {
          // Exhaust smoke puff trailing the missile.
          part.mesh.position.copy(pos).addScaledVector(fx.dir, -0.06 - t * 0.04);
          part.mesh.material.opacity = 0.5 * (1 - t);
          part.mesh.scale.setScalar(1 + t * 6);
        } else if (fx.type === "patriot_boom" || fx.type === "building_boom") {
          part.mesh.material.opacity = 0.9 * (1 - t);
          const s = 1 + t * (fx.type === "building_boom" ? 12 : 14);
          part.mesh.scale.set(s, s, s);
        } else if (fx.type === "mlrs") {
          part.mesh.position.copy(pos).addScaledVector(fx.dir, -0.06 - t * 0.04);
          part.mesh.material.opacity = 0.5 * (1 - t);
          part.mesh.scale.setScalar(1 + t * 6);
        } else {
          part.mesh.material.opacity = 0.85 * (1 - t);
          const s = 1 + t * 9;
          part.mesh.scale.set(s, s, s);
        }
      } else if (part.role === "debris") {
        const drift = part.mesh.userData.drift;
        if (drift) {
          part.mesh.position.x = fx.start.x + drift.x * t;
          part.mesh.position.y = fx.start.y + drift.y * t * (1 - t * 0.35);
          part.mesh.position.z = fx.start.z + drift.z * t;
          if (fx.type === "building_boom") {
            part.mesh.rotation.x += 0.08;
            part.mesh.rotation.z += 0.06;
          }
        }
        part.mesh.material.opacity = 0.55 * (1 - t);
        part.mesh.scale.setScalar(
          1 + t * (fx.type === "patriot_boom" || fx.type === "building_boom" ? 5.5 : 4),
        );
      } else if (part.role === "smoke") {
        if (fx.type === "patriot" || fx.type === "mlrs") {
          part.mesh.position.copy(pos).addScaledVector(fx.dir, -0.035);
          part.mesh.material.opacity = 0.9 * (1 - t * 0.85);
          part.mesh.scale.setScalar(1 + t * 2.2);
        } else {
          part.mesh.material.opacity = 0.55 * (1 - t);
          part.mesh.scale.setScalar(1 + t * 5);
          part.mesh.position.copy(fx.start).addScaledVector(fx.dir, 0.04 + t * 0.08);
        }
      }
    }

    if (t >= 1) {
      // Impact burst at destination
      if (fx.type === "shell") {
        if (fx.hit !== false) {
          spawnTankExplosion(fx.end);
          applyBlastKnock(fx.end.x, fx.end.z, 1.25);
        } else {
          // Miss: dirt puff only, no HE knock.
          spawnMissImpact(fx.end, true);
        }
      } else if (fx.type === "bullet") {
        spawnMissImpact(fx.end, fx.hit === false);
      } else if (
        fx.type === "missile" ||
        fx.type === "mortar" ||
        fx.type === "patriot" ||
        fx.type === "mlrs"
      ) {
        if (fx.hit !== false) {
          if (fx.type === "patriot") {
            spawnPatriotImpact(fx.end);
            applyBlastKnock(fx.end.x, fx.end.z, 1.45);
          } else {
            spawnTankExplosion(fx.end);
            applyBlastKnock(
              fx.end.x,
              fx.end.z,
              fx.type === "mlrs" ? 1.75 : fx.type === "mortar" ? 0.95 : 0.85,
            );
          }
        } else {
          spawnMissImpact(fx.end, true);
        }
      }

      for (const part of fx.parts) disposeFxPart(part);
      activeFx.splice(i, 1);
    }
  }
}

function spawnMissImpact(at, isMiss) {
  if (!scene) return;
  const now = performance.now();
  const spark = new THREE.Mesh(
    new THREE.SphereGeometry(isMiss ? 0.018 : 0.012, 5, 5),
    new THREE.MeshBasicMaterial({
      color: isMiss ? 0xc2b280 : 0xffcc66,
      transparent: true,
      opacity: isMiss ? 0.7 : 0.85,
      depthWrite: false,
    }),
  );
  spark.position.copy(at);
  if (isMiss) spark.position.y = Math.max(0.02, at.y);
  scene.add(spark);
  activeFx.push({
    type: "impact",
    born: now,
    life: isMiss ? 180 : 120,
    parts: [{ mesh: spark, role: "blast" }],
    start: spark.position.clone(),
    end: spark.position.clone(),
    dir: new THREE.Vector3(0, 1, 0),
    dist: 0,
  });
}

function spawnTankExplosion(at) {
  if (!scene) return;
  const now = performance.now();
  const parts = [];

  const fireball = new THREE.Mesh(
    new THREE.SphereGeometry(0.08, 10, 10),
    new THREE.MeshBasicMaterial({
      color: 0xff7722,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  fireball.position.copy(at);
  scene.add(fireball);
  parts.push({ mesh: fireball, role: "blast" });

  const core = new THREE.Mesh(
    new THREE.SphereGeometry(0.04, 8, 8),
    new THREE.MeshBasicMaterial({
      color: 0xfff0a8,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  core.position.copy(at);
  scene.add(core);
  parts.push({ mesh: core, role: "blast" });

  const ring = new THREE.Mesh(
    new THREE.RingGeometry(0.08, 0.14, 20),
    new THREE.MeshBasicMaterial({
      color: 0xffaa44,
      transparent: true,
      opacity: 0.85,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  ring.rotation.x = -Math.PI / 2;
  ring.position.set(at.x, 0.04, at.z);
  scene.add(ring);
  parts.push({ mesh: ring, role: "ring" });

  for (let i = 0; i < 5; i++) {
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.05 + Math.random() * 0.03, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x6a6558,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
      }),
    );
    const ang = (i / 5) * Math.PI * 2;
    smoke.position.set(at.x + Math.cos(ang) * 0.05, at.y + 0.05, at.z + Math.sin(ang) * 0.05);
    smoke.userData.drift = new THREE.Vector3(Math.cos(ang) * 0.35, 0.55, Math.sin(ang) * 0.35);
    scene.add(smoke);
    parts.push({ mesh: smoke, role: "debris" });
  }

  activeFx.push({
    type: "tank_boom",
    born: now,
    life: 520,
    parts,
    start: at.clone(),
    end: at.clone(),
    dir: new THREE.Vector3(0, 1, 0),
    dist: 0,
  });
}

/** PAC-2/3 style intercept flash — bright white core, shock ring, hot fragments. */
function spawnPatriotImpact(at) {
  if (!scene) return;
  const now = performance.now();
  const parts = [];
  const origin = at.clone();
  origin.y = Math.max(0.06, at.y);

  const flash = new THREE.Mesh(
    new THREE.SphereGeometry(0.11, 12, 12),
    new THREE.MeshBasicMaterial({
      color: 0xffffff,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  flash.position.copy(origin);
  scene.add(flash);
  parts.push({ mesh: flash, role: "blast" });

  const core = new THREE.Mesh(
    new THREE.SphereGeometry(0.055, 10, 10),
    new THREE.MeshBasicMaterial({
      color: 0xa8d8ff,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  core.position.copy(origin);
  scene.add(core);
  parts.push({ mesh: core, role: "blast" });

  const fire = new THREE.Mesh(
    new THREE.SphereGeometry(0.09, 10, 10),
    new THREE.MeshBasicMaterial({
      color: 0xff8844,
      transparent: true,
      opacity: 0.95,
      depthWrite: false,
    }),
  );
  fire.position.copy(origin);
  scene.add(fire);
  parts.push({ mesh: fire, role: "blast" });

  const ring = new THREE.Mesh(
    new THREE.RingGeometry(0.06, 0.2, 28),
    new THREE.MeshBasicMaterial({
      color: 0xd0e8ff,
      transparent: true,
      opacity: 0.9,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  ring.rotation.x = -Math.PI / 2;
  ring.position.set(origin.x, 0.05, origin.z);
  scene.add(ring);
  parts.push({ mesh: ring, role: "ring" });

  const ring2 = new THREE.Mesh(
    new THREE.RingGeometry(0.1, 0.28, 28),
    new THREE.MeshBasicMaterial({
      color: 0xffaa66,
      transparent: true,
      opacity: 0.55,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  ring2.rotation.x = -Math.PI / 2;
  ring2.position.set(origin.x, 0.04, origin.z);
  scene.add(ring2);
  parts.push({ mesh: ring2, role: "ring" });

  for (let i = 0; i < 8; i++) {
    const ang = (i / 8) * Math.PI * 2 + Math.random() * 0.2;
    const spark = new THREE.Mesh(
      new THREE.SphereGeometry(0.018 + Math.random() * 0.012, 5, 5),
      new THREE.MeshBasicMaterial({
        color: i % 2 === 0 ? 0xffeeaa : 0x88ccff,
        transparent: true,
        opacity: 0.95,
        depthWrite: false,
      }),
    );
    spark.position.copy(origin);
    spark.userData.drift = new THREE.Vector3(
      Math.cos(ang) * (0.45 + Math.random() * 0.35),
      0.35 + Math.random() * 0.55,
      Math.sin(ang) * (0.45 + Math.random() * 0.35),
    );
    scene.add(spark);
    parts.push({ mesh: spark, role: "debris" });
  }

  for (let i = 0; i < 4; i++) {
    const ang = (i / 4) * Math.PI * 2;
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.06 + Math.random() * 0.04, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x8a8880,
        transparent: true,
        opacity: 0.5,
        depthWrite: false,
      }),
    );
    smoke.position.set(
      origin.x + Math.cos(ang) * 0.04,
      origin.y + 0.04,
      origin.z + Math.sin(ang) * 0.04,
    );
    smoke.userData.drift = new THREE.Vector3(Math.cos(ang) * 0.25, 0.5, Math.sin(ang) * 0.25);
    scene.add(smoke);
    parts.push({ mesh: smoke, role: "debris" });
  }

  activeFx.push({
    type: "patriot_boom",
    born: now,
    life: 720,
    parts,
    start: origin.clone(),
    end: origin.clone(),
    dir: new THREE.Vector3(0, 1, 0),
    dist: 0,
  });
}

const TANK_WRECK_MS = 6_000;
const BUILDING_WRECK_MS = 6_000;

function clearDamageFire(mesh) {
  const fire = mesh?.userData?.damageFire;
  if (!fire) return;
  mesh.remove(fire);
  fire.traverse((obj) => {
    obj.geometry?.dispose?.();
    if (obj.material) {
      if (Array.isArray(obj.material)) obj.material.forEach((m) => m.dispose?.());
      else obj.material.dispose?.();
    }
  });
  mesh.userData.damageFire = null;
  mesh.userData.damageFlames = null;
  mesh.userData.damageSmokes = null;
  mesh.userData.damageFireLevel = 0;
}

/** Live building: catch fire as HP drops (light → heavy). */
function syncBuildingDamageFire(mesh, entity) {
  if (!mesh?.userData?.building || mesh.userData.wreck) return;
  const maxHp = Math.max(1, entity.max_hp || entity.hp || 1);
  const ratio = Math.max(0, Math.min(1, (entity.hp ?? maxHp) / maxHp));
  let level = 0;
  if (ratio <= 0.28) level = 2;
  else if (ratio <= 0.55) level = 1;

  if (level === (mesh.userData.damageFireLevel || 0) && mesh.userData.damageFire) {
    return;
  }
  clearDamageFire(mesh);
  mesh.userData.damageFireLevel = level;
  if (level === 0) return;

  const h = mesh.userData.unitHeight || labelHeightFor(entity) * 0.45 || 0.6;
  const fireRoot = new THREE.Group();
  fireRoot.name = "damageFire";
  fireRoot.position.set(0, h * 0.35, 0);
  const count = level === 2 ? 5 : 3;
  const flames = [];
  for (let i = 0; i < count; i++) {
    const flame = new THREE.Mesh(
      new THREE.SphereGeometry(0.05 + i * 0.018 + level * 0.02, 7, 7),
      new THREE.MeshBasicMaterial({
        color: i % 2 === 0 ? 0xff5522 : 0xffcc55,
        transparent: true,
        opacity: 0.75 - i * 0.08,
        depthWrite: false,
      }),
    );
    flame.position.set(
      (Math.random() - 0.5) * 0.35 * level,
      0.05 + i * 0.05,
      (Math.random() - 0.5) * 0.3 * level,
    );
    fireRoot.add(flame);
    flames.push(flame);
  }
  const smokes = [];
  const smokeN = level === 2 ? 5 : 3;
  for (let i = 0; i < smokeN; i++) {
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.07 + Math.random() * 0.05, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x3a3830,
        transparent: true,
        opacity: 0.4,
        depthWrite: false,
      }),
    );
    smoke.position.set((Math.random() - 0.5) * 0.2, 0.12 + i * 0.08, (Math.random() - 0.5) * 0.2);
    smoke.userData.baseY = smoke.position.y;
    smoke.userData.phase = Math.random() * Math.PI * 2;
    fireRoot.add(smoke);
    smokes.push(smoke);
  }
  mesh.add(fireRoot);
  mesh.userData.damageFire = fireRoot;
  mesh.userData.damageFlames = flames;
  mesh.userData.damageSmokes = smokes;
}

function updateDamageFire(mesh, now) {
  const flames = mesh.userData.damageFlames;
  if (!flames?.length) return;
  const level = mesh.userData.damageFireLevel || 1;
  for (let i = 0; i < flames.length; i++) {
    const f = flames[i];
    if (!f?.material) continue;
    const flicker = 0.7 + Math.sin(now * 0.02 + i * 2.1) * 0.3;
    f.scale.setScalar(flicker * (0.9 + level * 0.15));
    f.position.y = 0.05 + i * 0.05 + Math.sin(now * 0.015 + i) * 0.02;
    f.material.opacity = (0.7 - i * 0.07) * (0.75 + flicker * 0.25);
  }
  const smokes = mesh.userData.damageSmokes || [];
  for (let i = 0; i < smokes.length; i++) {
    const s = smokes[i];
    if (!s?.material) continue;
    const phase = s.userData.phase || 0;
    const rise = (now * 0.0004 + phase) % 1;
    s.position.y = (s.userData.baseY || 0.12) + rise * 0.55;
    s.position.x = Math.sin(now * 0.0015 + phase) * 0.06;
    s.scale.setScalar(1 + rise * 2.2);
    s.material.opacity = 0.42 * (1 - rise);
  }
}

/** Structural collapse blast — taller fireball + flying debris chunks. */
function spawnBuildingExplosion(at, scale = 1) {
  if (!scene) return;
  const now = performance.now();
  const parts = [];
  const s = scale;

  const fireball = new THREE.Mesh(
    new THREE.SphereGeometry(0.14 * s, 12, 12),
    new THREE.MeshBasicMaterial({
      color: 0xff6622,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  fireball.position.copy(at);
  scene.add(fireball);
  parts.push({ mesh: fireball, role: "blast" });

  const core = new THREE.Mesh(
    new THREE.SphereGeometry(0.07 * s, 10, 10),
    new THREE.MeshBasicMaterial({
      color: 0xfff0a0,
      transparent: true,
      opacity: 1,
      depthWrite: false,
    }),
  );
  core.position.copy(at);
  scene.add(core);
  parts.push({ mesh: core, role: "blast" });

  const ring = new THREE.Mesh(
    new THREE.RingGeometry(0.12 * s, 0.28 * s, 24),
    new THREE.MeshBasicMaterial({
      color: 0xc8a060,
      transparent: true,
      opacity: 0.8,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  ring.rotation.x = -Math.PI / 2;
  ring.position.set(at.x, 0.05, at.z);
  scene.add(ring);
  parts.push({ mesh: ring, role: "ring" });

  for (let i = 0; i < 8; i++) {
    const ang = (i / 8) * Math.PI * 2;
    const chunk = new THREE.Mesh(
      new THREE.BoxGeometry(0.04 * s, 0.03 * s, 0.05 * s),
      new THREE.MeshBasicMaterial({
        color: 0x4a4538,
        transparent: true,
        opacity: 0.9,
        depthWrite: false,
      }),
    );
    chunk.position.copy(at);
    chunk.userData.drift = new THREE.Vector3(
      Math.cos(ang) * (0.5 + Math.random() * 0.4) * s,
      0.6 + Math.random() * 0.7,
      Math.sin(ang) * (0.5 + Math.random() * 0.4) * s,
    );
    scene.add(chunk);
    parts.push({ mesh: chunk, role: "debris" });
  }

  for (let i = 0; i < 6; i++) {
    const ang = (i / 6) * Math.PI * 2 + 0.3;
    const dust = new THREE.Mesh(
      new THREE.SphereGeometry(0.08 * s + Math.random() * 0.04, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x6a6558,
        transparent: true,
        opacity: 0.55,
        depthWrite: false,
      }),
    );
    dust.position.set(at.x + Math.cos(ang) * 0.08, at.y + 0.08, at.z + Math.sin(ang) * 0.08);
    dust.userData.drift = new THREE.Vector3(Math.cos(ang) * 0.4, 0.45, Math.sin(ang) * 0.4);
    scene.add(dust);
    parts.push({ mesh: dust, role: "debris" });
  }

  activeFx.push({
    type: "building_boom",
    born: now,
    life: 780,
    parts,
    start: at.clone(),
    end: at.clone(),
    dir: new THREE.Vector3(0, 1, 0),
    dist: 0,
  });
}

function buildingWreckScale(kind) {
  const k = String(kind || "");
  if (k === "hq") return 1.6;
  if (k === "war_factory") return 1.4;
  if (k === "barracks" || k === "power_plant" || k === "supply") return 1.15;
  if (k === "radar") return 1.0;
  if (k === "turret" || k === "bunker") return 0.75;
  return 1.1;
}

/** Building death: collapse boom, rubble settle, burn ~10s, then remove. */
function beginBuildingWreck(mesh, x, z, kind) {
  if (!mesh || mesh.userData.wreck) return;
  const px = x ?? mesh.position.x;
  const pz = z ?? mesh.position.z;
  const scale = buildingWreckScale(kind);
  clearDamageFire(mesh);
  Sfx.buildingCollapse(px, pz);
  spawnBuildingExplosion(new THREE.Vector3(px, 0.35 * scale, pz), scale);
  setTimeout(() => {
    if (!mesh.userData?.wreck || !scene) return;
    spawnBuildingExplosion(
      new THREE.Vector3(mesh.position.x + (Math.random() - 0.5) * 0.2, 0.25 * scale, mesh.position.z),
      scale * 0.7,
    );
  }, 320 + Math.random() * 280);

  mesh.userData.wreck = true;
  mesh.userData.wreckKind = "building";
  mesh.userData.wreckAt = performance.now();
  mesh.userData.wreckLife = BUILDING_WRECK_MS;
  mesh.userData.aimAt = null;

  mesh.traverse((obj) => {
    if (!obj.isMesh || !obj.material) return;
    if (obj.parent?.name === "wreckFire" || obj.parent?.name === "damageFire") return;
    const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
    const next = mats.map((m) => {
      const c = m.clone();
      if (c.color) c.color.multiplyScalar(0.32);
      if ("metalness" in c) c.metalness = 0.08;
      if ("roughness" in c) c.roughness = 0.95;
      if ("emissive" in c && c.emissive) c.emissive.setHex(0x331100);
      return c;
    });
    obj.material = Array.isArray(obj.material) ? next : next[0];
  });

  // Collapse / settle — heavier lean than a tank kill.
  const side = Math.random() < 0.5 ? -1 : 1;
  mesh.rotation.z += side * (0.12 + Math.random() * 0.18);
  mesh.rotation.x += 0.04 + Math.random() * 0.1;
  mesh.position.y = -0.02 * scale;

  const fireRoot = new THREE.Group();
  fireRoot.name = "wreckFire";
  const baseH = mesh.userData.unitHeight || 0.5;
  fireRoot.position.set(0, Math.max(0.15, baseH * 0.25), 0);
  const flames = [];
  for (let i = 0; i < 7; i++) {
    const flame = new THREE.Mesh(
      new THREE.SphereGeometry(0.06 * scale + i * 0.02, 7, 7),
      new THREE.MeshBasicMaterial({
        color: i % 2 === 0 ? 0xff5522 : 0xffdd66,
        transparent: true,
        opacity: 0.88 - i * 0.07,
        depthWrite: false,
      }),
    );
    flame.position.set(
      (Math.random() - 0.5) * 0.45 * scale,
      0.08 + i * 0.06,
      (Math.random() - 0.5) * 0.4 * scale,
    );
    fireRoot.add(flame);
    flames.push(flame);
  }
  const smokes = [];
  for (let i = 0; i < 8; i++) {
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.08 * scale + Math.random() * 0.06, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x2e2c28,
        transparent: true,
        opacity: 0.48,
        depthWrite: false,
      }),
    );
    smoke.position.set(
      (Math.random() - 0.5) * 0.3 * scale,
      0.15 + i * 0.09,
      (Math.random() - 0.5) * 0.3 * scale,
    );
    smoke.userData.baseY = smoke.position.y;
    smoke.userData.phase = Math.random() * Math.PI * 2;
    fireRoot.add(smoke);
    smokes.push(smoke);
  }
  mesh.add(fireRoot);
  mesh.userData.wreckFlames = flames;
  mesh.userData.wreckSmokes = smokes;
  mesh.userData.wreckFire = fireRoot;
  mesh.userData.wreckScale = scale;

  for (const name of ["selRing", "ownerLabel", "hpBar", "progressBar"]) {
    const ui = mesh.getObjectByName(name);
    if (ui) {
      mesh.remove(ui);
      ui.geometry?.dispose?.();
      ui.material?.map?.dispose?.();
      ui.material?.dispose?.();
    }
  }
  mesh.userData.selRing = null;
  mesh.userData.ownerLabel = null;
  mesh.userData.hpBar = null;
}

/** Kill a tank visually: boom SFX, tip the hull, burn ~10s, then remove. */
function beginTankWreck(mesh, x, z) {
  if (!mesh || mesh.userData.wreck) return;
  const px = x ?? mesh.position.x;
  const pz = z ?? mesh.position.z;
  Sfx.tankDestroyed(px, pz);
  spawnTankExplosion(new THREE.Vector3(px, 0.14, pz));
  // Secondary cook-off bloom a beat later.
  setTimeout(() => {
    if (!mesh.userData?.wreck || !scene) return;
    spawnTankExplosion(
      new THREE.Vector3(mesh.position.x + (Math.random() - 0.5) * 0.08, 0.16, mesh.position.z),
    );
  }, 280 + Math.random() * 220);

  mesh.userData.wreck = true;
  mesh.userData.wreckAt = performance.now();
  mesh.userData.wreckLife = TANK_WRECK_MS;
  mesh.userData.moving = false;
  mesh.userData.aimAt = null;
  mesh.userData.velX = 0;
  mesh.userData.velZ = 0;

  // Char the hull — clone materials so live tanks stay clean.
  mesh.traverse((obj) => {
    if (!obj.isMesh || !obj.material) return;
    const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
    const next = mats.map((m) => {
      const c = m.clone();
      if (c.color) c.color.multiplyScalar(0.28);
      if ("metalness" in c) c.metalness = 0.05;
      if ("roughness" in c) c.roughness = 0.92;
      if ("emissive" in c && c.emissive) c.emissive.setHex(0x221100);
      return c;
    });
    obj.material = Array.isArray(obj.material) ? next : next[0];
  });

  // Tip / settle like a kill-shot.
  const side = Math.random() < 0.5 ? -1 : 1;
  mesh.rotation.z += side * (0.18 + Math.random() * 0.22);
  mesh.rotation.x += 0.06 + Math.random() * 0.1;
  mesh.position.y = 0.02;

  // Persistent fire + smoke column attached to the wreck.
  const fireRoot = new THREE.Group();
  fireRoot.name = "wreckFire";
  fireRoot.position.set(0, 0.12, -0.02);
  const flames = [];
  for (let i = 0; i < 4; i++) {
    const flame = new THREE.Mesh(
      new THREE.SphereGeometry(0.04 + i * 0.012, 7, 7),
      new THREE.MeshBasicMaterial({
        color: i % 2 === 0 ? 0xff6622 : 0xffcc44,
        transparent: true,
        opacity: 0.9 - i * 0.12,
        depthWrite: false,
      }),
    );
    flame.position.set((Math.random() - 0.5) * 0.06, 0.04 + i * 0.035, (Math.random() - 0.5) * 0.04);
    fireRoot.add(flame);
    flames.push(flame);
  }
  const smokes = [];
  for (let i = 0; i < 5; i++) {
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.05 + Math.random() * 0.04, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0x3a3830,
        transparent: true,
        opacity: 0.45,
        depthWrite: false,
      }),
    );
    smoke.position.set((Math.random() - 0.5) * 0.05, 0.1 + i * 0.06, (Math.random() - 0.5) * 0.05);
    smoke.userData.baseY = smoke.position.y;
    smoke.userData.phase = Math.random() * Math.PI * 2;
    fireRoot.add(smoke);
    smokes.push(smoke);
  }
  mesh.add(fireRoot);
  mesh.userData.wreckFlames = flames;
  mesh.userData.wreckSmokes = smokes;
  mesh.userData.wreckFire = fireRoot;

  // Strip selection / UI chrome.
  const ring = mesh.getObjectByName("selRing");
  if (ring) {
    mesh.remove(ring);
    ring.geometry?.dispose?.();
    ring.material?.dispose?.();
    mesh.userData.selRing = null;
  }
  for (const name of ["ownerLabel", "hpBar", "progressBar"]) {
    const ui = mesh.getObjectByName(name);
    if (ui) {
      mesh.remove(ui);
      ui.material?.map?.dispose?.();
      ui.material?.dispose?.();
    }
  }
}

function updateTankWreck(mesh, now, dt) {
  const age = now - (mesh.userData.wreckAt || now);
  const life = mesh.userData.wreckLife || TANK_WRECK_MS;
  const t = Math.min(1, age / life);
  const fade = t > 0.82 ? 1 - (t - 0.82) / 0.18 : 1;
  const isBuilding = mesh.userData.wreckKind === "building";
  const scale = mesh.userData.wreckScale || 1;

  const flames = mesh.userData.wreckFlames || [];
  for (let i = 0; i < flames.length; i++) {
    const f = flames[i];
    if (!f?.material) continue;
    const flicker = 0.75 + Math.sin(now * 0.018 + i * 1.7) * 0.25;
    f.scale.setScalar(flicker * (1.05 - t * 0.35) * (isBuilding ? 1.15 : 1));
    f.position.y = (isBuilding ? 0.08 : 0.04) + i * (isBuilding ? 0.06 : 0.035) + Math.sin(now * 0.012 + i) * 0.012;
    f.material.opacity = (0.85 - i * 0.08) * fade * (1 - t * 0.35);
    f.material.color.setHex(flicker > 0.9 ? 0xffee66 : 0xff5522);
  }

  const smokes = mesh.userData.wreckSmokes || [];
  for (let i = 0; i < smokes.length; i++) {
    const s = smokes[i];
    if (!s?.material) continue;
    const phase = s.userData.phase || 0;
    const rise = ((now * 0.00035 + phase) % 1);
    s.position.y = (s.userData.baseY || 0.1) + rise * (isBuilding ? 0.7 : 0.45);
    s.position.x = Math.sin(now * 0.002 + phase) * (isBuilding ? 0.08 : 0.04);
    s.scale.setScalar(1 + rise * (isBuilding ? 2.4 : 1.8));
    s.material.opacity = 0.5 * (1 - rise) * fade * (0.85 - t * 0.4);
  }

  // Hull / rubble sinks into ash near the end.
  if (t > 0.75) {
    const sink = isBuilding ? 0.35 * scale : 0.12;
    mesh.position.y = (isBuilding ? -0.02 * scale : 0.02) - (t - 0.75) * sink;
  }
  void dt;
}

function reapUnitMesh(mesh) {
  if (!mesh) return;
  const id = mesh.userData?.id;
  if (scene) scene.remove(mesh);
  disposeMeshTree(mesh);
  if (id) state.meshes.delete(id);
}

function applyBlastKnock(x, z, radius) {
  for (const mesh of state.meshes.values()) {
    if (!mesh.userData?.isInfantry) continue;
    const dx = mesh.position.x - x;
    const dz = mesh.position.z - z;
    const d = Math.hypot(dx, dz);
    if (d > radius) continue;

    let nx;
    let nz;
    let force;
    if (d < 0.05) {
      force = 1;
      nx = Math.random() - 0.5;
      nz = Math.random() - 0.5;
      const len = Math.hypot(nx, nz) || 1;
      nx /= len;
      nz /= len;
    } else {
      force = 1 - d / radius;
      nx = dx / d;
      nz = dz / d;
    }

    mesh.userData.knock = {
      age: 0,
      life: 0.5 + force * 0.55,
      originX: mesh.position.x,
      originZ: mesh.position.z,
      x: 0,
      y: 0,
      z: 0,
      vx: nx * force * 1.35,
      vy: 0.45 + force * 1.25,
      vz: nz * force * 1.35,
      spin: (Math.random() - 0.5) * force * 7,
    };
    mesh.userData.moving = false;
  }
}

function updateKnockPhysics(mesh, dt) {
  const k = mesh.userData.knock;
  if (!k) return;
  k.age += dt;
  k.vy -= 7.5 * dt;
  k.x += k.vx * dt;
  k.y += k.vy * dt;
  k.z += k.vz * dt;
  if (k.y < 0) {
    k.y = 0;
    k.vy *= -0.28;
    k.vx *= 0.55;
    k.vz *= 0.55;
    if (Math.abs(k.vy) < 0.15) k.vy = 0;
  }
  mesh.position.set(k.originX + k.x, k.y, k.originZ + k.z);
  mesh.rotation.z = k.spin * Math.min(1, k.age * 2) * (1 - k.age / k.life);
  mesh.rotation.x = Math.min(0.9, k.y * 1.8) * Math.sign(k.spin || 1);

  if (k.age >= k.life && k.y <= 0.02) {
    mesh.position.set(k.originX + k.x, 0, k.originZ + k.z);
    mesh.rotation.x = 0;
    mesh.rotation.z = 0;
    mesh.userData.knock = null;
  }
}

function applyCrushKnock(mesh, tankMesh) {
  if (!mesh?.userData?.isInfantry || mesh.userData.knock) return;
  const dx = mesh.position.x - tankMesh.position.x;
  const dz = mesh.position.z - tankMesh.position.z;
  const d = Math.hypot(dx, dz) || 0.01;
  mesh.userData.knock = {
    age: 0,
    life: 0.55,
    originX: mesh.position.x,
    originZ: mesh.position.z,
    x: 0,
    y: 0,
    z: 0,
    vx: (dx / d) * 0.55,
    vy: 0.55 + Math.random() * 0.35,
    vz: (dz / d) * 0.55,
    spin: (Math.random() - 0.5) * 10,
  };
  mesh.userData.moving = false;
}

let lastCrushAt = 0;

function updateTankCrushVisuals(now) {
  if (now - lastCrushAt < 130) return;
  lastCrushAt = now;
  const tanks = [];
  for (const mesh of state.meshes.values()) {
    if (mesh.userData?.isTank) tanks.push(mesh);
  }
  if (!tanks.length) return;
  for (const tankMesh of tanks) {
    const tank = state.entities.get(tankMesh.userData.id);
    if (!tank) continue;
    for (const mesh of state.meshes.values()) {
      if (!mesh.userData?.isInfantry || mesh.userData.knock) continue;
      const ent = state.entities.get(mesh.userData.id);
      if (!ent || ent.team === tank.team) continue;
      const d = Math.hypot(
        mesh.position.x - tankMesh.position.x,
        mesh.position.z - tankMesh.position.z,
      );
      if (d < 0.12) applyCrushKnock(mesh, tankMesh);
    }
  }
}

function animate() {
  requestAnimationFrame(animate);
  if (!renderer) return;
  const now = performance.now();
  const dt = Math.min(0.05, ((now - (animate._last || now)) / 1000) || 0.016);
  animate._last = now;
  applyEdgePan();
  controls?.update();
  updateTankCrushVisuals(now);
  const reap = [];
  for (const mesh of state.meshes.values()) {
    if (mesh.userData.knock) {
      updateKnockPhysics(mesh, dt);
      if (mesh.userData.corpse && !mesh.userData.knock) reap.push(mesh);
    } else if (mesh.userData.wreck) {
      updateTankWreck(mesh, now, dt);
      const life = mesh.userData.wreckLife || TANK_WRECK_MS;
      if (now - (mesh.userData.wreckAt || 0) > life) {
        reap.push(mesh);
      }
    } else if (mesh.userData.corpse) {
      if (now - (mesh.userData.corpseAt || 0) > 900) reap.push(mesh);
    } else {
      if (mesh.userData.damageFire) updateDamageFire(mesh, now);
      if (mesh.userData.isRadar) {
        const dish = mesh.getObjectByName("radarDish");
        if (dish) dish.rotation.y += (mesh.userData.scanRate || 0.85) * dt;
      } else if (mesh.userData.isPatriot || mesh.userData.isBunker) smoothPatriotFacing(mesh, dt);
      else smoothUnitFacing(mesh, dt);
      updateTankDrive(mesh, dt);
      updateInfantryDrive(mesh, dt);
      updateInfantryWalk(mesh, dt, now);
    }
  }
  for (const mesh of reap) reapUnitMesh(mesh);
  updateCombatFx(now);
  if (now - (animate._sfxAt || 0) > 80) {
    animate._sfxAt = now;
    Sfx.updateSpatial();
  }
  renderer.render(scene, camera);
}

/* ---------- UI events ---------- */

$("#faction-row").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-faction]");
  if (!btn) return;
  state.faction = btn.dataset.faction;
  syncFactionButtons();
  send({ t: "set_faction", faction: state.faction });
});

$("#btn-create").addEventListener("click", () => {
  void enterGameFullscreen();
  send({
    t: "create_lobby",
    max_players: Number($("#max-players").value) || 50,
    map_size: Number($("#map-size").value) || 128,
    ffa: $("#ffa").checked,
    faction: state.faction || "usa",
  });
  toast(`Starting ${String(state.faction || "usa").toUpperCase()} match…`);
});

$("#btn-join").addEventListener("click", () => {
  void enterGameFullscreen();
  joinMatchId($("#lobby-id").value);
});

$("#btn-refresh-lobbies").addEventListener("click", async () => {
  try {
    const res = await fetch("/api/lobbies", { credentials: "same-origin" });
    const data = await res.json();
    renderOpenMatches(data.matches || data.lobbies || []);
  } catch {
    toast("Could not list matches");
  }
});

$("#open-lobbies").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-join]");
  if (!btn) return;
  void enterGameFullscreen();
  joinMatchId(btn.dataset.join);
});

$("#build-list").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-kind]");
  if (!btn) return;
  if (state.selectedBuild === btn.dataset.kind) {
    setBuildPlacement(null);
    return;
  }
  setBuildPlacement(btn.dataset.kind);
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    if (!$("#scoreboard")?.hidden) {
      setScoreboardOpen(false);
      return;
    }
    if (state.selectedBuild) {
      setBuildPlacement(null);
      return;
    }
    if (currentFullscreenElement()) {
      void exitGameFullscreen();
    }
    return;
  }

  // Ignore shortcuts while typing in inputs.
  const tag = event.target?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || event.target?.isContentEditable) {
    return;
  }

  if (event.key === "Tab") {
    if (!state.match || $("#match-screen")?.hidden) return;
    event.preventDefault();
    if (!event.repeat) setScoreboardOpen(true);
    return;
  }

  // 1-9 select building from the build list for placement.
  if (event.key >= "1" && event.key <= "9") {
    const index = Number(event.key) - 1;
    const item = state.buildable[index];
    if (!item || !state.match) return;
    event.preventDefault();
    if (state.selectedBuild === item.kind) {
      setBuildPlacement(null);
    } else {
      setBuildPlacement(item.kind);
    }
    return;
  }

  // H — center camera on own Command Center.
  if (event.key === "h" || event.key === "H") {
    if (!state.match) return;
    event.preventDefault();
    centerCameraOnHq();
    return;
  }

  // M — dev: personal full-map vision toggle (server-side; others stay fogged).
  if (event.key === "m" || event.key === "M") {
    if (!state.match || $("#match-screen")?.hidden) return;
    event.preventDefault();
    send({ t: "toggle_debug_vision" });
  }
});

window.addEventListener("keyup", (event) => {
  if (event.key === "Tab") {
    event.preventDefault();
    setScoreboardOpen(false);
  }
});

window.addEventListener("blur", () => setScoreboardOpen(false));

document.addEventListener("fullscreenchange", () => {
  onResize();
});
document.addEventListener("webkitfullscreenchange", () => {
  onResize();
});

$("#unit-list").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-unit]");
  if (!btn) return;
  if (!state.selectedBuilding) {
    toast("Select a barracks/factory first");
    return;
  }
  send({
    t: "train_unit",
    building_id: state.selectedBuilding,
    unit: btn.dataset.unit,
  });
});

$("#scoreboard-body")?.addEventListener("pointerdown", (event) => {
  const row = event.target.closest("tr[data-owner]");
  if (!row) return;
  event.preventDefault();
  event.stopPropagation();
  const owner = row.dataset.owner;
  if (!owner) return;
  centerCameraOnOwner(owner, row.dataset.hqX, row.dataset.hqY);
});

$("#store-items").addEventListener("click", async (event) => {
  const btn = event.target.closest("[data-item]");
  if (!btn) return;
  const itemId = btn.dataset.item;
  const owned = state.entitlements.includes(itemId);

  if (owned && itemId.startsWith("flag_")) {
    send({ t: "equip_cosmetic", slot: "flag", id: itemId });
    return;
  }

  try {
    const response = await fetch("/api/store/dev-grant", {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ item_id: itemId }),
    });
    const data = await response.json();
    if (!response.ok) throw new Error(data.error || "Grant failed");
    state.entitlements = data.entitlements || [];
    renderStore();
    $("#store-status").textContent = `Granted ${itemId} (cosmetic / meta only)`;
    if (itemId.startsWith("flag_")) {
      send({ t: "equip_cosmetic", slot: "flag", id: itemId });
    }
  } catch (error) {
    $("#store-status").textContent = error.message;
  }
});

$("#btn-logout").addEventListener("click", async () => {
  await fetch("/auth/logout", {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
  });
  location.assign("/");
});

$("#viewport")?.addEventListener("contextmenu", (e) => e.preventDefault());

connect();

// Auto-load open matches once connected UI is ready
setTimeout(() => $("#btn-refresh-lobbies")?.click(), 600);
