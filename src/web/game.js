"use strict";

const $ = (selector) => document.querySelector(selector);

const definitions = {
  headquarters: {
    name: "Komuta Merkezi",
    description: "Ana üssün komuta binası; yalnızca ilk üste kurulur",
    map: { left: "58%", top: "32%" },
  },
  barracks: {
    name: "Kışla",
    description: "Piyade birliklerinin eğitildiği yer",
    map: { left: "78%", top: "42%" },
    href: "#army",
  },
  stable: {
    name: "Havaalanı",
    description: "Hava birliklerinin üretildiği yer",
    map: { left: "86%", top: "58%" },
  },
  workshop: {
    name: "Savaş Fabrikası",
    description: "Tank ve zırhlı araç üretimi",
    map: { left: "72%", top: "62%" },
  },
  academy: {
    name: "Strateji Merkezi",
    description: "Doktrin güçleri ve ileri teknolojiler",
    map: { left: "48%", top: "22%" },
  },
  smithy: {
    name: "Cephanelik",
    description: "Silah ve birim geliştirme",
    map: { left: "38%", top: "40%" },
  },
  rally_point: {
    name: "Seferberlik Sahası",
    description: "Orduların toplandığı çıkış noktası",
    map: { left: "50%", top: "55%" },
    href: "#army",
  },
  statue: {
    name: "Radar İstasyonu",
    description: "Keşif ve erken uyarı",
    map: { left: "62%", top: "48%" },
  },
  market: {
    name: "Tedarik Merkezi",
    description: "Lojistik ve hammadde transferi",
    map: { left: "34%", top: "70%" },
  },
  timber: {
    name: "İkmal Deposu",
    description: "Supplies production",
    map: { left: "18%", top: "42%" },
  },
  clay: {
    name: "Petrol Rafinerisi",
    description: "Fuel production",
    map: { left: "14%", top: "62%" },
  },
  iron: {
    name: "Maden Tesisi",
    description: "Munitions production",
    map: { left: "22%", top: "78%" },
  },
  farm: {
    name: "Enerji Santrali",
    description: "Nüfus / güç kapasitesi",
    map: { left: "42%", top: "78%" },
  },
  warehouse: {
    name: "Depo",
    description: "Üssün kaynak deposu",
    map: { left: "27%", top: "52%" },
  },
  hiding_place: {
    name: "Yeraltı Deposu",
    description: "Yağmalanamayan gizli stoklar",
    map: { left: "66%", top: "76%" },
  },
  wall: {
    name: "Savunma Bataryası",
    description: "Üs savunmasını güçlendirir",
    map: { left: "88%", top: "78%" },
  },
};

function currentFaction() {
  return String(
    snapshot?.village?.faction
      || snapshot?.user?.faction
      || selectedFaction
      || "usa",
  ).toLowerCase();
}

function buildingIconPath(kind) {
  return `/assets/buildings/${currentFaction()}/${kind}`;
}

function syncDefinitionsFromOffers() {
  const faction = currentFaction();

  for (const kind of Object.keys(definitions)) {
    definitions[kind].icon = `/assets/buildings/${faction}/${kind}`;
  }

  for (const offer of snapshot?.offers || []) {
    const current = definitions[offer.kind] || {
      map: { left: "50%", top: "50%" },
    };

    definitions[offer.kind] = {
      ...current,
      name: offer.name || current.name || offer.kind,
      description: offer.description || current.description || "",
      icon: `/assets/buildings/${faction}/${offer.kind}`,
    };
  }
}

function applyFactionSelection(factionId) {
  selectedFaction = factionId;

  document.querySelectorAll(".faction-card").forEach((card) => {
    const active = card.dataset.faction === factionId;
    card.setAttribute("aria-pressed", active ? "true" : "false");
  });

  const blurb = (snapshot?.factions || []).find((item) => item.id === factionId)?.blurb
    || "";
  const blurbEl = $("#faction-blurb");
  if (blurbEl) blurbEl.textContent = blurb;

  const createButton = $("#create-village");
  if (createButton) createButton.disabled = !factionId;
}

function buildingIcon(definition) {
  return `<img
    src="${definition.icon}"
    alt=""
    class="building-art"
    width="52"
    height="44"
    loading="lazy"
  >`;
}

let snapshot = null;
let serverOffset = 0;
let refreshing = false;
let mutating = false;
let sessionExpired = false;
let militarySnapshot = null;
let militaryRefreshing = false;
let selectedFaction = null;
let hasVillage = false;
const baseDocumentTitle = document.title;

const BASE_TILE_PX = 48;
const baseMap = {
  cameraX: 63.5,
  cameraY: 63.5,
  scale: 1,
  drag: null,
  moved: false,
  placementKind: null,
  hoverTile: null,
  centeredOnce: false,
};

function baseGridSize() {
  return Number(snapshot?.rules?.base_grid_size) || 128;
}

function buildMaxChebyshev() {
  return Number(snapshot?.rules?.build_max_chebyshev) || 3;
}

function chebyshev(ax, ay, bx, by) {
  return Math.max(Math.abs(ax - bx), Math.abs(ay - by));
}

function occupiedTiles(excludeKind = null) {
  const tiles = [];

  for (const offer of snapshot?.offers || []) {
    if (
      offer.tile_x == null
      || offer.tile_y == null
      || (excludeKind && offer.kind === excludeKind)
    ) {
      continue;
    }

    tiles.push({
      kind: offer.kind,
      x: offer.tile_x,
      y: offer.tile_y,
    });
  }

  return tiles;
}

function isValidPlacementTile(x, y, kind) {
  const size = baseGridSize();
  if (x < 0 || y < 0 || x >= size || y >= size) return false;

  const occupied = occupiedTiles(kind);
  if (occupied.some((tile) => tile.x === x && tile.y === y)) return false;

  if (occupied.length === 0) return true;

  const maxDist = buildMaxChebyshev();
  return occupied.some((tile) => chebyshev(tile.x, tile.y, x, y) <= maxDist);
}

