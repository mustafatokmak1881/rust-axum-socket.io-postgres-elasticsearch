"use strict";

const $ = (selector) => document.querySelector(selector);

const definitions = {
  headquarters: {
    name: "Bey otağı",
    description: "Obanın yönetim merkezi",
    icon: `<img
      src="/assets/building-headquarters.svg"
      alt=""
      class="building-art"
    >`,
  },
  timber: {
    name: "Oduncu",
    description: "Odun işçiliğinin merkezi",
    icon: `<img
      src="/assets/building-timber.svg"
      alt=""
      class="building-art"
    >`,
  },
  warehouse: {
    name: "Ambar",
    description: "Obanın kaynak deposu",
    icon: `<img
      src="/assets/building-warehouse.svg"
      alt=""
      class="building-art"
    >`,
  },
};

let snapshot = null;
let serverOffset = 0;
let refreshing = false;
let mutating = false;
let sessionExpired = false;

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

  // icon yalnızca kod içinde tanımladığımız sabit görsellerden gelir.
  const icon = definition?.icon || "";

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

  if (!snapshot.village) return;

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

  $("#building-rows").innerHTML = buildings.map((building) => {
    const definition = definitions[building.kind];
    if (!definition) return "";

    const maxed = building.level >= rules.max_level;
    const target = building.level + 1;
    const cost = target * rules.wood_per_target_level;
    const seconds = target * rules.seconds_per_target_level;

    let reason = "";

    if (maxed) reason = "En yüksek seviye";
    else if (active) reason = "İnşaat sürüyor";
    else if (village.wood < cost) reason = "Odun yetersiz";
    else if (mutating) reason = "İşleniyor…";

    return `
      <tr>
        <td>
          <div class="building-cell">
            <span class="building-icon">${definition.icon}</span>
            <div>
              <strong>${definition.name}</strong>
              <small>${definition.description}</small>
            </div>
          </div>
        </td>
        <td><b>${building.level}</b></td>
        <td>${maxed ? "—" : number(cost)}</td>
        <td>${maxed ? "—" : duration(seconds)}</td>
        <td>
          <button
            class="button small-button"
            type="button"
            data-upgrade="${building.kind}"
            ${reason ? "disabled" : ""}
          >
            ${reason || `Seviye ${target} yükselt`}
          </button>
        </td>
      </tr>
    `;
  }).join("");

  buildings.forEach((building) => {
    const element = document.getElementById(`level-${building.kind}`);

    if (element) {
      element.textContent = `Seviye ${building.level}`;
    }
  });

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
  applyTab();
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

// Bu interval API çağrısı yapmaz; yalnızca yazıyı günceller.
setInterval(() => {
  if (!document.hidden) {
    updateCountdowns();
  }
}, 250);

applyTab();
void refresh();