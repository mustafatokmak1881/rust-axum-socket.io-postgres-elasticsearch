import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { STLLoader } from "three/addons/loaders/STLLoader.js";
import { OBJLoader } from "three/addons/loaders/OBJLoader.js";
import { MTLLoader } from "three/addons/loaders/MTLLoader.js";

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
  faction: "usa",
  ready: false,
  reconnectAttempt: 0,
  matchEnded: false,
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
        <span>Map ${m.map_size}${m.ffa ? " · FFA" : ""} · click to join</span>
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
  send({ t: "join_lobby", lobby_id });
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
  state.entities.clear();
  clearWorldMeshes();
  aoiRadius = Number(snapshot.aoi_radius) || 28;
  for (const entity of snapshot.entities || []) {
    state.entities.set(entity.id, entity);
  }
  $("#lobby-screen").hidden = true;
  $("#match-screen").hidden = false;
  // Fullscreen only from click handlers (create/join/pointer) — browsers block gesture-less FS.
  void setupMatchScene(snapshot);
}

async function setupMatchScene(snapshot) {
  updateResources(snapshot.resources);
  updateArmyCounts();
  renderBuildList(snapshot.buildable || []);
  renderUnitList(snapshot.trainable || []);
  await ensureBuildingModel();
  const terrain = await loadTerrainTexture(snapshot.map_size);
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

function centerCameraOnHq() {
  if (!controls || !camera || !state.match) return;
  let hq = null;
  for (const entity of state.entities.values()) {
    if (entity.owner === state.match.you && entity.kind === "hq") {
      hq = entity;
      break;
    }
  }
  if (!hq) {
    toast("Command Center not found");
    return;
  }
  panCameraTo(hq.x, hq.y);
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
      if (String(entity.kind || "").includes("tank")) tanks += 1;
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

function renderUnitList(items) {
  $("#unit-list").innerHTML = items
    .map(
      (item) => `
      <button type="button" class="unit-item" data-unit="${escapeHtml(item.unit)}" data-from="${escapeHtml(item.from_building)}">
        <strong>${escapeHtml(item.name)}</strong>
        <small>from ${escapeHtml(item.from_building)} · ${item.cost_supplies}s/${item.cost_fuel}f · ${Math.round((item.train_ms || 0) / 1000)}s</small>
      </button>`,
    )
    .join("");
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
}

function applyDelta(msg) {
  updateResources(msg.resources);
  applyExploredNew(msg.explored_new);

  const removedBuildings = [];
  for (const id of msg.removed || []) {
    const prev = state.entities.get(id);
    if (prev?.building) removedBuildings.push(prev);
    state.entities.delete(id);
    const mesh = state.meshes.get(id);
    if (mesh && scene) {
      scene.remove(mesh);
      state.meshes.delete(id);
    } else {
      state.meshes.delete(id);
    }
    Sfx.stopBuild(id);
    Sfx.stopEngine(id);
  }
  for (const b of removedBuildings) {
    Sfx.buildingCollapse(b.x, b.y);
  }

  if (msg.removed?.length) {
    const dead = new Set(msg.removed);
    const before = state.selectedUnits.length;
    state.selectedUnits = state.selectedUnits.filter((id) => !dead.has(id));
    if (state.selectedUnits.length !== before) syncSelectionMarkers();
    if (dead.has(state.selectedBuilding)) state.selectedBuilding = null;
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
    `Tick ${msg.tick} · ${state.entities.size} entities · vision fog`;
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
  lastRifleAt: 0,
  rifleRest: 0.5,
  // World-units: full volume inside ref, silent past max. Camera look-at is listener.
  ranges: {
    rifle: { ref: 6, max: 28, exp: 1.85 },
    tank: { ref: 18, max: 150, exp: 1.2 },
    missile: { ref: 8, max: 48, exp: 1.6 },
    build: { ref: 5, max: 22, exp: 1.85 },
    collapse: { ref: 8, max: 56, exp: 1.5 },
    complete: { ref: 5, max: 26, exp: 1.85 },
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
      this.buses.tank.gain.value = 1.55;
      this.buses.fx.gain.value = 0.9;
      // Small-arms share a compressor; tank boom skips it so it stays on top.
      this.buses.rifle.connect(this.compressor);
      this.buses.fx.connect(this.compressor);
      this.compressor.connect(this.master);
      this.buses.tank.connect(this.master);
      this.master.connect(this.limiter);
      this.limiter.connect(this.ctx.destination);
    }
    if (this.ctx.state === "suspended") void this.ctx.resume();
    void this.loadTankMove();
    return true;
  },

  dest(name) {
    return this.buses[name] || this.master;
  },

  /** Cannon blast buries small-arms the way a real 120 mm does. */
  duckRifles(seconds = 1.35) {
    const bus = this.buses.rifle;
    if (!bus || !this.ctx) return;
    const t0 = this.ctx.currentTime;
    const g = bus.gain;
    const rest = this.rifleRest;
    try {
      g.cancelScheduledValues(t0);
      g.setValueAtTime(Math.max(0.03, g.value), t0);
      g.linearRampToValueAtTime(0.03, t0 + 0.012);
      g.setValueAtTime(0.03, t0 + seconds * 0.45);
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

  /**
   * 0 = inaudible, 1 = full. Natural falloff: loud nearby, quiet mid-range, mute far.
   */
  volumeAt(worldX, worldY, range) {
    const spec = range || this.ranges.rifle;
    const { ref, max } = spec;
    const ear = this.listenerXZ();
    const d = Math.hypot(worldX - ear.x, worldY - ear.z);
    if (!(d < max)) return 0;
    if (d <= ref) return 1;
    const t = (d - ref) / (max - ref);
    return Math.pow(1 - t, spec.exp || 1.85);
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

  /**
   * AK-47 style report: deep body crack + mid muzzle snap + bolt clack.
   * (No toy square-wave chirps.)
   */
  rifle(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.rifle);
    if (vol <= 0.004) return;
    const now = performance.now();
    // ~AK cyclic rate spacing when many fire; still allows multi-unit volleys.
    if (now - this.lastRifleAt < 48) return;
    this.lastRifleAt = now;

    const t0 = this.ctx.currentTime;
    const jitter = Math.random();
    const bus = this.dest("rifle");
    const loud = vol * 3.2;

    // 1) Low-end powder thump (body of the report)
    this.noiseBurst(0.09, 0.34, 140 + jitter * 40, "lowpass", loud, 0.55, bus);
    this.tone(78 + jitter * 18, 0.07, "sine", 0.2, 42, loud * 0.95, bus);
    this.tone(52 + jitter * 10, 0.1, "triangle", 0.12, 28, loud * 0.85, bus);

    // 2) Mid crack — the recognizable “AK bark”
    this.noiseBurst(0.055, 0.28, 900 + jitter * 200, "bandpass", loud, 1.1, bus);
    this.noiseBurst(0.035, 0.18, 1600 + jitter * 300, "bandpass", loud, 1.4, bus);

    // 3) Sharp transient snap (not a beep — short filtered noise)
    this.noiseBurst(0.018, 0.22, 2800 + jitter * 400, "highpass", loud * 0.9, 0.8, bus);

    // 4) Mechanical bolt / carrier clack
    this.tone(190 + jitter * 40, 0.025, "triangle", 0.045, 90, loud * 0.7, bus);
    this.noiseBurst(0.022, 0.08, 420, "bandpass", loud * 0.75, 2.2, bus);

    // 5) Short metallic ring tail (receiver / muzzle)
    const ring = this.ctx.createOscillator();
    const ringG = this.ctx.createGain();
    const ringF = this.ctx.createBiquadFilter();
    ring.type = "triangle";
    ring.frequency.setValueAtTime(620 + jitter * 80, t0);
    ring.frequency.exponentialRampToValueAtTime(280, t0 + 0.08);
    ringF.type = "bandpass";
    ringF.frequency.value = 700;
    ringF.Q.value = 3.5;
    const ringPeak = 0.035 * loud;
    ringG.gain.setValueAtTime(0.0001, t0);
    ringG.gain.exponentialRampToValueAtTime(Math.max(0.0001, ringPeak), t0 + 0.004);
    ringG.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.09);
    ring.connect(ringF);
    ringF.connect(ringG);
    ringG.connect(bus);
    ring.start(t0);
    ring.stop(t0 + 0.1);
  },

  tankCannon(x, y) {
    if (!this.ensure()) return;
    const vol = this.volumeAt(x, y, this.ranges.tank);
    if (vol <= 0.002) return;
    const t0 = this.ctx.currentTime;
    const bus = this.dest("tank");
    // 120 mm class: chest-thump bass carries; small-arms stay underneath.
    const boom = Math.min(1.7, Math.pow(Math.max(vol, 0.015), 0.32) * 1.85) * 5.2;
    this.duckRifles(1.4);

    // 1) Infrasonic pressure (felt more than heard)
    this.tone(19, 1.15, "sine", 0.72, 11, boom, bus);
    this.tone(28, 1.05, "sine", 0.62, 14, boom, bus);
    this.tone(41, 0.95, "sine", 0.5, 16, boom, bus);

    // 2) Muzzle blast + HE crack
    this.noiseBurst(0.85, 0.95, 55, "lowpass", boom, 0.35, bus);
    this.noiseBurst(0.45, 0.72, 160, "lowpass", boom, 0.5, bus);
    this.noiseBurst(0.18, 0.58, 520, "bandpass", boom, 0.9, bus);
    this.noiseBurst(0.07, 0.4, 1400, "highpass", boom * 0.7, 0.65, bus);
    this.tone(62, 0.55, "triangle", 0.38, 22, boom, bus);
    this.tone(110, 0.22, "sawtooth", 0.16, 36, boom * 0.8, bus);

    // 3) Shock roll + distant slap
    const roll = (delay, dur, freq, gain) => {
      const src = this.ctx.createBufferSource();
      src.buffer = this.noiseBuffer(dur);
      const filt = this.ctx.createBiquadFilter();
      filt.type = "lowpass";
      filt.frequency.value = freq;
      const g = this.ctx.createGain();
      const peak = gain * boom;
      g.gain.setValueAtTime(0.0001, t0);
      g.gain.setValueAtTime(0.0001, t0 + delay);
      g.gain.exponentialRampToValueAtTime(Math.max(0.0001, peak), t0 + delay + 0.04);
      g.gain.exponentialRampToValueAtTime(0.0001, t0 + delay + dur);
      src.connect(filt);
      filt.connect(g);
      g.connect(bus);
      src.start(t0 + delay);
      src.stop(t0 + delay + dur + 0.02);
    };
    roll(0.05, 0.9, 110, 0.55);
    roll(0.22, 0.7, 80, 0.32);
    roll(0.48, 0.55, 60, 0.18);
  },

  loadTankMove() {
    if (this.tankMoveBuf) return Promise.resolve(this.tankMoveBuf);
    if (this.tankMoveWait) return this.tankMoveWait;
    if (!this.ensure()) return Promise.resolve(null);
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
      g.connect(this.dest("tank"));
      node = { src: null, g, x, y, throttle: 0, stopping: false };
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
      node.g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.25);
      node.src?.stop(t0 + 0.28);
      node.src = null;
    } catch {
      // ignore
    }
    setTimeout(() => {
      const cur = this.engines.get(id);
      if (cur !== node) return;
      this.engines.delete(id);
      try {
        node.g.disconnect();
      } catch {
        // ignore
      }
    }, 320);
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

  startBuild(id, x, y) {
    if (!this.ensure()) return;
    if (this.builds.has(id)) {
      const existing = this.builds.get(id);
      existing.x = x;
      existing.y = y;
      return;
    }
    const t0 = this.ctx.currentTime;
    const osc = this.ctx.createOscillator();
    const lfo = this.ctx.createOscillator();
    const lfoGain = this.ctx.createGain();
    const g = this.ctx.createGain();
    const filt = this.ctx.createBiquadFilter();
    osc.type = "sawtooth";
    osc.frequency.value = 78;
    lfo.type = "sine";
    lfo.frequency.value = 3.2;
    lfoGain.gain.value = 12;
    filt.type = "lowpass";
    filt.frequency.value = 420;
    const vol = this.volumeAt(x, y, this.ranges.build);
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(Math.max(0.0001, 0.32 * vol), t0 + 0.3);
    lfo.connect(lfoGain);
    lfoGain.connect(osc.frequency);
    osc.connect(filt);
    filt.connect(g);
    g.connect(this.dest("fx"));
    osc.start();
    lfo.start();
    const tick = setInterval(() => {
      const node = this.builds.get(id);
      if (!node) return;
      const v = this.volumeAt(node.x, node.y, this.ranges.build);
      if (v <= 0.004) return;
      this.noiseBurst(0.03, 0.04, 1800, "bandpass", v * 3, 0.7, this.dest("fx"));
      this.tone(240 + Math.random() * 80, 0.04, "triangle", 0.025, 90, v * 3, this.dest("fx"));
    }, 480 + Math.random() * 220);
    this.builds.set(id, { osc, lfo, g, tick, x, y, baseGain: 0.32 });
  },

  stopBuild(id) {
    const node = this.builds.get(id);
    if (!node) return;
    clearInterval(node.tick);
    try {
      const t0 = this.ctx.currentTime;
      node.g.gain.cancelScheduledValues(t0);
      node.g.gain.setValueAtTime(Math.max(0.0001, node.g.gain.value), t0);
      node.g.gain.exponentialRampToValueAtTime(0.0001, t0 + 0.2);
      node.osc.stop(t0 + 0.25);
      node.lfo.stop(t0 + 0.25);
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
      const vol = this.volumeAt(node.x, node.y, this.ranges.build);
      const target = Math.max(0.0001, (node.baseGain || 0.11) * vol);
      try {
        node.g.gain.cancelScheduledValues(t0);
        node.g.gain.setTargetAtTime(target, t0, 0.08);
      } catch {
        // ignore
      }
    }
    for (const node of this.engines.values()) {
      if (node.stopping) continue;
      const vol = this.volumeAt(node.x, node.y, this.ranges.tank);
      const th = node.throttle || 0;
      const target = Math.max(0.0001, vol * 0.85 * (0.5 + th * 0.5));
      try {
        node.g.gain.setTargetAtTime(target, t0, 0.1);
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
    if (k.includes("tank")) this.tankCannon(x, y);
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
    if (k.includes("tank")) return 0;
    if (k.includes("missile")) return 1;
    return 2;
  };
  list.sort((a, b) => rank(shotKindOf(a)) - rank(shotKindOf(b)));
  let rifles = 0;
  for (const shot of list) {
    spawnShotFx(shot);
    const kind = shotKindOf(shot);
    const k = String(kind);
    Radar.ping(shot.x0, shot.y0, k);
    if (k.includes("tank") && shot.x1 != null && shot.y1 != null) {
      Radar.ping(shot.x1, shot.y1, "tank");
    }
    if (!k.includes("tank") && !k.includes("missile")) {
      rifles += 1;
      // A volley must never bury the cannon in the same frame.
      if (rifles > 3) continue;
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
    const missile = k.includes("missile");
    if (!tank && !missile) {
      this.rifleSkip += 1;
      if (this.rifleSkip % 5) return;
    }
    this.pings.push({
      x,
      y,
      tank,
      born: performance.now(),
      life: tank ? 900 : 500,
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
      if (entity.building || entity.kind === "hq" || String(entity.kind).includes("tank")) {
        continue;
      }
      if (entity.owner !== you) continue;
      ownN += 1;
      if (ownN % 2) continue;
      ctx.fillRect((entity.x / mapSize) * w - 0.75, (entity.y / mapSize) * h - 0.75, 2, 2);
    }
    ctx.fillStyle = "#ff5a3a";
    for (const entity of state.entities.values()) {
      if (entity.hp != null && entity.hp <= 0) continue;
      if (entity.building || entity.kind === "hq" || String(entity.kind).includes("tank")) {
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
      const heavy = entity.building || kind.includes("tank") || kind === "hq";
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
/** Pre-scaled OBJ/MTL groups (HQ etc.) — clone per entity. */
let buildingTemplates = Object.create(null);
let buildingModelsPromise = null;
/** True after OBJ/STL templates are ready — avoid permanent fallback boxes. */
let buildingModelsReady = false;
let ghostMesh = null;
let fogOfWar = null;
let fogExploredData = null; // Uint8Array size*size — 0/1 explored
let fogVisionData = null;   // Uint8Array size*size — 0/1 currently visible
let fogDataTexture = null;
let aoiRadius = 20;
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

const BUILDING_MODELS = {
  hq: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 2.15 },
  power_plant: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.7 },
  supply: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.7 },
  barracks: { type: "stl", url: "/assets/models/barracks.stl", target: 1.35 },
  war_factory: { type: "obj", obj: "/assets/models/war-factory.obj", mtl: "/assets/models/war-factory.mtl", target: 2.1 },
  turret: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.4 },
};

/** Bump when BUILDING_MODELS targets change so cached meshes refit. */
const BUILDING_FIT_VERSION = 2;

async function prepareStlGeometry(url, targetSize) {
  const loader = new STLLoader();
  const geo = await loader.loadAsync(url);
  // Most CAD STLs are Z-up; Three.js is Y-up — stand the building upright.
  geo.rotateX(-Math.PI / 2);
  geo.computeVertexNormals();
  geo.center();
  geo.computeBoundingBox();
  const box = geo.boundingBox;
  const size = new THREE.Vector3();
  box.getSize(size);
  const maxDim = Math.max(size.x, size.y, size.z) || 1;
  const s = targetSize / maxDim;
  geo.scale(s, s, s);
  geo.computeBoundingBox();
  geo.translate(0, -geo.boundingBox.min.y, 0);
  return geo;
}

async function loadObjRoot(objUrl, mtlUrl) {
  const mtlLoader = new MTLLoader();
  const materials = await mtlLoader.loadAsync(mtlUrl);
  materials.preload();
  const objLoader = new OBJLoader();
  objLoader.setMaterials(materials);
  const root = await objLoader.loadAsync(objUrl);
  root.traverse((child) => {
    if (!child.isMesh) return;
    child.castShadow = true;
    child.receiveShadow = true;
    const mats = Array.isArray(child.material) ? child.material : [child.material];
    for (const mat of mats) {
      if (!mat) continue;
      mat.side = THREE.FrontSide;
      if (mat.map) mat.map.colorSpace = THREE.SRGBColorSpace;
    }
  });
  return root;
}

function fitObjRoot(root, targetSize) {
  // Reset local transform so bbox is in true model space.
  root.position.set(0, 0, 0);
  root.rotation.set(0, 0, 0);
  root.scale.set(1, 1, 1);
  root.updateMatrixWorld(true);

  const box = new THREE.Box3().setFromObject(root);
  const size = new THREE.Vector3();
  const center = new THREE.Vector3();
  box.getSize(size);
  box.getCenter(center);

  const maxDim = Math.max(size.x, size.y, size.z) || 1;
  const s = targetSize / maxDim;

  // Three.js matrix is T*R*S — scale first, then translate by -center*s
  // so the visual center lands on the group origin (matches STL geo.center()).
  root.scale.setScalar(s);
  root.position.set(-center.x * s, -center.y * s, -center.z * s);
  root.updateMatrixWorld(true);

  const grounded = new THREE.Box3().setFromObject(root);
  root.position.y -= grounded.min.y;
  root.updateMatrixWorld(true);
}

function makeObjTemplate(sharedRoot, targetSize) {
  const root = sharedRoot.clone(true);
  detachMaterials(root);
  fitObjRoot(root, targetSize);
  const wrapper = new THREE.Group();
  wrapper.add(root);
  wrapper.userData.keepMtlColors = true;
  return wrapper;
}

/** Force unique materials so opacity/ghost never leaks across buildings. */
function detachMaterials(root) {
  root.traverse((child) => {
    if (!child.isMesh || !child.material) return;
    if (Array.isArray(child.material)) {
      child.material = child.material.map((mat) => (mat ? mat.clone() : mat));
    } else {
      child.material = child.material.clone();
    }
  });
}

function geometryForKind(kind) {
  return buildingGeometries[kind] || buildingGeometries.barracks || null;
}

function templateForKind(kind) {
  return buildingTemplates[kind] || null;
}

function createBuildingMesh(kind, fallbackMat) {
  const template = templateForKind(kind);
  if (template) {
    const mesh = template.clone(true);
    detachMaterials(mesh);
    rememberBaseOpacities(mesh);
    mesh.userData.keepMtlColors = true;
    mesh.userData.building = true;
    mesh.userData.modelKind = kind;
    mesh.userData.isFallback = false;
    mesh.userData.buildingFitVersion = BUILDING_FIT_VERSION;
    return mesh;
  }
  const geo = geometryForKind(kind);
  if (geo) {
    const mesh = new THREE.Mesh(geo, fallbackMat);
    mesh.userData.building = true;
    mesh.userData.modelKind = kind;
    mesh.userData.isFallback = false;
    mesh.userData.buildingFitVersion = BUILDING_FIT_VERSION;
    return mesh;
  }
  // Temporary placeholder only — replaced once models finish loading.
  const h = kind === "hq" ? 2.6 : 1.6;
  const w = kind === "hq" ? 2.4 : 1.4;
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, w), fallbackMat);
  mesh.userData.building = true;
  mesh.userData.modelKind = kind;
  mesh.userData.isFallback = true;
  mesh.userData.buildingFitVersion = BUILDING_FIT_VERSION;
  return mesh;
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
  if (
    buildingTemplates.hq &&
    buildingGeometries.barracks &&
    buildingTemplates.hq.userData?.buildingFitVersion === BUILDING_FIT_VERSION
  ) {
    buildingModelsReady = true;
    return true;
  }
  // Force reload when footprint targets change.
  if (buildingTemplates.hq?.userData?.buildingFitVersion !== BUILDING_FIT_VERSION) {
    for (const key of Object.keys(buildingTemplates)) delete buildingTemplates[key];
    for (const key of Object.keys(buildingGeometries)) delete buildingGeometries[key];
    buildingModelsPromise = null;
  }
  if (buildingModelsPromise) return buildingModelsPromise;

  buildingModelsPromise = (async () => {
    const stlCache = new Map();
    const objBaseCache = new Map();
    for (const [kind, spec] of Object.entries(BUILDING_MODELS)) {
      if (spec.type === "obj") {
        const key = `${spec.obj}|${spec.mtl}`;
        if (!objBaseCache.has(key)) {
          objBaseCache.set(key, await loadObjRoot(spec.obj, spec.mtl));
        }
        const tpl = makeObjTemplate(objBaseCache.get(key), spec.target);
        tpl.userData.buildingFitVersion = BUILDING_FIT_VERSION;
        buildingTemplates[kind] = tpl;
      } else {
        const key = `${spec.url}|${spec.target}`;
        if (!stlCache.has(key)) {
          stlCache.set(key, prepareStlGeometry(spec.url, spec.target));
        }
        buildingGeometries[kind] = await stlCache.get(key);
      }
    }
    buildingModelsReady = true;
    return true;
  })();

  try {
    return await buildingModelsPromise;
  } catch (error) {
    console.error(error);
    buildingModelsPromise = null;
    buildingModelsReady = false;
    toast("Building models failed to load — using fallbacks");
    return null;
  }
}

async function loadTerrainTexture(mapSize) {
  try {
    const loader = new THREE.TextureLoader();
    const base = await loader.loadAsync("/assets/terrain.jpg");
    return bakeRandomTerrainTexture(base.image, mapSize);
  } catch (error) {
    console.error(error);
    return bakeRandomTerrainTexture(null, mapSize);
  }
}

/** One unique ground atlas — avoids obvious square tile repeats. */
function bakeRandomTerrainTexture(image, mapSize) {
  const size = 1024;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");

  ctx.fillStyle = "#455832";
  ctx.fillRect(0, 0, size, size);

  if (image) {
    for (let i = 0; i < 48; i++) {
      ctx.save();
      const x = Math.random() * size;
      const y = Math.random() * size;
      const scale = 0.35 + Math.random() * 1.4;
      ctx.translate(x, y);
      ctx.rotate(Math.random() * Math.PI * 2);
      ctx.globalAlpha = 0.22 + Math.random() * 0.5;
      const hue = (Math.random() - 0.5) * 48;
      const sat = 0.65 + Math.random() * 0.7;
      const bri = 0.8 + Math.random() * 0.35;
      ctx.filter = `hue-rotate(${hue}deg) saturate(${sat}) brightness(${bri})`;
      const w = Math.max(64, (image.width || 256) * scale * 0.45);
      const h = Math.max(64, (image.height || 256) * scale * 0.45);
      ctx.drawImage(image, -w / 2, -h / 2, w, h);
      ctx.restore();
    }
  }

  // Soft irregular patches (ellipses — not squares)
  for (let i = 0; i < 90; i++) {
    const x = Math.random() * size;
    const y = Math.random() * size;
    const rx = 18 + Math.random() * 90;
    const ry = 14 + Math.random() * 75;
    ctx.beginPath();
    ctx.ellipse(x, y, rx, ry, Math.random() * Math.PI, 0, Math.PI * 2);
    const dirt = Math.random() > 0.55;
    ctx.fillStyle = dirt
      ? `rgba(${70 + Math.random() * 40},${55 + Math.random() * 35},${30 + Math.random() * 25},${0.1 + Math.random() * 0.22})`
      : `rgba(${35 + Math.random() * 40},${70 + Math.random() * 60},${30 + Math.random() * 35},${0.08 + Math.random() * 0.2})`;
    ctx.fill();
  }

  // Fine grain so large flats don't look painted
  const data = ctx.getImageData(0, 0, size, size);
  const px = data.data;
  for (let i = 0; i < px.length; i += 4) {
    const n = (Math.random() - 0.5) * 28;
    px[i] = Math.max(0, Math.min(255, px[i] + n));
    px[i + 1] = Math.max(0, Math.min(255, px[i + 1] + n * 1.05));
    px[i + 2] = Math.max(0, Math.min(255, px[i + 2] + n * 0.7));
  }
  ctx.putImageData(data, 0, 0);

  const tex = new THREE.CanvasTexture(canvas);
  tex.wrapS = THREE.MirroredRepeatWrapping;
  tex.wrapT = THREE.MirroredRepeatWrapping;
  tex.anisotropy = 8;
  tex.colorSpace = THREE.SRGBColorSpace;
  // Few large mirrored tiles — not a dense square grid
  const tiles = Math.max(1.6, mapSize / 90);
  tex.repeat.set(tiles, tiles * (0.85 + Math.random() * 0.3));
  tex.offset.set(Math.random(), Math.random());
  tex.rotation = Math.random() * Math.PI * 2;
  tex.center.set(0.5, 0.5);
  tex.needsUpdate = true;
  return tex;
}

function makeFallbackTerrainTexture(mapSize) {
  return bakeRandomTerrainTexture(null, mapSize);
}

function visionRadiusFor(entity) {
  if (entity.kind === "hq") return 20;
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

  fogVisionData.fill(0);
  const you = state.match?.you;
  const stamped = [];
  for (const entity of state.entities.values()) {
    if (entity.owner !== you) continue;
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
  const tex = fogDataTexture.image?.data;
  if (!tex) return;
  for (let i = 0; i < mapSize * mapSize; i++) {
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

  // Soft dirt patches
  const patchMat = new THREE.MeshStandardMaterial({
    color: 0x6a5a3a,
    roughness: 1,
    metalness: 0,
    flatShading: true,
  });
  const patchCount = Math.min(80, Math.floor(size * 0.35));
  for (let i = 0; i < patchCount; i++) {
    const w = 2.5 + Math.random() * 6;
    const d = 2 + Math.random() * 5;
    const mesh = new THREE.Mesh(new THREE.CircleGeometry(1, 7), patchMat);
    mesh.scale.set(w * 0.5, d * 0.5, 1);
    mesh.rotation.x = -Math.PI / 2;
    mesh.position.set(
      4 + Math.random() * (size - 8),
      0.03,
      4 + Math.random() * (size - 8),
    );
    mesh.rotation.z = Math.random() * Math.PI;
    group.add(mesh);
  }

  // Low rock / rubble blobs
  const rockMat = new THREE.MeshStandardMaterial({
    color: 0x5a5848,
    roughness: 0.95,
    metalness: 0.05,
    flatShading: true,
  });
  const rockCount = Math.min(60, Math.floor(size * 0.22));
  for (let i = 0; i < rockCount; i++) {
    const s = 0.25 + Math.random() * 0.55;
    const mesh = new THREE.Mesh(
      new THREE.DodecahedronGeometry(s, 0),
      rockMat,
    );
    mesh.position.set(
      3 + Math.random() * (size - 6),
      s * 0.35,
      3 + Math.random() * (size - 6),
    );
    mesh.rotation.set(Math.random(), Math.random(), Math.random());
    group.add(mesh);
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
  scene.background = new THREE.Color(0x12180e);
  // Distant haze; vision limit is the fog-of-war disc.
  scene.fog = new THREE.Fog(0x12180e, Math.max(70, aoiRadius * 2.2), Math.max(120, aoiRadius * 4.5));

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

  const hemi = new THREE.HemisphereLight(0xc8d8a8, 0x1a2010, 1.15);
  scene.add(hemi);
  const sun = new THREE.DirectionalLight(0xfff0c8, 0.95);
  sun.position.set(40, 60, 20);
  scene.add(sun);

  const geo = new THREE.PlaneGeometry(size, size, 1, 1);
  const mat = new THREE.MeshStandardMaterial({
    map: terrainTexture || null,
    color: terrainTexture ? 0xd0d8c0 : 0x3d5230,
    roughness: 0.97,
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
    0x24301c,
  );
  grid.material.opacity = 0.08;
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
      color: 0x10160c,
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
    // Shared STL geometries stay cached.
    if (obj.geometry && !Object.values(buildingGeometries).includes(obj.geometry)) {
      obj.geometry.dispose?.();
    }
    // Only dispose materials we created for ghost (not shared template mats).
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
  // Half-footprint from BUILDING_MODELS target (must match server building_radius).
  const visual = {
    hq: 2.15,
    war_factory: 2.1,
    barracks: 1.35,
    power_plant: 1.7,
    supply: 1.7,
    turret: 1.4,
  }[kind] ?? 1.35;
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

function setSelectedUnits(ids, toastMsg) {
  state.selectedUnits = ids;
  state.selectedBuilding = null;
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
      state.selectedUnits = [];
      syncSelectionMarkers();
      toast(`Selected ${best.kind}`);
    } else if (!boxSelect.additive) {
      state.selectedUnits = [];
      state.selectedBuilding = null;
      syncSelectionMarkers();
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
      : String(entity.kind || "").includes("tank")
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
    return entity.kind === "hq" ? 2.05 : 1.5;
  }
  const h = unitDims(entity.kind).h || 0.08;
  return h + (String(entity.kind || "").includes("tank") ? 0.1 : 0.05);
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
  if (k.includes("tank") || k.includes("vehicle") || k.includes("truck")) {
    // Half prior tank size — closer to infantry / building proportions.
    return { w: 0.18, h: 0.12, d: 0.28 };
  }
  if (k.includes("missile")) {
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

function createRangerMesh(teamColor) {
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.isInfantry = true;
  g.userData.rigVersion = 4;
  g.userData.tintParts = [];
  g.userData.walkPhase = Math.random() * Math.PI * 2;
  g.userData.moving = false;
  g.rotation.order = "YXZ";

  const camo = 0x4f6340;
  const camoDark = 0x3a4a30;
  const vest = 0x2c3326;
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

function createMissileDefenderMesh(teamColor) {
  const g = createRangerMesh(teamColor);
  // Swap rifle for a thicker tube launcher on the shoulder
  const old = g.getObjectByName("muzzleRoot");
  if (old) old.parent?.remove(old);
  const launcher = new THREE.Group();
  launcher.name = "muzzleRoot";
  const tube = new THREE.Mesh(
    new THREE.CylinderGeometry(0.016, 0.018, 0.14, 6),
    matStd(0x3a4034, { metalness: 0.35, roughness: 0.55 }),
  );
  tube.rotation.x = Math.PI / 2;
  tube.position.set(0.02, 0.14, 0.04);
  launcher.add(tube);
  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0.02, 0.14, 0.12);
  launcher.add(tip);
  const torso = g.getObjectByName("torso");
  (torso || g).add(launcher);
  return g;
}

function createTankMesh(teamColor) {
  const g = new THREE.Group();
  g.userData.isUnitRig = true;
  g.userData.tintParts = [];
  g.userData.tankRigVersion = 5;

  const hull = 0x4a5538;
  const hullDark = 0x353c2c;
  const hullLight = 0x5a6648;
  const track = 0x1a1814;
  const rubber = 0x11100e;
  const metal = 0x2a2a26;
  const rust = 0x3a3228;
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
  // Cupola MG
  add(turret, new THREE.CylinderGeometry(0.005, 0.005, 0.07, 5), matStd(0x111110), 0.045, 0.318, 0.04, Math.PI / 2, 0, 0);
  add(turret, new THREE.BoxGeometry(0.02, 0.012, 0.025), matStd(metal), 0.045, 0.318, 0.01);
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
  g.userData.unitHeight = 0.16;
  g.userData.isTank = true;
  g.userData.hullTurnRate = 1.05;
  g.userData.turretTurnRate = 0.95;
  g.userData.barrelRecoil = 0;
  // Half visual size vs prior rig (matches unitDims / server radius).
  g.scale.setScalar(0.5);
  return g;
}

function createUnitMesh(kind, teamColor) {
  const k = String(kind || "");
  if (k.includes("tank")) return createTankMesh(teamColor);
  if (k.includes("missile")) return createMissileDefenderMesh(teamColor);
  return createRangerMesh(teamColor);
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
  return Boolean(templateForKind(kind) || geometryForKind(kind));
}

function upsertMesh(entity) {
  if (!scene) return;

  let mesh = state.meshes.get(entity.id);
  const colors = entityColors(entity);

  // Replace temporary building boxes once real OBJ/STL templates are ready.
  if (mesh && entity.building && mesh.userData.isFallback && buildingHasProperModel(entity.kind)) {
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
    const isTank = kind.includes("tank");
    const needsWalkRig =
      !mesh.userData.isUnitRig ||
      (isTank &&
        (!mesh.userData.isTank ||
          !mesh.getObjectByName("tankBarrel") ||
          (mesh.userData.tankRigVersion || 0) < 5)) ||
      (!isTank && (!mesh.userData.isInfantry || (mesh.userData.rigVersion || 0) < 4));
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
  } else if (mesh.userData.isUnitRig) {
    applyUnitMotion(mesh, entity);
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
        const tank = String(entity.kind || "").includes("tank");
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

function worldMuzzlePoint(mesh) {
  const tip = mesh?.getObjectByName?.("muzzle");
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
  if (!mesh || mesh.userData.building) return;
  const dx = x1 - mesh.position.x;
  const dz = z1 - mesh.position.z;
  if (dx * dx + dz * dz < 1e-6) return;
  const yaw = Math.atan2(dx, dz);
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
    : kind.includes("missile")
      ? 0.16
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
    Sfx.stopEngine(id);
    return;
  }
  const beforeX = mesh.position.x;
  const beforeZ = mesh.position.z;
  slideToward(mesh, dt);
  const step = Math.hypot(mesh.position.x - beforeX, mesh.position.z - beforeZ);
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
  } else if (performance.now() - (mesh.userData.moveSeenAt || 0) > 200) {
    mesh.userData.moving = false;
  }
  if (mesh.userData.moving && speed > 0.05) {
    Sfx.setEngine(id, mesh.position.x, mesh.position.z, Math.min(1, 0.45 + speed));
  } else {
    Sfx.stopEngine(id);
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
      const aim =
        mesh.userData.aimYaw != null ? mesh.userData.aimYaw : mesh.userData.faceYaw;
      if (aim != null) {
        const desiredLocal = shortestAngle(0, aim - mesh.rotation.y);
        const cur = turret.rotation.y;
        const diff = shortestAngle(cur, desiredLocal);
        const maxStep = turretRate * dt;
        turret.rotation.y = cur + Math.max(-maxStep, Math.min(maxStep, diff));
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

  faceMeshToward(fromMesh, shot.x1, shot.y1);

  const start = fromMesh
    ? worldMuzzlePoint(fromMesh)
    : new THREE.Vector3(shot.x0, 0.12, shot.y0);
  // Always use server impact point so misses fly wide of the mesh.
  const endY = didHit
    ? (toMesh?.userData?.unitHeight || (toMesh?.userData?.building ? 0.6 : 0.12) || 0.12) * 0.55
    : 0.04;
  const end = new THREE.Vector3(shot.x1, endY, shot.y1);

  const kind = String(shot.kind || fromMesh?.userData?.kind || "");
  const isTank = kind.includes("tank") || !!fromMesh?.userData?.isTank;
  const isMissile = kind.includes("missile");
  const dir = new THREE.Vector3().subVectors(end, start);
  const dist = Math.max(0.05, dir.length());
  dir.normalize();

  const now = performance.now();
  const fx = {
    type: isTank ? "shell" : isMissile ? "missile" : "bullet",
    born: now,
    life: isTank ? 380 : isMissile ? 520 : 90,
    start: start.clone(),
    end: end.clone(),
    dir: dir.clone(),
    dist,
    fromMesh: fromMesh || null,
    hit: didHit,
    parts: [],
  };

  if (isTank) {
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
  } else if (isMissile) {
    const smoke = new THREE.Mesh(
      new THREE.SphereGeometry(0.03, 6, 6),
      new THREE.MeshBasicMaterial({
        color: 0xaaccee,
        transparent: true,
        opacity: 0.7,
        depthWrite: false,
      }),
    );
    smoke.position.copy(start);
    scene.add(smoke);
    fx.parts.push({ mesh: smoke, role: "blast" });

    const rocket = new THREE.Mesh(
      new THREE.CylinderGeometry(0.01, 0.014, 0.08, 5),
      new THREE.MeshBasicMaterial({ color: 0x88ddff }),
    );
    rocket.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
    rocket.position.copy(start);
    scene.add(rocket);
    fx.parts.push({ mesh: rocket, role: "projectile" });
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
  part.mesh.geometry?.dispose?.();
  if (part.mesh.material) {
    if (Array.isArray(part.mesh.material)) part.mesh.material.forEach((m) => m.dispose?.());
    else part.mesh.material.dispose?.();
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
      // Visual shudder: tiny pitch on turret
      const turret = mesh.getObjectByName("muzzleRoot");
      if (turret) {
        turret.rotation.x = -0.08 * mesh.userData.hullKick;
      }
    } else {
      const turret = mesh.getObjectByName("muzzleRoot");
      if (turret && turret.rotation.x) {
        turret.rotation.x *= 0.75;
        if (Math.abs(turret.rotation.x) < 0.001) turret.rotation.x = 0;
      }
    }
  }

  for (let i = activeFx.length - 1; i >= 0; i--) {
    const fx = activeFx[i];
    const t = Math.min(1, (now - fx.born) / fx.life);
    const pos = fx.start.clone().lerp(fx.end, t);

    for (const part of fx.parts) {
      if (part.role === "projectile") {
        part.mesh.position.copy(pos);
      } else if (part.role === "tracer") {
        const tip = fx.start.clone().addScaledVector(fx.dir, part.tipLen || 0.4);
        const head = pos.clone();
        const tail = head.clone().addScaledVector(fx.dir, -(part.tipLen || 0.4));
        // Keep tracer behind the bullet head
        const a = t < 0.15 ? fx.start : tail;
        const b = head;
        part.mesh.geometry.setFromPoints([a, b]);
        part.mesh.geometry.attributes.position.needsUpdate = true;
        part.mesh.material.opacity = 0.95 * (1 - t * 0.5);
      } else if (part.role === "blast") {
        const fade = Math.max(0, 1 - t * (fx.type === "tank_boom" ? 1.6 : 4));
        part.mesh.material.opacity = fade;
        const grow = fx.type === "shell" || fx.type === "tank_boom" ? 1 + t * 8 : 1 + t * 3;
        part.mesh.scale.setScalar(grow);
        if (fx.type === "tank_boom") {
          part.mesh.position.y = fx.start.y + t * 0.25;
        }
      } else if (part.role === "ring") {
        part.mesh.material.opacity = 0.85 * (1 - t);
        const s = 1 + t * 9;
        part.mesh.scale.set(s, s, s);
      } else if (part.role === "debris") {
        const drift = part.mesh.userData.drift;
        if (drift) {
          part.mesh.position.x = fx.start.x + drift.x * t;
          part.mesh.position.y = fx.start.y + drift.y * t;
          part.mesh.position.z = fx.start.z + drift.z * t;
        }
        part.mesh.material.opacity = 0.55 * (1 - t);
        part.mesh.scale.setScalar(1 + t * 4);
      } else if (part.role === "smoke") {
        part.mesh.material.opacity = 0.55 * (1 - t);
        part.mesh.scale.setScalar(1 + t * 5);
        part.mesh.position.copy(fx.start).addScaledVector(fx.dir, 0.04 + t * 0.08);
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
      } else if (fx.type === "missile" && fx.hit === false) {
        spawnMissImpact(fx.end, true);
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
  for (const mesh of state.meshes.values()) {
    if (mesh.userData.knock) {
      updateKnockPhysics(mesh, dt);
    } else {
      smoothUnitFacing(mesh, dt);
      updateTankDrive(mesh, dt);
      updateInfantryDrive(mesh, dt);
      updateInfantryWalk(mesh, dt, now);
    }
  }
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
  send({ t: "set_faction", faction: state.faction });
  send({
    t: "create_lobby",
    max_players: Number($("#max-players").value) || 16,
    map_size: Number($("#map-size").value) || 192,
    ffa: $("#ffa").checked,
  });
  toast("Starting match…");
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
  }
});

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