function validPlacementTiles(kind) {
  const occupied = occupiedTiles(kind);
  const size = baseGridSize();
  const maxDist = buildMaxChebyshev();
  const found = new Map();

  if (occupied.length === 0) {
    const cx = Math.floor(size / 2);
    const cy = Math.floor(size / 2);
    found.set(`${cx},${cy}`, { x: cx, y: cy });
    return [...found.values()];
  }

  for (const tile of occupied) {
    for (let dy = -maxDist; dy <= maxDist; dy++) {
      for (let dx = -maxDist; dx <= maxDist; dx++) {
        if (Math.max(Math.abs(dx), Math.abs(dy)) > maxDist) continue;

        const x = tile.x + dx;
        const y = tile.y + dy;

        if (x < 0 || y < 0 || x >= size || y >= size) continue;
        if (occupied.some((item) => item.x === x && item.y === y)) continue;

        found.set(`${x},${y}`, { x, y });
      }
    }
  }

  return [...found.values()];
}

function applyBaseCamera() {
  const viewport = $("#base-map-viewport");
  const world = $("#base-map-world");
  if (!viewport || !world) return;

  const size = baseGridSize();
  const tile = BASE_TILE_PX;
  const worldPx = size * tile;

  world.style.width = `${worldPx}px`;
  world.style.height = `${worldPx}px`;

  const grid = $("#base-map-grid");
  if (grid) {
    grid.style.backgroundSize = `${tile}px ${tile}px`;
  }

  const vw = viewport.clientWidth;
  const vh = viewport.clientHeight;
  if (vw < 8 || vh < 8) return;

  const halfW = (vw / baseMap.scale) / (2 * tile);
  const halfH = (vh / baseMap.scale) / (2 * tile);

  baseMap.cameraX = Math.max(halfW, Math.min(size - halfW, baseMap.cameraX));
  baseMap.cameraY = Math.max(halfH, Math.min(size - halfH, baseMap.cameraY));

  const originX = vw / 2 - baseMap.cameraX * tile * baseMap.scale;
  const originY = vh / 2 - baseMap.cameraY * tile * baseMap.scale;

  world.style.transform =
    `translate(${originX}px, ${originY}px) scale(${baseMap.scale})`;

  drawBaseRadar();
  updateBaseCaption();
}

function drawBaseRadar() {
  const canvas = $("#base-map-radar");
  const viewport = $("#base-map-viewport");
  if (!canvas || !viewport) return;

  const ctx = canvas.getContext("2d");
  const size = baseGridSize();
  const w = canvas.width;
  const h = canvas.height;

  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "#0c140acc";
  ctx.fillRect(0, 0, w, h);

  ctx.strokeStyle = "#3a4a3488";
  ctx.lineWidth = 1;
  ctx.strokeRect(0.5, 0.5, w - 1, h - 1);

  const occupied = occupiedTiles();
  const cell = Math.max(1.2, w / size);

  for (const tile of occupied) {
    ctx.fillStyle = "#c8a050";
    ctx.fillRect((tile.x / size) * w, (tile.y / size) * h, cell, cell);
  }

  const tilePx = BASE_TILE_PX;
  const viewW = (viewport.clientWidth / baseMap.scale) / tilePx;
  const viewH = (viewport.clientHeight / baseMap.scale) / tilePx;
  const left = baseMap.cameraX - viewW / 2;
  const top = baseMap.cameraY - viewH / 2;

  ctx.strokeStyle = "#e8c547";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(
    (left / size) * w,
    (top / size) * h,
    (viewW / size) * w,
    (viewH / size) * h,
  );
}

function updateBaseCaption() {
  const caption = $("#base-map-caption");
  if (!caption) return;

  const size = baseGridSize();

  if (baseMap.placementKind) {
    const name = definitions[baseMap.placementKind]?.name || baseMap.placementKind;
    caption.textContent =
      `Placing ${name} · click a green tile (≤${buildMaxChebyshev()} from base) · ${size}×${size} field`;
    return;
  }

  const tx = Math.floor(baseMap.cameraX);
  const ty = Math.floor(baseMap.cameraY);
  caption.textContent =
    `Drag to pan · Scroll to zoom · Focus (${tx}, ${ty}) · ${size}×${size} ops grid`;
}

function updateBuildRailDetail(offer) {
  const detail = $("#base-build-detail");
  const status = $("#base-build-status");
  if (!detail || !status) return;

  if (!offer) {
    status.textContent = baseMap.placementKind
      ? "Place on map"
      : "Select a structure";
    detail.innerHTML = `
      <p class="muted small">
        Choose a structure from the left, then click a green tile on the map.
      </p>
    `;
    return;
  }

  const name = offer.name || definitions[offer.kind]?.name || offer.kind;
  status.textContent = offer.level === 0 ? `Build ${name}` : `Upgrade ${name}`;

  if (offer.blocked_reason && !offer.can_upgrade) {
    detail.innerHTML = `
      <strong>${escapeHtml(name)}</strong>
      <p>${escapeHtml(offer.blocked_reason)}</p>
    `;
    return;
  }

  const cost = offer.level >= offer.max_level
    ? "<p>Maximum level reached.</p>"
    : `
      <div class="base-build-costs">
        <span>Sup ${number(offer.cost_wood)}</span>
        <span>Fuel ${number(offer.cost_clay)}</span>
        <span>Mun ${number(offer.cost_iron)}</span>
        <span>${duration(offer.duration_seconds)}</span>
      </div>
      <p>${escapeHtml(offer.description || "")}</p>
    `;

  detail.innerHTML = `<strong>${escapeHtml(name)}</strong>${cost}`;
}

