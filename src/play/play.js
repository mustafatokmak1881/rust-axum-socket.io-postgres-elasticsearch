import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { STLLoader } from "three/addons/loaders/STLLoader.js";

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
  faction: "usa",
  ready: false,
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

function connect() {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${proto}://${location.host}/ws`);
  state.ws = ws;

  ws.addEventListener("open", () => send({ t: "hello" }));
  ws.addEventListener("close", () => {
    toast("Disconnected — refreshing…");
    setTimeout(() => location.reload(), 1500);
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
    case "match_start":
      enterMatch(msg.snapshot);
      break;
    case "delta":
      applyDelta(msg);
      break;
    case "match_end":
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

function enterMatch(snapshot) {
  state.match = snapshot;
  state.entities.clear();
  for (const entity of snapshot.entities || []) {
    state.entities.set(entity.id, entity);
  }
  $("#lobby-screen").hidden = true;
  $("#match-screen").hidden = false;
  void setupMatchScene(snapshot);
}

async function setupMatchScene(snapshot) {
  updateResources(snapshot.resources);
  renderBuildList(snapshot.buildable || []);
  renderUnitList(snapshot.trainable || []);
  await ensureBuildingModel();
  initThree(snapshot.map_size);
  rebuildMeshes();
  toast("Match live — move mouse to screen edges to pan");
}

function updateResources(res) {
  if (!res) return;
  $("#res-supplies").textContent = res.supplies;
  $("#res-fuel").textContent = res.fuel;
  $("#res-munitions").textContent = res.munitions;
  $("#res-power").textContent = `${res.power_used}/${res.power}`;
}

function renderBuildList(items) {
  $("#build-list").innerHTML = items
    .map(
      (item) => `
      <button type="button" class="build-item" data-kind="${escapeHtml(item.kind)}">
        <strong>${escapeHtml(item.name)}</strong>
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
    `Tick ${msg.tick} · ${state.entities.size} entities · edge-scroll`;
}

/* ---------- Three.js ---------- */

let renderer, scene, camera, controls, ground, raycaster, pointer;
let mapSize = 192;
let buildingGeometry = null;
let buildingModelPromise = null;
const edgeMouse = { x: 0, y: 0, w: 1, h: 1, inside: false };

/** Generals-style locked pitch (radians from vertical-ish). */
const CAMERA_PITCH = Math.PI / 3.35;
const EDGE_SCROLL_PX = 42;

async function ensureBuildingModel() {
  if (buildingGeometry) return buildingGeometry;
  if (buildingModelPromise) return buildingModelPromise;

  buildingModelPromise = (async () => {
    const loader = new STLLoader();
    const geo = await loader.loadAsync("/assets/models/command-center.stl");
    // Most CAD STLs are Z-up; Three.js is Y-up — stand the building upright.
    geo.rotateX(-Math.PI / 2);
    geo.computeVertexNormals();
    geo.center();
    geo.computeBoundingBox();
    const box = geo.boundingBox;
    const size = new THREE.Vector3();
    box.getSize(size);
    const maxDim = Math.max(size.x, size.y, size.z) || 1;
    const target = 2.6;
    const s = target / maxDim;
    geo.scale(s, s, s);
    geo.computeBoundingBox();
    // Feet on the ground (Y = 0).
    geo.translate(0, -geo.boundingBox.min.y, 0);
    buildingGeometry = geo;
    return geo;
  })();

  try {
    return await buildingModelPromise;
  } catch (error) {
    console.error(error);
    buildingModelPromise = null;
    toast("STL model failed to load — using cubes");
    return null;
  }
}

function initThree(size) {
  mapSize = size;
  const canvas = $("#viewport");

  if (renderer) {
    // Re-entering a match: dispose previous GL context lightly by clearing scene refs.
    controls?.dispose();
  }

  renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);

  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1a2a14);
  scene.fog = new THREE.Fog(0x1a2a14, Math.max(60, size * 0.55), Math.max(160, size * 1.4));

  camera = new THREE.PerspectiveCamera(
    42,
    canvas.clientWidth / canvas.clientHeight,
    0.1,
    Math.max(800, size * 4),
  );

  const cx = size / 2;
  const cz = size / 2;
  const dist = Math.min(48, size * 0.28);
  camera.position.set(
    cx,
    Math.sin(CAMERA_PITCH) * dist,
    cz + Math.cos(CAMERA_PITCH) * dist,
  );

  controls = new OrbitControls(camera, canvas);
  controls.target.set(cx, 0, cz);
  // Fixed Generals angle: no free rotate.
  controls.enableRotate = false;
  controls.enablePan = false;
  controls.enableZoom = true;
  controls.minDistance = 12;
  controls.maxDistance = Math.max(90, size * 0.85);
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.zoomSpeed = 1.05;
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
    color: 0x3d5230,
    wireframe: false,
    flatShading: true,
  });
  ground = new THREE.Mesh(geo, mat);
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(cx, 0, cz);
  ground.receiveShadow = true;
  scene.add(ground);

  const grid = new THREE.GridHelper(size, Math.min(size, 128), 0x5a6a40, 0x2a3a20);
  grid.position.set(cx, 0.02, cz);
  scene.add(grid);

  raycaster = new THREE.Raycaster();
  pointer = new THREE.Vector2();
  state.meshes.clear();

  window.addEventListener("resize", onResize);
  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointermove", onEdgePointerMove);
  canvas.addEventListener("pointerleave", onEdgePointerLeave);
  animate();
}

function onEdgePointerMove(event) {
  const canvas = $("#viewport");
  const rect = canvas.getBoundingClientRect();
  edgeMouse.x = event.clientX - rect.left;
  edgeMouse.y = event.clientY - rect.top;
  edgeMouse.w = rect.width;
  edgeMouse.h = rect.height;
  edgeMouse.inside = true;
}

function onEdgePointerLeave() {
  edgeMouse.inside = false;
}

function applyEdgePan() {
  if (!controls || !camera || !edgeMouse.inside) return;

  let dx = 0;
  let dz = 0;
  const e = EDGE_SCROLL_PX;
  const edgeSpeed = 0.7 * (controls.getDistance() / 26);

  if (edgeMouse.x < e) {
    dx -= edgeSpeed * (1 - edgeMouse.x / e);
  } else if (edgeMouse.x > edgeMouse.w - e) {
    dx += edgeSpeed * (1 - (edgeMouse.w - edgeMouse.x) / e);
  }
  if (edgeMouse.y < e) {
    dz -= edgeSpeed * (1 - edgeMouse.y / e);
  } else if (edgeMouse.y > edgeMouse.h - e) {
    dz += edgeSpeed * (1 - (edgeMouse.h - edgeMouse.y) / e);
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
  canvas.width = 256;
  canvas.height = 64;
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, canvas.width, canvas.height);

  // Tricolor identity bar
  const bandW = canvas.width / 3;
  for (let i = 0; i < 3; i++) {
    ctx.fillStyle = `#${(colors[i] >>> 0).toString(16).padStart(6, "0")}`;
    ctx.fillRect(i * bandW, 0, bandW, 10);
  }

  ctx.font = "bold 22px Segoe UI, Tahoma, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineWidth = 4;
  ctx.strokeStyle = "rgba(0,0,0,0.85)";
  ctx.fillStyle = "#f4f1e8";
  const label = String(text || "?").slice(0, 18);
  ctx.strokeText(label, canvas.width / 2, 38);
  ctx.fillText(label, canvas.width / 2, 38);

  const texture = new THREE.CanvasTexture(canvas);
  texture.needsUpdate = true;
  const mat = new THREE.SpriteMaterial({
    map: texture,
    transparent: true,
    depthTest: false,
  });
  const sprite = new THREE.Sprite(mat);
  sprite.scale.set(3.2, 0.8, 1);
  sprite.center.set(0.5, 0);
  return sprite;
}

function attachOwnerMarkings(mesh, entity) {
  const colors = entityColors(entity);
  const name = entity.owner_name || "Player";

  // Three vertical color bands on the building
  const bandGeo = new THREE.BoxGeometry(0.22, entity.kind === "hq" ? 1.5 : 1.05, 0.08);
  for (let i = 0; i < 3; i++) {
    const band = new THREE.Mesh(
      bandGeo,
      new THREE.MeshStandardMaterial({
        color: colors[i],
        metalness: 0.05,
        roughness: 0.55,
        emissive: colors[i],
        emissiveIntensity: 0.12,
      }),
    );
    const y = entity.kind === "hq" ? 1.1 : 0.85;
    band.position.set(-0.45 + i * 0.45, y, 0.85);
    band.name = `colorBand${i}`;
    mesh.add(band);
  }

  const sprite = makeNameSprite(name, colors);
  sprite.position.set(0, entity.kind === "hq" ? 3.2 : 2.4, 0);
  sprite.name = "ownerLabel";
  mesh.add(sprite);
  mesh.userData.ownerLabel = sprite;
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

    if (entity.building && buildingGeometry) {
      mesh = new THREE.Mesh(buildingGeometry, mat);
      // Temporary: every building uses command-center.stl until per-kind STLs exist.
      const scale = entity.kind === "hq" ? 1.15 : 0.85;
      mesh.scale.setScalar(scale);
    } else if (entity.building) {
      const h = entity.kind === "hq" ? 2.4 : 1.4;
      const w = entity.kind === "hq" ? 2.2 : 1.2;
      mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, w), mat);
    } else {
      mesh = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.7, 0.55), mat);
    }

    mesh.userData.id = entity.id;
    mesh.userData.building = !!entity.building;
    scene.add(mesh);
    state.meshes.set(entity.id, mesh);

    if (entity.building) {
      attachOwnerMarkings(mesh, entity);
    } else {
      // Units: small tricolor fin for ownership at a glance.
      for (let i = 0; i < 3; i++) {
        const fin = new THREE.Mesh(
          new THREE.BoxGeometry(0.12, 0.35, 0.05),
          new THREE.MeshStandardMaterial({ color: colors[i] }),
        );
        fin.position.set(-0.18 + i * 0.18, 0.55, 0.2);
        mesh.add(fin);
      }
    }

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
    mesh.position.set(entity.x, 0.35, entity.y);
  }

  mesh.material.color.setHex(colors[0]);
  const building = entity.progress != null && entity.progress < 1;
  mesh.material.opacity = building ? 0.55 : 1;
  mesh.material.transparent = mesh.material.opacity < 1;

  // Refresh label if owner name/colors changed (rare).
  const label = mesh.userData.ownerLabel;
  if (label && entity.building) {
    const key = `${entity.owner_name}|${colors.join(",")}`;
    if (mesh.userData.labelKey !== key) {
      mesh.remove(label);
      label.material.map?.dispose();
      label.material.dispose();
      const sprite = makeNameSprite(entity.owner_name || "Player", colors);
      sprite.position.set(0, entity.kind === "hq" ? 3.2 : 2.4, 0);
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
  joinMatchId(btn.dataset.join);
});

$("#build-list").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-kind]");
  if (!btn) return;
  state.selectedBuild = btn.dataset.kind;
  document.querySelectorAll(".build-item").forEach((el) => {
    el.classList.toggle("on", el === btn);
  });
  $("#build-detail").textContent = `Placing ${btn.dataset.kind} — click any empty tile`;
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
