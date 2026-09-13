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
    // Browsers may block until a user gesture; create/join clicks also call this.
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
  void enterGameFullscreen();
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
    if (mesh) {
      scene.remove(mesh);
      state.meshes.delete(id);
    }
  }
  for (const entity of msg.entities || []) {
    state.entities.set(entity.id, entity);
    upsertMesh(entity);
  }
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
const CAMERA_PITCH = Math.PI / 3.15;
/** Close RTS camera — no wide pull-back. */
const CAMERA_DIST = 18;
const CAMERA_DIST_MIN = 14;
const CAMERA_DIST_MAX = 20;
const EDGE_SCROLL_PX = 160;

const BUILDING_MODELS = {
  hq: { type: "obj", obj: "/assets/models/command-center.obj", mtl: "/assets/models/command-center.mtl", target: 1.4 },
  power_plant: { type: "stl", url: "/assets/models/command-center.stl", target: 1.1 },
  supply: { type: "stl", url: "/assets/models/command-center.stl", target: 1.1 },
  barracks: { type: "stl", url: "/assets/models/barracks.stl", target: 0.85 },
  war_factory: { type: "stl", url: "/assets/models/command-center.stl", target: 1.2 },
  turret: { type: "stl", url: "/assets/models/command-center.stl", target: 0.9 },
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

async function prepareObjTemplate(objUrl, mtlUrl, targetSize) {
  const mtlLoader = new MTLLoader();
  const materials = await mtlLoader.loadAsync(mtlUrl);
  materials.preload();
  const objLoader = new OBJLoader();
  objLoader.setMaterials(materials);
  const root = await objLoader.loadAsync(objUrl);

  root.traverse((child) => {
    if (child.isMesh) {
      child.castShadow = true;
      child.receiveShadow = true;
      const mats = Array.isArray(child.material) ? child.material : [child.material];
      for (const mat of mats) {
        if (!mat) continue;
        mat.side = THREE.FrontSide;
        if (mat.map) mat.map.colorSpace = THREE.SRGBColorSpace;
      }
    }
  });

  // Fit + ground like STL path (Blender OBJ is usually Y-up already).
  const box = new THREE.Box3().setFromObject(root);
  const size = new THREE.Vector3();
  const center = new THREE.Vector3();
  box.getSize(size);
  box.getCenter(center);
  root.position.sub(center);
  const maxDim = Math.max(size.x, size.y, size.z) || 1;
  root.scale.setScalar(targetSize / maxDim);
  root.updateMatrixWorld(true);
  const grounded = new THREE.Box3().setFromObject(root);
  root.position.y -= grounded.min.y;

  const wrapper = new THREE.Group();
  wrapper.add(root);
  wrapper.userData.keepMtlColors = true;
  return wrapper;
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
    mesh.userData.keepMtlColors = true;
    mesh.userData.building = true;
    return mesh;
  }
  const geo = geometryForKind(kind);
  if (geo) {
    return new THREE.Mesh(geo, fallbackMat);
  }
  const h = kind === "hq" ? 2.4 : 1.4;
  const w = kind === "hq" ? 2.2 : 1.2;
  return new THREE.Mesh(new THREE.BoxGeometry(w, h, w), fallbackMat);
}

function setBuildingOpacity(root, opacity) {
  root.traverse((child) => {
    if (!child.isMesh || !child.material) return;
    const mats = Array.isArray(child.material) ? child.material : [child.material];
    for (const mat of mats) {
      mat.transparent = opacity < 1;
      mat.opacity = opacity;
      mat.depthWrite = opacity >= 1;
    }
  });
}

async function ensureBuildingModel() {
  if (buildingTemplates.hq && buildingGeometries.barracks) return true;
  if (buildingModelsPromise) return buildingModelsPromise;

  buildingModelsPromise = (async () => {
    const stlCache = new Map();
    for (const [kind, spec] of Object.entries(BUILDING_MODELS)) {
      if (spec.type === "obj") {
        buildingTemplates[kind] = await prepareObjTemplate(spec.obj, spec.mtl, spec.target);
      } else {
        const key = `${spec.url}|${spec.target}`;
        if (!stlCache.has(key)) {
          stlCache.set(key, prepareStlGeometry(spec.url, spec.target));
        }
        buildingGeometries[kind] = await stlCache.get(key);
      }
    }
    return true;
  })();

  try {
    return await buildingModelsPromise;
  } catch (error) {
    console.error(error);
    buildingModelsPromise = null;
    toast("Building models failed to load — using fallbacks");
    return null;
  }
}

async function loadTerrainTexture(mapSize) {
  try {
    const loader = new THREE.TextureLoader();
    const tex = await loader.loadAsync("/assets/terrain.jpg");
    tex.wrapS = THREE.RepeatWrapping;
    tex.wrapT = THREE.RepeatWrapping;
    tex.anisotropy = 8;
    tex.colorSpace = THREE.SRGBColorSpace;
    const tiles = Math.max(12, Math.round(mapSize / 10));
    tex.repeat.set(tiles, tiles);
    return tex;
  } catch (error) {
    console.error(error);
    return makeFallbackTerrainTexture(mapSize);
  }
}