function renderBuildRail() {
  const list = $("#base-build-list");
  if (!list) return;

  const offers = snapshot?.offers || [];

  list.innerHTML = offers.map((offer) => {
    const definition = definitions[offer.kind] || {
      name: offer.name,
      icon: buildingIconPath(offer.kind),
    };
    const selected = baseMap.placementKind === offer.kind;
    const built = offer.level > 0 || offer.tile_x != null;
    const locked = !offer.can_upgrade;
    const disabled = mutating;

    const meta = offer.level === 0
      ? (offer.can_upgrade ? "Ready to build" : (offer.blocked_reason || "Locked"))
      : offer.can_upgrade
        ? `Upgrade → ${offer.level + 1}`
        : `Lv ${offer.level}/${offer.max_level}`;

    return `
      <button
        type="button"
        class="base-build-item${selected ? " is-selected" : ""}${built ? " is-built" : ""}${locked ? " is-locked" : ""}"
        data-build-kind="${escapeHtml(offer.kind)}"
        ${disabled ? "disabled" : ""}
        title="${escapeHtml(offer.description || definition.name || offer.kind)}"
      >
        <img
          src="${definition.icon || buildingIconPath(offer.kind)}"
          alt=""
          width="40"
          height="34"
          loading="lazy"
        >
        <span class="base-build-item-copy">
          <strong>${escapeHtml(offer.name || definition.name || offer.kind)}</strong>
          <small>${escapeHtml(meta)}</small>
        </span>
      </button>
    `;
  }).join("");

  const selectedOffer = offers.find((offer) => offer.kind === baseMap.placementKind)
    || null;
  updateBuildRailDetail(selectedOffer);

  const cancel = $("#placement-cancel");
  if (cancel) cancel.hidden = !baseMap.placementKind;
}

function screenToBaseTile(clientX, clientY) {
  const viewport = $("#base-map-viewport");
  if (!viewport) return null;

  const rect = viewport.getBoundingClientRect();
  const localX = (clientX - rect.left - rect.width / 2) / baseMap.scale;
  const localY = (clientY - rect.top - rect.height / 2) / baseMap.scale;
  const tile = BASE_TILE_PX;

  const x = Math.floor(baseMap.cameraX + localX / tile);
  const y = Math.floor(baseMap.cameraY + localY / tile);
  const size = baseGridSize();

  if (x < 0 || y < 0 || x >= size || y >= size) return null;
  return { x, y };
}

function renderPlacementHighlights() {
  const root = $("#base-map-highlights");
  if (!root) return;

  if (!baseMap.placementKind) {
    root.innerHTML = "";
    return;
  }

  const tile = BASE_TILE_PX;
  const kind = baseMap.placementKind;
  const parts = [];

  for (const spot of validPlacementTiles(kind)) {
    const hover = baseMap.hoverTile?.x === spot.x && baseMap.hoverTile?.y === spot.y;

    parts.push(`
      <div
        class="base-tile-highlight valid${hover ? " hover" : ""}"
        style="left:${spot.x * tile}px;top:${spot.y * tile}px;width:${tile}px;height:${tile}px"
      ></div>
    `);
  }

  root.innerHTML = parts.join("");
}

function centerBaseOnBuildings() {
  const tiles = occupiedTiles();
  if (!tiles.length) {
    const mid = baseGridSize() / 2;
    baseMap.cameraX = mid;
    baseMap.cameraY = mid;
    return;
  }

  const sx = tiles.reduce((sum, tile) => sum + tile.x + 0.5, 0) / tiles.length;
  const sy = tiles.reduce((sum, tile) => sum + tile.y + 0.5, 0) / tiles.length;
  baseMap.cameraX = sx;
  baseMap.cameraY = sy;
}

function enterPlacementMode(kind) {
  const offer = (snapshot?.offers || []).find((item) => item.kind === kind);
  if (!offer) return;

  if (offer.level > 0) {
    exitPlacementMode();
    void startUpgrade(kind, {});
    return;
  }

  if (!offer.can_upgrade) {
    showMessage(offer.blocked_reason || "Bu bina şu an kurulamaz.", true);
    updateBuildRailDetail(offer);
    return;
  }

  baseMap.placementKind = kind;
  baseMap.hoverTile = null;

  const viewport = $("#base-map-viewport");
  viewport?.classList.add("placing");

  const cancel = $("#placement-cancel");
  if (cancel) cancel.hidden = false;

  location.hash = "#overview";
  applyTab();
  renderBuildRail();
  renderPlacementHighlights();
  applyBaseCamera();
  viewport?.focus();
  showMessage("Sol menüden seçildi — haritada yeşil kareye tıkla.");
}

function exitPlacementMode() {
  baseMap.placementKind = null;
  baseMap.hoverTile = null;

  $("#base-map-viewport")?.classList.remove("placing");

  const cancel = $("#placement-cancel");
  if (cancel) cancel.hidden = true;

  renderBuildRail();
  renderPlacementHighlights();
  updateBaseCaption();
}

async function startUpgrade(kind, body) {
  if (mutating) return;

  mutating = true;
  document.querySelectorAll("[data-upgrade], [data-build-kind]").forEach((element) => {
    element.disabled = true;
  });

  try {
    await api(`/api/buildings/${encodeURIComponent(kind)}/upgrade`, {
      method: "POST",
      body: JSON.stringify(body || {}),
    });
    showMessage("İnşaat başladı. Tamamlandığında bina seviyesi güncellenecek.");
  } catch (error) {
    showMessage(error.message, true);
  } finally {
    mutating = false;
    await refresh();
  }
}

async function confirmPlacement(x, y) {
  const kind = baseMap.placementKind;
  if (!kind || mutating) return;

  if (!isValidPlacementTile(x, y, kind)) {
    showMessage("Bu kareye kurulamaz. Yeşil karelerden birini seç.", true);
    return;
  }

  exitPlacementMode();
  await startUpgrade(kind, { tile_x: x, tile_y: y });
}

function renderBaseBuildings() {
  const mapRoot = $("#village-map-buildings");
  if (!mapRoot) return;

  const tile = BASE_TILE_PX;

  mapRoot.innerHTML = (snapshot.offers || [])
    .filter((offer) => offer.tile_x != null && offer.tile_y != null)
    .map((offer) => {
      const definition = definitions[offer.kind] || {
        name: offer.kind,
        icon: buildingIconPath(offer.kind),
      };
      const left = (offer.tile_x + 0.5) * tile;
      const top = (offer.tile_y + 0.5) * tile;
      const buildingLabel = offer.level > 0
        ? `Lv ${offer.level}`
        : "Building…";

      return `
        <a
          href="${definition.href || "#buildings"}"
          class="map-building"
          style="left:${left}px;top:${top}px"
          title="${escapeHtml(definition.name)} (${offer.tile_x}, ${offer.tile_y})"
          data-tile-x="${offer.tile_x}"
          data-tile-y="${offer.tile_y}"
        >
          <img
            src="${definition.icon}"
            alt="${escapeHtml(definition.name)}"
            class="village-building-art"
            width="56"
            height="46"
            loading="lazy"
          >
          <strong>${escapeHtml(definition.name)}</strong>
          <small>${escapeHtml(buildingLabel)}</small>
        </a>
      `;
    })
    .join("");

  if (!baseMap.centeredOnce && (snapshot.offers || []).some((o) => o.tile_x != null)) {
    centerBaseOnBuildings();
    baseMap.centeredOnce = true;
  }

  renderBuildRail();
  applyBaseCamera();
  renderPlacementHighlights();
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;",
  })[character]);
}

