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
      applyDelta(msg);
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
  if (!state.meshes.size) return;
  for (const mesh of state.meshes.values()) {
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
  loadExploredFromSnapshot(snapshot);
  rebuildMeshes();
  refreshLiveVision();
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
  const lookX = hq.x;
  const lookZ = hq.y;
  const dist = CAMERA_DIST;
  controls.target.set(lookX, 0, lookZ);
  camera.position.set(
    lookX,
    Math.sin(CAMERA_PITCH) * dist,
    lookZ + Math.cos(CAMERA_PITCH) * dist,
  );
  controls.update();
  send({ t: "set_focus", x: lookX, y: lookZ });
}

function updateResources(res) {
  if (!res) return;
  $("#res-supplies").textContent = res.supplies;
  $("#res-fuel").textContent = res.fuel;
  $("#res-munitions").textContent = res.munitions;
  $("#res-power").textContent = `${res.power_used}/${res.power}`;
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
        <small>from ${escapeHtml(item.from_building)} · ${item.cost_supplies}s</small>
      </button>`,
    )
    .join("");
}

function applyDelta(msg) {
  updateResources(msg.resources);
  applyExploredNew(msg.explored_new);
  for (const id of msg.removed || []) {
    state.entities.delete(id);
    const mesh = state.meshes.get(id);
    if (mesh && scene) {
      scene.remove(mesh);
      state.meshes.delete(id);
    } else {
      state.meshes.delete(id);
    }
  }
  for (const entity of msg.entities || []) {
    state.entities.set(entity.id, entity);
    if (scene) upsertMesh(entity);
  }
  playShots(msg.shots || []);
  $("#match-caption").textContent =
    `Tick ${msg.tick} · ${state.entities.size} entities · vision fog`;
}

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
const edgeMouse = { x: 0, y: 0, w: 1, h: 1, inside: false };

/** Generals-style locked pitch (radians from vertical-ish). */
const CAMERA_PITCH = Math.PI / 3.0;
/** Very close RTS camera — almost no pull-back. */
const CAMERA_DIST = 9;
const CAMERA_DIST_MIN = 7;
const CAMERA_DIST_MAX = 10;
const CAMERA_FOV = 32;
const EDGE_SCROLL_PX = 160;

const BUILDING_MODELS = {
  hq: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.4 },
  power_plant: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.1 },
  supply: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.1 },
  barracks: { type: "stl", url: "/assets/models/barracks.stl", target: 0.85 },
  war_factory: { type: "obj", obj: "/assets/models/war-factory.obj", mtl: "/assets/models/war-factory.mtl", target: 1.35 },
  turret: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 0.9 },
};

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
    return mesh;
  }
  const geo = geometryForKind(kind);
  if (geo) {
    const mesh = new THREE.Mesh(geo, fallbackMat);
    mesh.userData.building = true;
    mesh.userData.modelKind = kind;
    mesh.userData.isFallback = false;
    return mesh;
  }
  // Temporary placeholder only — replaced once models finish loading.
  const h = kind === "hq" ? 2.4 : 1.4;
  const w = kind === "hq" ? 2.2 : 1.2;
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, w), fallbackMat);
  mesh.userData.building = true;
  mesh.userData.modelKind = kind;
  mesh.userData.isFallback = true;
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
  if (buildingTemplates.hq && buildingGeometries.barracks) {
    buildingModelsReady = true;
    return true;
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
        buildingTemplates[kind] = makeObjTemplate(objBaseCache.get(key), spec.target);
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
  if (fogDataTexture) fogDataTexture.needsUpdate = true;
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

function refreshLiveVision() {
  if (!fogVisionData || !fogExploredData || !fogDataTexture) return;
  fogVisionData.fill(0);
  const you = state.match?.you;
  for (const entity of state.entities.values()) {
    if (entity.owner !== you) continue;
    const radius = visionRadiusFor(entity);
    if (!radius) continue;
    stampVisionCircle(fogVisionData, mapSize, entity.x, entity.y, radius);
    // Client-side explore while moving (server confirms via explored_new).
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

  renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
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
  canvas.addEventListener("pointerdown", onPointerDown);
  if (!onEdgePointerMove._bound) {
    window.addEventListener("pointermove", onEdgePointerMove);
    onEdgePointerMove._bound = true;
  }
  animate();
}

function onEdgePointerMove(event) {
  edgeMouse.x = event.clientX;
  edgeMouse.y = event.clientY;
  edgeMouse.w = window.innerWidth;
  edgeMouse.h = window.innerHeight;
  edgeMouse.inside = Boolean(state.match) && !$("#match-screen")?.hidden;
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
    hq: 1.4,
    war_factory: 1.35,
    barracks: 0.85,
    power_plant: 1.1,
    supply: 1.1,
    turret: 0.9,
  }[kind] ?? 1.0;
  return visual * 0.42;
}

function canPlaceBuildingAt(kind, tileX, tileY) {
  const fx = tileX + 0.5;
  const fy = tileY + 0.5;
  const placeR = buildingRadius(kind);
  for (const entity of state.entities.values()) {
    if (!entity.building) continue;
    const otherR = buildingRadius(entity.kind);
    const dx = entity.x - fx;
    const dy = entity.y - fy;
    const minDist = placeR + otherR + 0.04;
    if (dx * dx + dy * dy < minDist * minDist) return false;
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

function onPointerDown(event) {
  void enterGameFullscreen();
  const point = worldFromEvent(event);
  if (!point) return;

  const x = Math.floor(point.x);
  const y = Math.floor(point.z);

  if (event.button === 2) {
    event.preventDefault();
    // Attack enemy under cursor, else move
    let enemy = null;
    let bestDist = 1.6;
    for (const entity of state.entities.values()) {
      if (entity.team === state.match?.team) continue;
      const dx = entity.x - point.x;
      const dy = entity.y - point.z;
      const d = Math.hypot(dx, dy);
      if (d < bestDist) {
        enemy = entity;
        bestDist = d;
      }
    }
    if (enemy && state.selectedUnits.length) {
      send({ t: "attack", ids: state.selectedUnits, target_id: enemy.id });
      toast(`Attacking ${enemy.kind}`);
    } else if (state.selectedUnits.length) {
      send({ t: "move_units", ids: state.selectedUnits, x: point.x, y: point.z });
    }
    return;
  }

  if (event.button !== 0) return;

  if (state.selectedBuild) {
    if (!canPlaceBuildingAt(state.selectedBuild, x, y)) {
      toast("Buraya bina kurulamaz — yer dolu");
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

  // Select unit / building under cursor
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

  if (best?.unit) {
    state.selectedUnits = [best.id];
    state.selectedBuilding = null;
    toast(`Selected ${best.kind}`);
  } else if (best?.building) {
    state.selectedBuilding = best.id;
    state.selectedUnits = [];
    toast(`Selected ${best.kind}`);
  } else {
    send({ t: "set_focus", x: point.x, y: point.z });
  }
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
    sprite.scale.set(1.05, 0.26, 1);
  }
  sprite.position.set(0, labelHeightFor(entity), 0);
  sprite.name = "ownerLabel";
  mesh.add(sprite);
  mesh.userData.ownerLabel = sprite;
  mesh.userData.labelKey = `${name}|${colors.join(",")}`;
}

function labelHeightFor(entity) {
  if (entity.building) {
    return entity.kind === "hq" ? 1.55 : 1.15;
  }
  return (unitDims(entity.kind).h || 0.22) + 0.28;
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
  sprite.name = "hpBar";
  const baseH = labelHeightFor(entity);
  const load = activeLoadProgress(entity);
  const y = load
    ? Math.max(0.22, baseH - 0.62)
    : Math.max(0.38, baseH - 0.36);
  sprite.position.set(0, y, 0);
  mesh.add(sprite);
  mesh.userData.hpBar = sprite;
  mesh.userData.hpBarKey = key;
}

function unitDims(kind) {
  const k = String(kind || "");
  // Vehicles a bit larger than infantry, still small vs buildings.
  if (k.includes("tank") || k.includes("vehicle") || k.includes("truck")) {
    return { w: 0.22, h: 0.14, d: 0.32 };
  }
  if (k.includes("missile")) {
    return { w: 0.08, h: 0.16, d: 0.08 };
  }
  // Ranger / infantry — low-poly humanoid height
  return { w: 0.09, h: 0.22, d: 0.09 };
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
  g.userData.tintParts = [];
  g.userData.walkPhase = Math.random() * Math.PI * 2;
  g.userData.moving = false;

  const camo = 0x4a5c38;
  const dark = 0x2a3224;
  const boot = 0x1a1814;
  const skin = 0xc4a882;
  const gun = 0x2c2c2c;
  const accent = teamColor >>> 0;

  const add = (parent, geo, mat, x, y, z, tint = false) => {
    const m = new THREE.Mesh(geo, mat);
    m.position.set(x, y, z);
    if (tint) g.userData.tintParts.push(m);
    parent.add(m);
    return m;
  };

  // Legs — pivots at hip so they can swing while walking
  const leftLeg = new THREE.Group();
  leftLeg.name = "leftLeg";
  leftLeg.position.set(-0.018, 0.07, 0);
  add(leftLeg, new THREE.BoxGeometry(0.028, 0.07, 0.03), matStd(camo), 0, -0.035, 0);
  add(leftLeg, new THREE.BoxGeometry(0.03, 0.02, 0.04), matStd(boot), 0, -0.06, 0.005);
  g.add(leftLeg);

  const rightLeg = new THREE.Group();
  rightLeg.name = "rightLeg";
  rightLeg.position.set(0.018, 0.07, 0);
  add(rightLeg, new THREE.BoxGeometry(0.028, 0.07, 0.03), matStd(camo), 0, -0.035, 0);
  add(rightLeg, new THREE.BoxGeometry(0.03, 0.02, 0.04), matStd(boot), 0, -0.06, 0.005);
  g.add(rightLeg);

  // Upper body group (bobs slightly while walking)
  const torso = new THREE.Group();
  torso.name = "torso";
  add(torso, new THREE.BoxGeometry(0.07, 0.08, 0.045), matStd(camo), 0, 0.11, 0);
  add(torso, new THREE.BoxGeometry(0.074, 0.05, 0.05), matStd(dark), 0, 0.105, 0.002);
  add(
    torso,
    new THREE.BoxGeometry(0.076, 0.012, 0.052),
    matStd(accent, { roughness: 0.55 }),
    0,
    0.12,
    0.003,
    true,
  );

  const leftArm = new THREE.Group();
  leftArm.name = "leftArm";
  leftArm.position.set(-0.048, 0.13, 0.01);
  add(leftArm, new THREE.BoxGeometry(0.022, 0.06, 0.022), matStd(camo), 0, -0.025, 0);
  torso.add(leftArm);

  const rightArm = new THREE.Group();
  rightArm.name = "rightArm";
  rightArm.position.set(0.048, 0.125, 0.02);
  add(rightArm, new THREE.BoxGeometry(0.022, 0.055, 0.022), matStd(camo), 0, -0.025, 0);
  torso.add(rightArm);

  add(torso, new THREE.BoxGeometry(0.038, 0.038, 0.038), matStd(skin), 0, 0.168, 0);
  add(torso, new THREE.BoxGeometry(0.046, 0.022, 0.05), matStd(dark), 0, 0.185, 0.002);
  add(
    torso,
    new THREE.BoxGeometry(0.048, 0.01, 0.02),
    matStd(accent, { roughness: 0.5 }),
    0,
    0.178,
    0.018,
    true,
  );
  add(torso, new THREE.BoxGeometry(0.04, 0.045, 0.025), matStd(dark), 0, 0.115, -0.032);

  // Rifle (held across body, muzzle toward +Z)
  const rifle = new THREE.Group();
  rifle.name = "muzzleRoot";
  const stock = new THREE.Mesh(new THREE.BoxGeometry(0.018, 0.02, 0.04), matStd(0x3a2a1a));
  stock.position.set(0.03, 0.1, -0.01);
  rifle.add(stock);
  const body = new THREE.Mesh(
    new THREE.BoxGeometry(0.016, 0.016, 0.1),
    matStd(gun, { metalness: 0.55, roughness: 0.4 }),
  );
  body.position.set(0.035, 0.105, 0.04);
  rifle.add(body);
  const barrel = new THREE.Mesh(
    new THREE.BoxGeometry(0.01, 0.01, 0.06),
    matStd(0x111111, { metalness: 0.7, roughness: 0.35 }),
  );
  barrel.position.set(0.035, 0.108, 0.11);
  rifle.add(barrel);
  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0.035, 0.108, 0.145);
  rifle.add(tip);
  torso.add(rifle);

  g.add(torso);
  g.userData.unitHeight = 0.22;
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

  const hull = 0x4a5538;
  const track = 0x222018;
  const accent = teamColor >>> 0;

  const add = (geo, mat, x, y, z, rx = 0, ry = 0, rz = 0, tint = false) => {
    const m = new THREE.Mesh(geo, mat);
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, rz);
    if (tint) g.userData.tintParts.push(m);
    g.add(m);
    return m;
  };

  add(new THREE.BoxGeometry(0.2, 0.07, 0.28), matStd(hull), 0, 0.06, 0);
  add(new THREE.BoxGeometry(0.04, 0.05, 0.3), matStd(track), -0.12, 0.035, 0);
  add(new THREE.BoxGeometry(0.04, 0.05, 0.3), matStd(track), 0.12, 0.035, 0);
  add(new THREE.BoxGeometry(0.18, 0.02, 0.06), matStd(accent, { roughness: 0.5 }), 0, 0.1, -0.08, 0, 0, 0, true);

  const turret = new THREE.Group();
  turret.name = "muzzleRoot";
  const cupola = new THREE.Mesh(new THREE.BoxGeometry(0.12, 0.06, 0.14), matStd(0x3d4730));
  cupola.position.set(0, 0.12, -0.02);
  turret.add(cupola);
  const barrel = new THREE.Mesh(
    new THREE.CylinderGeometry(0.012, 0.015, 0.22, 6),
    matStd(0x1a1a16, { metalness: 0.65, roughness: 0.4 }),
  );
  barrel.rotation.x = Math.PI / 2;
  barrel.position.set(0, 0.125, 0.12);
  turret.add(barrel);
  const tip = new THREE.Object3D();
  tip.name = "muzzle";
  tip.position.set(0, 0.125, 0.24);
  turret.add(tip);
  g.add(turret);

  g.userData.unitHeight = 0.16;
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

  // Upgrade old unit boxes / static infantry to walk-capable rigs.
  if (mesh && entity.unit) {
    const kind = String(entity.kind || "");
    const isTank = kind.includes("tank");
    const needsWalkRig =
      !mesh.userData.isUnitRig || (!isTank && !mesh.userData.isInfantry);
    if (needsWalkRig) {
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
    scene.add(mesh);
    state.meshes.set(entity.id, mesh);

    attachOwnerMarkings(mesh, entity);
  }

  if (mesh.userData.building) {
    mesh.position.set(entity.x, 0, entity.y);
  } else if (mesh.userData.isUnitRig) {
    const prevX = mesh.userData.lastX;
    const prevZ = mesh.userData.lastZ;
    mesh.position.set(entity.x, 0, entity.y);
    tintUnitMesh(mesh, colors);
    if (prevX != null && prevZ != null) {
      const dx = entity.x - prevX;
      const dz = entity.y - prevZ;
      const moved2 = dx * dx + dz * dz;
      mesh.userData.moving = moved2 > 1e-8;
      if (moved2 > 1e-8) {
        // Face travel direction; smooth toward it in animate().
        mesh.userData.faceYaw = Math.atan2(dx, dz);
      }
    } else {
      mesh.userData.moving = false;
    }
    mesh.userData.lastX = entity.x;
    mesh.userData.lastZ = entity.y;
    mesh.userData.moveSeenAt = performance.now();
  } else {
    mesh.position.set(entity.x, unitDims(entity.kind).h * 0.5, entity.y);
  }

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
        sprite.scale.set(1.05, 0.26, 1);
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
  mesh.userData.faceYaw = Math.atan2(dx, dz);
  // Snap quickly when firing so the muzzle lines up with the tracer.
  mesh.rotation.y = mesh.userData.faceYaw;
}

function smoothUnitFacing(mesh, dt) {
  if (!mesh?.userData?.isUnitRig) return;
  if (mesh.userData.faceYaw == null) return;
  const target = mesh.userData.faceYaw;
  let cur = mesh.rotation.y;
  let diff = target - cur;
  // Shortest angle
  while (diff > Math.PI) diff -= Math.PI * 2;
  while (diff < -Math.PI) diff += Math.PI * 2;
  const turn = Math.min(1, dt * 12);
  mesh.rotation.y = cur + diff * turn;
}

function updateInfantryWalk(mesh, dt, now) {
  if (!mesh?.userData?.isInfantry) return;
  const leftLeg = mesh.getObjectByName("leftLeg");
  const rightLeg = mesh.getObjectByName("rightLeg");
  const leftArm = mesh.getObjectByName("leftArm");
  const rightArm = mesh.getObjectByName("rightArm");
  const torso = mesh.getObjectByName("torso");
  if (!leftLeg || !rightLeg) return;

  // Keep walking briefly between network ticks so the cycle doesn't stutter.
  const recentlyMoved =
    mesh.userData.moving ||
    (mesh.userData.moveSeenAt && now - mesh.userData.moveSeenAt < 180);

  if (recentlyMoved) {
    mesh.userData.walkPhase = (mesh.userData.walkPhase || 0) + dt * 11;
    const swing = Math.sin(mesh.userData.walkPhase) * 0.55;
    leftLeg.rotation.x = swing;
    rightLeg.rotation.x = -swing;
    if (leftArm) leftArm.rotation.x = -swing * 0.45;
    if (rightArm) rightArm.rotation.x = swing * 0.35;
    if (torso) {
      torso.position.y = Math.abs(Math.sin(mesh.userData.walkPhase * 2)) * 0.008;
      torso.rotation.z = Math.sin(mesh.userData.walkPhase) * 0.04;
    }
  } else {
    // Ease back to idle stance
    const ease = Math.min(1, dt * 10);
    leftLeg.rotation.x *= 1 - ease;
    rightLeg.rotation.x *= 1 - ease;
    if (leftArm) leftArm.rotation.x *= 1 - ease;
    if (rightArm) rightArm.rotation.x *= 1 - ease;
    if (torso) {
      torso.position.y *= 1 - ease;
      torso.rotation.z *= 1 - ease;
    }
  }
}

function spawnShotFx(shot) {
  if (!scene) return;
  const fromMesh = state.meshes.get(shot.from);
  const toMesh = state.meshes.get(shot.to);
  const you = state.match?.you;
  const fromEnt = state.entities.get(shot.from);
  const toEnt = state.entities.get(shot.to);
  const involvesYou =
    (fromEnt && fromEnt.owner === you) || (toEnt && toEnt.owner === you);

  faceMeshToward(fromMesh, shot.x1, shot.y1);

  const start = fromMesh
    ? worldMuzzlePoint(fromMesh)
    : new THREE.Vector3(shot.x0, 0.12, shot.y0);
  const end = toMesh
    ? new THREE.Vector3(
        toMesh.position.x,
        (toMesh.userData.unitHeight || (toMesh.userData.building ? 0.6 : 0.12)) * 0.55,
        toMesh.position.z,
      )
    : new THREE.Vector3(shot.x1, 0.12, shot.y1);

  const kind = String(shot.kind || "");
  const isTank = kind.includes("tank");
  const isMissile = kind.includes("missile") || kind === "turret";

  // Tracer streak
  const geo = new THREE.BufferGeometry().setFromPoints([start, end]);
  const mat = new THREE.LineBasicMaterial({
    color: isTank ? 0xff9933 : isMissile ? 0x66ddff : 0xffe066,
    transparent: true,
    opacity: involvesYou ? 0.95 : 0.7,
    depthTest: true,
  });
  const line = new THREE.Line(geo, mat);
  scene.add(line);

  // Muzzle flash sprite (simple bright sphere)
  const flash = new THREE.Mesh(
    new THREE.SphereGeometry(isTank ? 0.06 : 0.035, 6, 6),
    new THREE.MeshBasicMaterial({
      color: 0xfff2a8,
      transparent: true,
      opacity: 0.95,
      depthWrite: false,
    }),
  );
  flash.position.copy(start);
  scene.add(flash);

  // Impact spark
  const impact = new THREE.Mesh(
    new THREE.SphereGeometry(isTank ? 0.05 : 0.03, 6, 6),
    new THREE.MeshBasicMaterial({
      color: 0xff5533,
      transparent: true,
      opacity: 0.9,
      depthWrite: false,
    }),
  );
  impact.position.copy(end);
  scene.add(impact);

  activeFx.push({
    line,
    flash,
    impact,
    born: performance.now(),
    life: isTank ? 220 : 160,
  });
}

function playShots(shots) {
  for (const shot of shots || []) {
    spawnShotFx(shot);
  }
}

function updateCombatFx(now) {
  for (let i = activeFx.length - 1; i >= 0; i--) {
    const fx = activeFx[i];
    const t = (now - fx.born) / fx.life;
    if (t >= 1) {
      scene?.remove(fx.line);
      scene?.remove(fx.flash);
      scene?.remove(fx.impact);
      fx.line.geometry.dispose();
      fx.line.material.dispose();
      fx.flash.geometry.dispose();
      fx.flash.material.dispose();
      fx.impact.geometry.dispose();
      fx.impact.material.dispose();
      activeFx.splice(i, 1);
      continue;
    }
    const fade = 1 - t;
    fx.line.material.opacity = fade * 0.85;
    fx.flash.material.opacity = fade;
    fx.flash.scale.setScalar(1 + t * 1.8);
    fx.impact.material.opacity = fade * 0.9;
    fx.impact.scale.setScalar(1 + t * 2.2);
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
  refreshLiveVision();
  for (const mesh of state.meshes.values()) {
    smoothUnitFacing(mesh, dt);
    updateInfantryWalk(mesh, dt, now);
  }
  updateCombatFx(now);
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
