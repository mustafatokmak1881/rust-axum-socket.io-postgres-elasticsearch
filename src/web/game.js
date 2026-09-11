"use strict";

const $ = (selector) => document.querySelector(selector);

const definitions = {
  headquarters: {
    name: "Bey otağı",
    description: "Obanın yönetim merkezi",
    icon: `/assets/building-headquarters.svg`,
    map: { left: "58%", top: "32%" },
  },
  barracks: {
    name: "Kışla",
    description: "Piyade birliklerinin eğitildiği yer",
    icon: `/assets/building-generic.svg`,
    map: { left: "78%", top: "42%" },
  },
  stable: {
    name: "Ahır",
    description: "Atlı birliklerin yetiştirildiği yer",
    icon: `/assets/building-generic.svg`,
    map: { left: "86%", top: "58%" },
  },
  workshop: {
    name: "Atölye",
    description: "Kuşatma silahlarının üretildiği yer",
    icon: `/assets/building-generic.svg`,
    map: { left: "72%", top: "62%" },
  },
  academy: {
    name: "Akademi",
    description: "Misyoner eğitimi ve fetih merkezi",
    icon: `/assets/building-generic.svg`,
    map: { left: "48%", top: "22%" },
  },
  smithy: {
    name: "Demirci",
    description: "Silah araştırma ve geliştirme",
    icon: `/assets/building-generic.svg`,
    map: { left: "38%", top: "40%" },
  },
  rally_point: {
    name: "İçtima meydanı",
    description: "Orduların toplandığı komuta noktası",
    icon: `/assets/building-generic.svg`,
    map: { left: "50%", top: "55%" },
  },
  statue: {
    name: "Heykel",
    description: "Şövalye anıtı",
    icon: `/assets/building-generic.svg`,
    map: { left: "62%", top: "48%" },
  },
  market: {
    name: "Pazar",
    description: "Ticaret ve hammadde gönderimi",
    icon: `/assets/building-generic.svg`,
    map: { left: "34%", top: "70%" },
  },
  timber: {
    name: "Oduncu",
    description: "Odun üretimi",
    icon: `/assets/building-timber.svg`,
    map: { left: "18%", top: "42%" },
  },
  clay: {
    name: "Kil ocağı",
    description: "Kil üretimi",
    icon: `/assets/building-generic.svg`,
    map: { left: "14%", top: "62%" },
  },
  iron: {
    name: "Demir madeni",
    description: "Demir üretimi",
    icon: `/assets/building-generic.svg`,
    map: { left: "22%", top: "78%" },
  },
  farm: {
    name: "Çiftlik",
    description: "Nüfus ve birlik beslemesi",
    icon: `/assets/building-generic.svg`,
    map: { left: "42%", top: "78%" },
  },
  warehouse: {
    name: "Ambar",
    description: "Obanın kaynak deposu",
    icon: `/assets/building-warehouse.svg`,
    map: { left: "27%", top: "52%" },
  },
  hiding_place: {
    name: "Gizli depo",
    description: "Yağmalanamayan gizlenmiş kaynaklar",
    icon: `/assets/building-generic.svg`,
    map: { left: "66%", top: "76%" },
  },
  wall: {
    name: "Duvar",
    description: "Köy savunmasını güçlendirir",
    icon: `/assets/building-generic.svg`,
    map: { left: "88%", top: "78%" },
  },
};

function buildingIcon(definition) {
  return `<img
    src="${definition.icon}"
    alt=""
    class="building-art"
  >`;
}

let snapshot = null;
let serverOffset = 0;
let refreshing = false;
let mutating = false;
let sessionExpired = false;
let militarySnapshot = null;
let militaryRefreshing = false;
let hasVillage = false;
const baseDocumentTitle = document.title;

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

  return ["overview", "buildings", "jobs", "account"].includes(tab)
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

  if (!snapshot.village) {
    updateIncomingBadge(0);
    return;
  }

  const { village, buildings, upgrades, user, rules } = snapshot;
  const active = upgrades.find((upgrade) => !upgrade.completed_at);

  $("#village-name").textContent = village.name;
  $("#wood").textContent = number(village.wood);

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
    const definition = definitions[offer.kind];
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
        ${number(offer.production_per_hour)} odun/saat
        ${offer.next_production_per_hour !== null
        ? ` → ${number(offer.next_production_per_hour)} odun/saat`
        : ""}
      </small>
    `
      : "";

    const actionLabel = offer.level === 0
      ? "İnşa et"
      : `Seviye ${offer.level + 1} yükselt`;

    return `
    <tr>
      <td>
        <div class="building-cell">
          <span class="building-icon">${buildingIcon(definition)}</span>

          <div>
            <strong>${escapeHtml(definition.name)}</strong>
            <small>${escapeHtml(definition.description)}</small>
            ${production}
            ${requirements
        ? `<small class="building-requirements">${requirements}</small>`
        : ""}
          </div>
        </div>
      </td>

      <td><b>${offer.level}</b> / ${offer.max_level}</td>

      <td>
        ${maxed ? "—" : number(offer.cost_wood)}
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

  const mapRoot = $("#village-map-buildings");

  if (mapRoot) {
    mapRoot.innerHTML = (snapshot.offers || [])
      .filter((offer) => offer.level > 0 && definitions[offer.kind]?.map)
      .map((offer) => {
        const definition = definitions[offer.kind];
        const { left, top } = definition.map;

        return `
          <a
            href="#buildings"
            class="map-building"
            style="left:${left};top:${top}"
            title="${escapeHtml(definition.name)}"
          >
            <img
              src="${definition.icon}"
              alt="${escapeHtml(definition.name)}"
              class="village-building-art"
            >
            <strong>${escapeHtml(definition.name)}</strong>
            <small>Seviye ${offer.level}</small>
          </a>
        `;
      })
      .join("");
  }

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
        Ordu: <strong>${attack.sent_spears}</strong> mızrakçı
        · Varış: ${escapeHtml(date(attack.arrives_at))}
        · Kalan:
        <span class="army-countdown">${escapeHtml(formatCountdown(attack.arrives_at))}</span>
      </p>
    </article>
  `).join("");
}

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

  const projectedWood = Math.floor(
    economy.wood
    + economy.wood_remainder / 3_600_000_000
    + elapsedMs * economy.wood_per_hour / 3_600_000,
  );

  $("#wood").textContent = number(projectedWood);

  const rate = $("#wood-rate");

  if (rate) {
    rate.textContent = `+${number(economy.wood_per_hour)}/saat`;
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

$("#create-village").addEventListener("click", (event) => {
  void mutate(
    event.currentTarget,
    "/api/villages",
    { method: "POST" },
    "Oban kuruldu. Yurduna hoş geldin!",
  );
});

$("#building-rows").addEventListener("click", (event) => {
  const button = event.target.closest("[data-upgrade]");
  if (!button) return;

  const kind = button.dataset.upgrade;

  void mutate(
    button,
    `/api/buildings/${encodeURIComponent(kind)}/upgrade`,
    { method: "POST" },
    "İnşaat başladı. Tamamlandığında bina seviyesi güncellenecek.",
  );
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
    "Köy adı güncellendi.",
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

  // İnşaat varsa 1 saniye, yoksa 5 saniye.
  nextPollAt = now + (hasActiveConstruction ? 1000 : 5000);

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