function number(value) {
  return new Intl.NumberFormat("tr-TR").format(value);
}

function date(value) {
  return new Date(value).toLocaleString("tr-TR");
}

function duration(seconds) {
  seconds = Math.max(0, Math.ceil(seconds));

  const hours = String(Math.floor(seconds / 3600)).padStart(2, "0");
  const minutes = String(Math.floor((seconds % 3600) / 60)).padStart(2, "0");
  const rest = String(seconds % 60).padStart(2, "0");

  return `${hours}:${minutes}:${rest}`;
}

function showMessage(text, error = false) {
  const element = $("#message");
  element.textContent = text;
  element.className = error ? "notice error" : "notice success";
  element.hidden = false;
}

async function api(path, options = {}) {
  let response;

  try {
    response = await fetch(path, {
      credentials: "same-origin",
      cache: "no-store",
      ...options,
      headers: {
        ...(options.body ? { "Content-Type": "application/json" } : {}),
        ...(options.headers || {}),
      },
    });
  } catch {
    throw new Error("Sunucuya ulaşılamadı. Bağlantını kontrol et.");
  }

  if (response.status === 401) {
    sessionExpired = true;
    throw new Error("Oturumun sona erdi. Ana sayfadan tekrar giriş yap.");
  }

  if (!response.ok) {
    const body = await response.json().catch(() => null);

    throw new Error(
      body?.error || `İşlem başarısız oldu (${response.status}).`,
    );
  }

  if (response.status === 204) return null;

  return response.json();
}

function currentTab() {
  const tab = location.hash.slice(1);

  return ["overview", "army", "buildings", "jobs", "account"].includes(tab)
    ? tab
    : "overview";
}

function applyTab() {
  const tab = currentTab();

  document.querySelectorAll("[data-screen]").forEach((element) => {
    element.hidden = element.dataset.screen !== tab;
  });

  document.querySelectorAll("[data-tab]").forEach((element) => {
    const active = element.dataset.tab === tab;
    element.classList.toggle("selected", active);

    if (active) {
      element.setAttribute("aria-current", "page");
    } else {
      element.removeAttribute("aria-current");
    }
  });
}

function activeUpgradeMarkup(upgrade) {
  const definition = definitions[upgrade.building_kind];
  const name = definition?.name || upgrade.building_kind;
  const icon = definition ? buildingIcon(definition) : "";

  return `
    <div class="table-scroll live-construction">
      <table>
        <thead>
          <tr>
            <th>İnşaat</th>
            <th>Kalan süre</th>
            <th>Planlanan tamamlanma</th>
          </tr>
        </thead>

        <tbody>
          <tr>
            <td>
              <div class="building-cell">
                <span class="building-icon">${icon}</span>

                <div>
                  <strong>${escapeHtml(name)}</strong>
                  <small>Seviye ${escapeHtml(upgrade.target_level)}</small>
                </div>
              </div>
            </td>

            <td>
              <strong
                class="construction-countdown"
                data-countdown="${escapeHtml(upgrade.run_at)}"
              ></strong>
            </td>

            <td>
              <time datetime="${escapeHtml(upgrade.run_at)}">
                ${escapeHtml(date(upgrade.run_at))}
              </time>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  `;
}

function renderActiveConstruction(active) {
  const overviewQueue = $("#active-upgrade");
  const buildingsWrapper = $("#buildings-active-queue");
  const buildingsQueue = $("#buildings-active-upgrade");

  if (active) {
    const markup = activeUpgradeMarkup(active);

    overviewQueue.innerHTML = markup;
    buildingsQueue.innerHTML = markup;
    buildingsWrapper.hidden = false;
    return;
  }

  // Tamamlanan inşaat, binalar ekranından tamamen kaldırılır.
  buildingsQueue.innerHTML = "";
  buildingsWrapper.hidden = true;

  overviewQueue.innerHTML = `
    <p class="muted">Devam eden inşaat yok.</p>
    <a class="button" href="#buildings">Binaları geliştir</a>
  `;
}

