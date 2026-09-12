import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

const $ = (sel) => document.querySelector(sel);

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
      state.lobby = msg.lobby;
      renderLobby();
      break;
    case "lobby_left":
      state.lobby = null;
      $("#lobby-card").hidden = true;
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

function renderLobby() {
  const lobby = state.lobby;
  if (!lobby) return;
  $("#lobby-card").hidden = false;
  $("#lobby-id-label").textContent = lobby.id;
  $("#lobby-slots").innerHTML = lobby.slots
    .map(
      (slot) => `
      <li>
        <span>${escapeHtml(slot.name)} · ${slot.faction.toUpperCase()} · T${slot.team}</span>
        <span>${slot.ready ? "READY" : "…"} ${slot.flag ? `· ${slot.flag}` : ""}</span>
      </li>`,
    )
    .join("");
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
  updateResources(snapshot.resources);
  renderBuildList(snapshot.buildable || []);
  renderUnitList(snapshot.trainable || []);
  initThree(snapshot.map_size);
  rebuildMeshes();
  toast("Match live — build from the left rail");
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
    `Tick ${msg.tick} · entities ${state.entities.size} · LMB place/select · RMB move selected`;
}

/* ---------- Three.js ---------- */

let renderer, scene, camera, controls, ground, raycaster, pointer;
let mapSize = 96;

function initThree(size) {
  mapSize = size;
  const canvas = $("#viewport");
  renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);

  scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1a2a14);
  scene.fog = new THREE.Fog(0x1a2a14, 40, 120);

  camera = new THREE.PerspectiveCamera(
    50,
    canvas.clientWidth / canvas.clientHeight,
    0.1,
    500,
  );
  camera.position.set(size / 2, 35, size / 2 + 28);

  controls = new OrbitControls(camera, canvas);
  controls.target.set(size / 2, 0, size / 2);
  controls.maxPolarAngle = Math.PI * 0.45;
  controls.minDistance = 8;
  controls.maxDistance = 90;
  controls.enableDamping = true;

  const hemi = new THREE.HemisphereLight(0xc8d8a8, 0x1a2010, 1.1);
  scene.add(hemi);
  const sun = new THREE.DirectionalLight(0xfff0c8, 0.85);
  sun.position.set(30, 50, 10);
  scene.add(sun);

  const geo = new THREE.PlaneGeometry(size, size, size, size);
  const mat = new THREE.MeshStandardMaterial({
    color: 0x3d5230,
    wireframe: false,
    flatShading: true,
  });
  ground = new THREE.Mesh(geo, mat);
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(size / 2, 0, size / 2);
  ground.receiveShadow = true;
  scene.add(ground);

  const grid = new THREE.GridHelper(size, size, 0x5a6a40, 0x2a3a20);
  grid.position.set(size / 2, 0.02, size / 2);
  scene.add(grid);

  raycaster = new THREE.Raycaster();
  pointer = new THREE.Vector2();

  window.addEventListener("resize", onResize);
  canvas.addEventListener("pointerdown", onPointerDown);
  animate();
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

function colorFor(entity) {
  if (entity.kind === "hq") return 0xe8c547;
  if (entity.building) return entity.team === 0 ? 0x4a7a3a : 0x8a4a3a;
  if (entity.kind.includes("tank")) return 0x6a7a50;
  return 0xb0c080;
}

function upsertMesh(entity) {
  let mesh = state.meshes.get(entity.id);
  const h = entity.building ? (entity.kind === "hq" ? 2.4 : 1.4) : 0.7;
  const w = entity.building ? (entity.kind === "hq" ? 2.2 : 1.2) : 0.55;

  if (!mesh) {
    const geo = new THREE.BoxGeometry(w, h, w);
    const mat = new THREE.MeshStandardMaterial({ color: colorFor(entity) });
    mesh = new THREE.Mesh(geo, mat);
    mesh.userData.id = entity.id;
    scene.add(mesh);
    state.meshes.set(entity.id, mesh);

    if (entity.flag) {
      const flag = new THREE.Mesh(
        new THREE.BoxGeometry(0.15, 1.2, 0.4),
        new THREE.MeshStandardMaterial({ color: 0xf0d060 }),
      );
      flag.position.set(0.7, h * 0.6, 0);
      mesh.add(flag);
    }
  }

  mesh.position.set(entity.x, h / 2, entity.y);
  mesh.material.color.setHex(colorFor(entity));
  mesh.material.opacity = entity.progress != null && entity.progress < 1 ? 0.65 : 1;
  mesh.material.transparent = mesh.material.opacity < 1;
}

function rebuildMeshes() {
  for (const entity of state.entities.values()) {
    upsertMesh(entity);
  }
}

function animate() {
  requestAnimationFrame(animate);
  if (!renderer) return;
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
  send({
    t: "create_lobby",
    max_players: Number($("#max-players").value) || 16,
    map_size: Number($("#map-size").value) || 96,
    ffa: $("#ffa").checked,
  });
  send({ t: "set_faction", faction: state.faction });
});

$("#btn-join").addEventListener("click", () => {
  const lobby_id = $("#lobby-id").value.trim();
  if (!lobby_id) return toast("Enter lobby id");
  send({ t: "join_lobby", lobby_id });
});

$("#btn-ready").addEventListener("click", () => {
  state.ready = !state.ready;
  send({ t: "ready", ready: state.ready });
  $("#btn-ready").textContent = state.ready ? "Unready" : "Ready";
});

$("#btn-start").addEventListener("click", () => send({ t: "start_match" }));
$("#btn-leave").addEventListener("click", () => send({ t: "leave_lobby" }));

$("#build-list").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-kind]");
  if (!btn) return;
  state.selectedBuild = btn.dataset.kind;
  document.querySelectorAll(".build-item").forEach((el) => {
    el.classList.toggle("on", el === btn);
  });
  $("#build-detail").textContent = `Placing ${btn.dataset.kind} — click a tile near your base`;
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

$("#btn-refresh-lobbies").addEventListener("click", async () => {
  try {
    const res = await fetch("/api/lobbies", { credentials: "same-origin" });
    const data = await res.json();
    $("#open-lobbies").innerHTML = (data.lobbies || [])
      .map(
        (lobby) => `
        <button type="button" class="store-item" data-join="${lobby.id}">
          <strong>${lobby.slots.length}/${lobby.max_players}</strong>
          <small>${lobby.id}</small>
          <span>Map ${lobby.map_size}${lobby.ffa ? " · FFA" : ""}</span>
        </button>`,
      )
      .join("") || "<p class='muted'>No open lobbies</p>";
  } catch {
    toast("Could not list lobbies");
  }
});

$("#open-lobbies").addEventListener("click", (event) => {
  const btn = event.target.closest("[data-join]");
  if (!btn) return;
  send({ t: "join_lobby", lobby_id: btn.dataset.join });
});

connect();