function makeFallbackTerrainTexture(mapSize) {
  const canvas = document.createElement("canvas");
  canvas.width = 256;
  canvas.height = 256;
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = "#4a5f34";
  ctx.fillRect(0, 0, 256, 256);
  for (let i = 0; i < 900; i++) {
    const x = Math.random() * 256;
    const y = Math.random() * 256;
    const s = 1 + Math.random() * 3;
    ctx.fillStyle = `rgba(${60 + Math.random() * 50},${80 + Math.random() * 60},${40 + Math.random() * 30},${0.15 + Math.random() * 0.35})`;
    ctx.fillRect(x, y, s, s);
  }
  const tex = new THREE.CanvasTexture(canvas);
  tex.wrapS = THREE.RepeatWrapping;
  tex.wrapT = THREE.RepeatWrapping;
  const tiles = Math.max(12, Math.round(mapSize / 10));
  tex.repeat.set(tiles, tiles);
  tex.colorSpace = THREE.SRGBColorSpace;
  return tex;
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
  const mat = new THREE.ShaderMaterial({
    transparent: true,
    depthWrite: false,
    uniforms: {
      uMap: { value: fogDataTexture },
      uSize: { value: size },
    },
    vertexShader: `
      varying vec3 vWorldPos;
      void main() {
        vec4 world = modelMatrix * vec4(position, 1.0);
        vWorldPos = world.xyz;
        gl_Position = projectionMatrix * viewMatrix * world;
      }
    `,
    fragmentShader: `
      uniform sampler2D uMap;
      uniform float uSize;
      varying vec3 vWorldPos;
      void main() {
        vec2 uv = vec2(vWorldPos.x, vWorldPos.z) / uSize;
        if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
          gl_FragColor = vec4(0.02, 0.03, 0.02, 0.92);
          return;
        }
        vec4 sample = texture2D(uMap, uv);
        float explored = sample.r;
        float visible = sample.g;
        if (explored < 0.5) {
          gl_FragColor = vec4(0.02, 0.03, 0.02, 0.92);
          return;
        }
        if (visible < 0.5) {
          gl_FragColor = vec4(0.05, 0.07, 0.04, 0.62);
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

  // Sparse dry-grass tufts (thin boxes)
  const grassMat = new THREE.MeshStandardMaterial({
    color: 0x6a8038,
    roughness: 1,
    flatShading: true,
  });
  const tuftCount = Math.min(120, Math.floor(size * 0.45));
  for (let i = 0; i < tuftCount; i++) {
    const h = 0.25 + Math.random() * 0.45;
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.08, h, 0.08), grassMat);
    mesh.position.set(
      2 + Math.random() * (size - 4),
      h * 0.5,
      2 + Math.random() * (size - 4),
    );
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
    42,
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
    color: terrainTexture ? 0xb8c898 : 0x3d5230,
    roughness: 0.95,
    metalness: 0.02,
    flatShading: false,
  });
  ground = new THREE.Mesh(geo, mat);
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(cx, 0, cz);
  ground.receiveShadow = true;
  scene.add(ground);

  // Subtle tile hint — not a loud debug grid.
  const grid = new THREE.GridHelper(
    size,
    Math.min(size, 96),
    0x000000,
    0x2a3820,
  );
  grid.material.opacity = 0.22;
  grid.material.transparent = true;
  grid.position.set(cx, 0.025, cz);
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
  mesh.userData.disposeMaterials = !mesh.userData.keepMtlColors;

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
  const x = Math.floor(point.x) + 0.5;
  const z = Math.floor(point.z) + 0.5;
  ghost.position.set(x, 0, z);
  ghost.visible = true;
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
  return 1.15;
}

function colorFor(entity) {
  return entityColors(entity)[0];
}

function upsertMesh(entity) {
  let mesh = state.meshes.get(entity.id);
  const colors = entityColors(entity);

  if (!mesh) {
    const mat = new THREE.MeshStandardMaterial({
      color: colors[0],
      metalness: 0.15,
      roughness: 0.72,
    });

    if (entity.building) {
      mesh = createBuildingMesh(entity.kind, mat);
    } else {
      mesh = new THREE.Mesh(new THREE.BoxGeometry(0.28, 0.36, 0.28), mat);
    }

    mesh.userData.id = entity.id;
    mesh.userData.building = !!entity.building;
    scene.add(mesh);
    state.meshes.set(entity.id, mesh);

    attachOwnerMarkings(mesh, entity);

    if (entity.flag) {
      const flag = new THREE.Mesh(
        new THREE.BoxGeometry(0.12, 1.1, 0.35),
        new THREE.MeshStandardMaterial({ color: colors[2] }),
      );
      flag.position.set(0.9, 1.4, 0);
      mesh.add(flag);
    }
  }

  if (mesh.userData.building) {
    mesh.position.set(entity.x, 0, entity.y);
  } else {
    mesh.position.set(entity.x, 0.2, entity.y);
  }

  const building = entity.progress != null && entity.progress < 1;
  const opacity = building ? 0.55 : 1;
  if (mesh.userData.keepMtlColors) {
    setBuildingOpacity(mesh, opacity);
  } else if (mesh.material) {
    mesh.material.color.setHex(colors[0]);
    mesh.material.opacity = opacity;
    mesh.material.transparent = opacity < 1;
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

function animate() {
  requestAnimationFrame(animate);
  if (!renderer) return;
  applyEdgePan();
  controls?.update();
  refreshLiveVision();
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