function render() {
  if (!snapshot) return;

  $("#loading").hidden = true;
  $("#onboarding").hidden = Boolean(snapshot.village);
  $("#game-content").hidden = !snapshot.village;

  const villageJustAppeared = !hasVillage && Boolean(snapshot.village);
  hasVillage = Boolean(snapshot.village);

  if (villageJustAppeared) {
    baseMap.centeredOnce = false;
  }

  if (!snapshot.village) {
    updateIncomingBadge(0);
    if (selectedFaction) {
      applyFactionSelection(selectedFaction);
    } else {
      const blurbEl = $("#faction-blurb");
      if (blurbEl) {
        blurbEl.textContent = "Devam etmek için bir fraksiyon seç.";
      }
      const createButton = $("#create-village");
      if (createButton) createButton.disabled = true;
    }
    return;
  }

  syncDefinitionsFromOffers();

  const { village, buildings, upgrades, user, rules } = snapshot;
  const active = upgrades.find((upgrade) => !upgrade.completed_at);

  const factionLabel = (village.faction || user.faction || "").toUpperCase();
  $("#village-name").textContent = factionLabel
    ? `${village.name} · ${factionLabel}`
    : village.name;
  $("#wood").textContent = number(village.wood);
  $("#clay").textContent = number(village.clay);
  $("#iron").textContent = number(village.iron);

  $("#points").textContent = number(
    buildings.reduce((sum, building) => sum + building.level, 0),
  );

  $("#user-email").textContent = user.email;
  $("#user-created").textContent = date(user.created_at);

  // Kullanıcı yazarken polling input içeriğini ezmesin.
  const nameInput = $("#new-name");

  if (nameInput.dataset.villageId !== village.id) {
    nameInput.value = village.name;
    nameInput.dataset.villageId = village.id;
  }

  $("#building-rows").innerHTML = (snapshot.offers || []).map((offer) => {
    const definition = definitions[offer.kind] || {
      name: offer.name,
      description: offer.description,
      icon: buildingIconPath(offer.kind),
    };
    if (!definition) return "";

    const maxed = offer.level >= offer.max_level;

    const reason = mutating
      ? "İşleniyor…"
      : offer.blocked_reason || "";

    const requirements = offer.requirements.map((requirement) => `
    <span class="${requirement.met ? "requirement-met" : "requirement-missing"}">
      ${requirement.met ? "✓" : "✗"}
      ${escapeHtml(requirement.name)}
      ${requirement.required_level}
    </span>
  `).join(" · ");

    const production = offer.production_per_hour !== null
      ? `
      <small class="production-description">
        ${number(offer.production_per_hour)}/${
        offer.kind === "clay"
          ? "fuel"
          : offer.kind === "iron"
            ? "munitions"
            : "supplies"
      }/saat
        ${offer.next_production_per_hour !== null
        ? ` → ${number(offer.next_production_per_hour)}`
        : ""}
      </small>
    `
      : "";

    const costLabel = maxed
      ? "—"
      : `${number(offer.cost_wood)} / ${number(offer.cost_clay)} / ${number(offer.cost_iron)}`;

    const actionLabel = offer.level === 0
      ? "İnşa et"
      : `Seviye ${offer.level + 1} yükselt`;

    return `
    <tr>
      <td>
        <div class="building-cell">
          <span class="building-icon">${buildingIcon(definition)}</span>

          <div>
            <strong>${escapeHtml(offer.name || definition.name)}</strong>
            <small>${escapeHtml(offer.description || definition.description || "")}</small>
            ${production}
            ${requirements
        ? `<small class="building-requirements">${requirements}</small>`
        : ""}
          </div>
        </div>
      </td>

      <td><b>${offer.level}</b> / ${offer.max_level}</td>

      <td>
        ${costLabel}
      </td>

      <td>
        ${maxed ? "—" : duration(offer.duration_seconds)}
      </td>

      <td>
        <button
          class="button small-button"
          type="button"
          data-upgrade="${escapeHtml(offer.kind)}"
          ${mutating || !offer.can_upgrade ? "disabled" : ""}
        >
          ${escapeHtml(reason || actionLabel)}
        </button>
      </td>
    </tr>
  `;
  }).join("");

  renderBaseBuildings();
  renderActiveConstruction(active);

  const pendingUpgrades = upgrades.filter(
    (upgrade) => !upgrade.completed_at,
  );

  $("#job-rows").innerHTML = pendingUpgrades.length
    ? pendingUpgrades.map((upgrade) => `
    <tr>
      <td>${escapeHtml(
      definitions[upgrade.building_kind]?.name || upgrade.building_kind,
    )}</td>

      <td>${escapeHtml(upgrade.target_level)}</td>

      <td>${escapeHtml(date(upgrade.run_at))}</td>

      <td>
        <span
          class="construction-countdown"
          data-countdown="${escapeHtml(upgrade.run_at)}"
        ></span>
      </td>
    </tr>
  `).join("")
    : `
    <tr>
      <td colspan="4" class="empty">
        Devam eden inşaat bulunmuyor.
      </td>
    </tr>
  `;

  updateCountdowns();
  updateLiveResources();
  applyTab();

  if (villageJustAppeared) {
    void refreshMilitary();
  }
}

function formatCountdown(targetIso) {
  const remainingMs = new Date(targetIso).getTime() - Date.now();

  if (remainingMs <= 0) {
    return "Varıyor…";
  }

  const total = Math.ceil(remainingMs / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (n) => String(n).padStart(2, "0");

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }

  return `${minutes}:${pad(seconds)}`;
}

function updateIncomingBadge(count) {
  const badge = $("#incoming-badge");
  const countEl = $("#incoming-badge-count");
  const sectionCount = $("#incoming-section-count");

  if (!badge || !countEl) return;

  countEl.textContent = String(count);

  if (sectionCount) {
    sectionCount.textContent = count > 0 ? `(${count})` : "";
  }

  badge.hidden = count === 0;
  badge.title = count > 0 ? `${count} gelen saldırı` : "";

  document.title = count > 0
    ? `(⚔${count}) ${baseDocumentTitle}`
    : baseDocumentTitle;
}

function renderIncoming(incoming) {
  const list = $("#army-incoming-list");
  if (!list) return;

  updateIncomingBadge(incoming.length);

  if (!incoming.length) {
    list.innerHTML = `<p class="army-empty">Gelen saldırı yok.</p>`;
    return;
  }

  list.innerHTML = incoming.map((attack) => `
    <article class="army-order" data-arrives-at="${escapeHtml(attack.arrives_at)}">
      <div>
        <strong>
          ${escapeHtml(attack.source_name)}
          (${attack.source_x}|${attack.source_y})
        </strong>
        <span>Saldırı</span>
      </div>
      <p>
        Ordu: <strong>${attack.sent_spears}</strong> infantry
        · Varış: ${escapeHtml(date(attack.arrives_at))}
        · Kalan:
        <span class="army-countdown">${escapeHtml(formatCountdown(attack.arrives_at))}</span>
      </p>
    </article>
  `).join("");
}

function spearRecruitSeconds(count, barracksLevel) {
  const perUnit = Math.max(1, Math.ceil(20 * (0.95 ** (barracksLevel - 1))));
  return count * perUnit;
}

function maxRecruitable() {
  if (!militarySnapshot) return 0;

  const farmFree = militarySnapshot.farm?.free ?? 0;
  const wood = militarySnapshot.wood ?? snapshot?.village?.wood ?? 0;
  const clay = militarySnapshot.clay ?? snapshot?.village?.clay ?? 0;
  const iron = militarySnapshot.iron ?? snapshot?.village?.iron ?? 0;
  const woodCost = militarySnapshot.units?.spear?.wood_cost ?? 50;
  const clayCost = militarySnapshot.units?.spear?.clay_cost ?? 30;
  const ironCost = militarySnapshot.units?.spear?.iron_cost ?? 10;

  if (militarySnapshot.recruit) return 0;
  if ((militarySnapshot.barracks_level ?? 0) < 1) return 0;

  return Math.max(
    0,
    Math.min(
      farmFree,
      Math.floor(wood / woodCost),
      Math.floor(clay / clayCost),
      Math.floor(iron / ironCost),
      10000,
    ),
  );
}

function updateRecruitPreview() {
  const input = $("#recruit-spears");
  if (!input || !militarySnapshot) return;

  const count = Math.max(0, Number(input.value) || 0);
  const wood = (militarySnapshot.units?.spear?.wood_cost ?? 50) * count;
  const clay = (militarySnapshot.units?.spear?.clay_cost ?? 30) * count;
  const iron = (militarySnapshot.units?.spear?.iron_cost ?? 10) * count;
  const barracks = militarySnapshot.barracks_level ?? 0;
  const seconds = barracks >= 1 ? spearRecruitSeconds(count, barracks) : 0;

  $("#recruit-cost").textContent =
    `Maliyet: ${number(wood)} / ${number(clay)} / ${number(iron)}`;
  $("#recruit-duration").textContent = count > 0 && barracks >= 1
    ? `Süre: ${duration(seconds)}`
    : "Süre: —";
  $("#recruit-pop").textContent = `Nüfus: ${number(count)}`;
}

function renderArmyPanel() {
  if (!militarySnapshot) return;

  const army = militarySnapshot.army || {
    home: { spear: militarySnapshot.spears || 0 },
    away: { spear: 0 },
    training: { spear: 0 },
    total: { spear: militarySnapshot.spears || 0 },
  };

  const farm = militarySnapshot.farm || {
    used: 0,
    capacity: 0,
    free: 0,
    level: 0,
  };

  const homeEl = $("#army-home-total");
  const farmEl = $("#army-farm-usage");
  const barracksEl = $("#army-barracks-level");

  if (homeEl) {
    homeEl.textContent = `${number(army.home.spear || 0)} infantry`;
  }

  if (farmEl) {
    farmEl.textContent =
      `${number(farm.used)} / ${number(farm.capacity)} (boş ${number(farm.free)})`;
  }

  if (barracksEl) {
    barracksEl.textContent = `Seviye ${militarySnapshot.barracks_level ?? 0}`;
  }

  const rows = $("#army-unit-rows");

  if (rows) {
    rows.innerHTML = `
      <tr>
        <td><strong>Infantry</strong></td>
        <td>${number(army.home.spear || 0)}</td>
        <td>${number(army.away.spear || 0)}</td>
        <td>${number(army.training.spear || 0)}</td>
        <td><b>${number(army.total.spear || 0)}</b></td>
      </tr>
    `;
  }

  const queue = $("#army-recruit-queue");
  const panel = $("#army-recruit-panel");
  const hint = $("#army-recruit-hint");
  const form = $("#recruit-form");
  const result = $("#recruit-result");

  if (queue && militarySnapshot.recruit) {
    const recruit = militarySnapshot.recruit;
    queue.hidden = false;
    queue.innerHTML = `
      <div class="table-scroll live-construction">
        <table>
          <thead>
            <tr>
              <th>Eğitim</th>
              <th>Kalan süre</th>
              <th>Planlanan tamamlanma</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>
                <strong>${number(recruit.count)} infantry</strong>
              </td>
              <td>
                <strong
                  class="construction-countdown"
                  data-countdown="${escapeHtml(recruit.finishes_at)}"
                ></strong>
              </td>
              <td>
                <time datetime="${escapeHtml(recruit.finishes_at)}">
                  ${escapeHtml(date(recruit.finishes_at))}
                </time>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    `;
  } else if (queue) {
    queue.hidden = true;
    queue.innerHTML = "";
  }

  const barracksReady = (militarySnapshot.barracks_level ?? 0) >= 1;
  const training = Boolean(militarySnapshot.recruit);

  if (hint) {
    if (!barracksReady) {
      hint.textContent =
        "Train infantry: build Barracks (Command Center level 3).";
    } else if (training) {
      hint.textContent = "Eğitim tamamlanınca yeni emir verebilirsin.";
    } else {
      hint.textContent =
        `Infantry: ${militarySnapshot.units?.spear?.wood_cost ?? 50} / ${militarySnapshot.units?.spear?.clay_cost ?? 30} / ${militarySnapshot.units?.spear?.iron_cost ?? 10} (Supplies/Fuel/Munitions), 1 pop.`;
    }
  }

  if (form) {
    form.hidden = !barracksReady || training;
  }

  if (result && !recruiting) {
    result.textContent = "";
  }

  updateRecruitPreview();

  const orders = $("#army-orders-list");

  if (orders) {
    const attacks = militarySnapshot.attacks || [];
    const statusNames = {
      outbound: "Yolda",
      returning: "Dönüyor",
      completed: "Tamamlandı",
    };

    orders.innerHTML = attacks.length
      ? attacks.map((attack) => {
        const showCountdown = attack.status === "outbound"
          || attack.status === "returning";
        const countdownAt = attack.status === "returning"
          ? attack.returns_at
          : attack.arrives_at;
        const resolved = attack.resolved_at !== null;

        return `
          <article class="army-order"${showCountdown && countdownAt
            ? ` data-arrives-at="${escapeHtml(countdownAt)}"`
            : ""}>
            <div>
              <strong>
                ${escapeHtml(attack.target_name)}
                (${attack.target_x}|${attack.target_y})
              </strong>
              <span>${escapeHtml(statusNames[attack.status] || attack.status)}</span>
            </div>
            <p>
              Gönderilen: ${number(attack.sent_spears)} infantry
              · Varış: ${escapeHtml(date(attack.arrives_at))}
              ${showCountdown && countdownAt ? `
                · Kalan:
                <span class="army-countdown">${escapeHtml(formatCountdown(countdownAt))}</span>
              ` : ""}
            </p>
            ${resolved ? `
              <p class="army-report">
                Sağ kalan: ${attack.surviving_spears}
                · Savunmacı: ${attack.defender_before} → ${attack.defender_after}
                ${formatLoot(attack) ? ` · Ganimet: ${formatLoot(attack)}` : ""}
              </p>
            ` : ""}
          </article>
        `;
      }).join("")
      : `<p class="army-empty">Henüz saldırı göndermedin.</p>`;
  }
}

function formatLoot(attack) {
  const wood = attack.loot_wood || 0;
  const clay = attack.loot_clay || 0;
  const iron = attack.loot_iron || 0;

  if (wood <= 0 && clay <= 0 && iron <= 0) {
    return "";
  }

  return `${number(wood)} / ${number(clay)} / ${number(iron)}`;
}

let recruiting = false;

function tickMilitaryCountdowns() {
  document.querySelectorAll("[data-arrives-at] .army-countdown").forEach((el) => {
    const article = el.closest("[data-arrives-at]");
    if (!article) return;
    el.textContent = formatCountdown(article.dataset.arrivesAt);
  });
}

async function refreshMilitary() {
  if (!hasVillage || militaryRefreshing || sessionExpired) return;

  militaryRefreshing = true;

  try {
    militarySnapshot = await api("/api/military");
    const incoming = Array.isArray(militarySnapshot.incoming)
      ? militarySnapshot.incoming
      : [];
    renderIncoming(incoming);
    renderArmyPanel();
    updateCountdowns();
  } catch (error) {
    if (sessionExpired) return;
    const list = $("#army-incoming-list");
    if (list && !militarySnapshot) {
      list.innerHTML =
        `<p class="army-empty">${escapeHtml(error.message)}</p>`;
    }
  } finally {
    militaryRefreshing = false;
  }
}

function updateLiveResources() {
  if (!snapshot?.village || !snapshot.economy) return;

  const economy = snapshot.economy;
  const updatedAt = Date.parse(economy.resources_updated_at);

  let displayTime = Date.now() + serverOffset;

  // İnşaatın bitişinden sonra eski üretim hızıyla tahmin yapma.
  if (economy.production_valid_until) {
    displayTime = Math.min(
      displayTime,
      Date.parse(economy.production_valid_until),
    );
  }

  const elapsedMs = Math.max(0, displayTime - updatedAt);
  const hourMs = 3_600_000;
  const remScale = 3_600_000_000;

  const project = (balance, remainder, perHour) => Math.floor(
    balance
    + (remainder || 0) / remScale
    + elapsedMs * (perHour || 0) / hourMs,
  );

  $("#wood").textContent = number(project(
    economy.wood,
    economy.wood_remainder,
    economy.wood_per_hour,
  ));
  $("#clay").textContent = number(project(
    economy.clay,
    economy.clay_remainder,
    economy.clay_per_hour,
  ));
  $("#iron").textContent = number(project(
    economy.iron,
    economy.iron_remainder,
    economy.iron_per_hour,
  ));

  const woodRate = $("#wood-rate");
  const clayRate = $("#clay-rate");
  const ironRate = $("#iron-rate");

  if (woodRate) {
    woodRate.textContent = `+${number(economy.wood_per_hour)}/saat`;
  }
  if (clayRate) {
    clayRate.textContent = `+${number(economy.clay_per_hour)}/saat`;
  }
  if (ironRate) {
    ironRate.textContent = `+${number(economy.iron_per_hour)}/saat`;
  }
}

function updateCountdowns() {
  const now = Date.now() + serverOffset;

  document.querySelectorAll("[data-countdown]").forEach((element) => {
    const end = Date.parse(element.dataset.countdown);

    if (!Number.isFinite(end)) {
      element.textContent = "Süre alınamadı";
      element.classList.remove("awaiting-completion");
      return;
    }

    const remaining = end - now;
    const waiting = remaining <= 0;

    element.classList.toggle("awaiting-completion", waiting);

    const text = waiting
      ? "Tamamlanıyor…"
      : duration(remaining / 1000);

    // Aynı metni tekrar tekrar DOM'a yazma.
    if (element.textContent !== text) {
      element.textContent = text;
    }

    element.title = waiting
      ? "Sunucunun inşaat sonucunu kaydetmesi bekleniyor."
      : `Planlanan bitiş: ${date(element.dataset.countdown)}`;
  });
}

async function refresh() {
  if (refreshing || mutating || sessionExpired) return;

  refreshing = true;

  try {
    const started = Date.now();
    const data = await api("/api/game");

    serverOffset =
      new Date(data.server_time).getTime() - (started + Date.now()) / 2;

    snapshot = data;
    render();

    $("#sync-status").textContent =
      ` · Güncellendi ${new Date().toLocaleTimeString("tr-TR")}`;
  } catch (error) {
    $("#sync-status").textContent = " · Bağlantı güncellenemedi";

    if (!snapshot || sessionExpired) {
      showMessage(error.message, true);
    }
  } finally {
    refreshing = false;
  }
}

async function mutate(button, path, options, message) {
  if (mutating || sessionExpired) return;

  mutating = true;
  button.disabled = true;

  // Sunucu yanıtı beklenirken diğer yükseltme düğmelerini de kilitle.
  document.querySelectorAll("[data-upgrade]").forEach((element) => {
    element.disabled = true;
  });

  try {
    await api(path, options);
    showMessage(message);
  } catch (error) {
    showMessage(error.message, true);
  } finally {
    mutating = false;
    button.disabled = false;
    await refresh();
  }
}

document.querySelectorAll(".faction-card").forEach((card) => {
  card.addEventListener("click", () => {
    applyFactionSelection(card.dataset.faction);
  });
});

$("#create-village").addEventListener("click", (event) => {
  if (!selectedFaction) {
    showMessage("Önce USA, China veya GLA seç.", true);
    return;
  }

  void mutate(
    event.currentTarget,
    "/api/villages",
    {
      method: "POST",
      body: JSON.stringify({ faction: selectedFaction }),
    },
    `${selectedFaction.toUpperCase()} üssün kuruldu. Hoş geldin!`,
  );
});

$("#building-rows").addEventListener("click", (event) => {
  const button = event.target.closest("[data-upgrade]");
  if (!button) return;

  const kind = button.dataset.upgrade;
  const offer = (snapshot?.offers || []).find((item) => item.kind === kind);

  if (offer && offer.level === 0) {
    enterPlacementMode(kind);
    return;
  }

  void startUpgrade(kind, {});
});

$("#base-build-list")?.addEventListener("click", (event) => {
  const button = event.target.closest("[data-build-kind]");
  if (!button || button.disabled) return;

  const kind = button.dataset.buildKind;
  const offer = (snapshot?.offers || []).find((item) => item.kind === kind);
  if (!offer) return;

  updateBuildRailDetail(offer);

  if (!offer.can_upgrade) {
    showMessage(offer.blocked_reason || "Bu yapı şu an kullanılamaz.", true);
    return;
  }

  if (offer.level === 0) {
    enterPlacementMode(kind);
    return;
  }

  void startUpgrade(kind, {});
});

$("#placement-cancel")?.addEventListener("click", () => {
  exitPlacementMode();
});

(function setupBaseMapControls() {
  const viewport = $("#base-map-viewport");
  if (!viewport) return;

  viewport.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;

    baseMap.drag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      cameraX: baseMap.cameraX,
      cameraY: baseMap.cameraY,
    };
    baseMap.moved = false;
    viewport.setPointerCapture(event.pointerId);
    viewport.classList.add("dragging");
  });

  viewport.addEventListener("pointermove", (event) => {
    if (baseMap.placementKind) {
      baseMap.hoverTile = screenToBaseTile(event.clientX, event.clientY);
      renderPlacementHighlights();
    }

    if (!baseMap.drag || event.pointerId !== baseMap.drag.pointerId) return;

    const dx = event.clientX - baseMap.drag.startX;
    const dy = event.clientY - baseMap.drag.startY;

    if (Math.abs(dx) + Math.abs(dy) > 4) {
      baseMap.moved = true;
    }

    const tile = BASE_TILE_PX * baseMap.scale;
    baseMap.cameraX = baseMap.drag.cameraX - dx / tile;
    baseMap.cameraY = baseMap.drag.cameraY - dy / tile;
    applyBaseCamera();
  });

  const endDrag = (event) => {
    if (!baseMap.drag || event.pointerId !== baseMap.drag.pointerId) return;

    const wasDrag = baseMap.moved;
    const placing = baseMap.placementKind;
    const tile = screenToBaseTile(event.clientX, event.clientY);

    baseMap.drag = null;
    viewport.classList.remove("dragging");

    if (!wasDrag && placing && tile) {
      void confirmPlacement(tile.x, tile.y);
    }
  };

  viewport.addEventListener("pointerup", endDrag);
  viewport.addEventListener("pointercancel", endDrag);

  viewport.addEventListener("click", (event) => {
    if (baseMap.moved || baseMap.placementKind) {
      event.preventDefault();
      event.stopPropagation();
    }
  }, true);

  viewport.addEventListener("wheel", (event) => {
    event.preventDefault();
    const factor = event.deltaY < 0 ? 1.12 : 1 / 1.12;
    baseMap.scale = Math.max(0.45, Math.min(2.2, baseMap.scale * factor));
    applyBaseCamera();
  }, { passive: false });

  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && baseMap.placementKind) {
      exitPlacementMode();
    }
  });

  window.addEventListener("resize", () => {
    applyBaseCamera();
  });
})();

$("#recruit-spears")?.addEventListener("input", updateRecruitPreview);

$("#recruit-max")?.addEventListener("click", () => {
  const max = maxRecruitable();
  const input = $("#recruit-spears");
  if (!input) return;
  input.value = String(Math.max(1, max));
  updateRecruitPreview();
});

$("#recruit-form")?.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (recruiting || mutating || !militarySnapshot) return;

  const count = Number($("#recruit-spears")?.value);
  const result = $("#recruit-result");
  const submit = $("#recruit-submit");

  if (!Number.isInteger(count) || count < 1) {
    if (result) result.textContent = "Geçerli bir sayı gir.";
    return;
  }

  recruiting = true;
  if (submit) submit.disabled = true;
  if (result) result.textContent = "Eğitim başlatılıyor…";

  try {
    await api("/api/military/recruit", {
      method: "POST",
      body: JSON.stringify({ count }),
    });

    if (result) {
      result.textContent = `${count} infantry eğitimi başladı.`;
    }

    await refresh();
    await refreshMilitary();
  } catch (error) {
    if (result) result.textContent = error.message;
  } finally {
    recruiting = false;
    if (submit) submit.disabled = false;
  }
});

$("#rename-form").addEventListener("submit", (event) => {
  event.preventDefault();

  const button = event.currentTarget.querySelector("button");

  void mutate(
    button,
    "/api/village/name",
    {
      method: "PATCH",
      body: JSON.stringify({ name: $("#new-name").value.trim() }),
    },
    "Base name updated.",
  );
});

$("#logout").addEventListener("click", async (event) => {
  const button = event.currentTarget;
  button.disabled = true;

  try {
    await api("/auth/logout", { method: "POST" });
    location.replace("/");
  } catch (error) {
    showMessage(error.message, true);
    button.disabled = false;
  }
});

$("#retry").addEventListener("click", () => void refresh());

window.addEventListener("hashchange", applyTab);

window.addEventListener("pageshow", () => {
  if (snapshot) void refresh();
});

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) void refresh();
});

let nextPollAt = 0;

setInterval(() => {
  if (document.hidden || sessionExpired) return;
  if (refreshing || mutating) return;

  const now = Date.now();

  if (now < nextPollAt) return;

  const hasActiveConstruction = Boolean(
    snapshot?.upgrades?.some((upgrade) => !upgrade.completed_at),
  );
  const hasActiveRecruit = Boolean(militarySnapshot?.recruit);

  // İnşaat/eğitim varsa 1 saniye, yoksa 5 saniye.
  nextPollAt = now + (hasActiveConstruction || hasActiveRecruit ? 1000 : 5000);

  void refresh();
}, 250);

setInterval(() => {
  if (!document.hidden && !sessionExpired) {
    updateLiveResources();
  }
}, 250);

setInterval(() => {
  if (!document.hidden && !sessionExpired && hasVillage) {
    void refreshMilitary();
  }
}, 2000);

setInterval(() => {
  if (!document.hidden && !sessionExpired && hasVillage) {
    tickMilitaryCountdowns();
  }
}, 250);

applyTab();
void refresh();