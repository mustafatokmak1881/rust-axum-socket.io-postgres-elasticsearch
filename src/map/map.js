(() => {
    "use strict";

    const $ = (id) => document.getElementById(id);

    const canvas = $("map");
    const ctx = canvas.getContext("2d");
    const stage = $("map-stage");

    const overview = $("overview");
    const overviewCtx = overview.getContext("2d");

    const state = {
        world: null,
        home: null,
        villages: [],
        selected: null,

        camera: { x: 500.5, y: 500.5 },
        scale: 52,

        width: 1,
        height: 1,
        dpr: 1,

        drag: null,
        controller: null,
        requestId: 0,
        loadTimer: null,
        frame: null,
        ready: false,
    };

    const affiliationLabels = {
        own: "Senin köyün",
        player: "Oyuncu köyü",
        barbarian: "Barbar köyü",
    };

    function status(message, error = false) {
        $("status").textContent = message;
        $("status").classList.toggle("error", error);
    }

    async function api(url, options = {}) {
        const response = await fetch(url, {
            credentials: "same-origin",
            cache: "no-store",
            ...options,
        });

        if (response.status === 401) {
            location.assign("/auth/google");
            throw new Error("Oturumun sona ermiş. Girişe yönlendiriliyorsun.");
        }

        if (!response.ok) {
            throw new Error(`İstek başarısız: HTTP ${response.status}`);
        }

        return response.json();
    }

    const clamp = (value, min, max) =>
        Math.max(min, Math.min(max, value));

    function continent(x, y) {
        return `K${Math.floor(y / 100)}${Math.floor(x / 100)}`;
    }

    function minimumScale() {
        // API bir istekte en fazla 100 x 100 kare kabul eder.
        // Görünen alanı, kenar payları dahil bunun altında tutar.
        return Math.max(
            14,
            state.width / 88,
            state.height / 88,
        );
    }

    function clampCamera() {
        if (!state.world) return;

        const halfWidth = state.width / (2 * state.scale);
        const halfHeight = state.height / (2 * state.scale);

        state.camera.x = clamp(
            state.camera.x,
            halfWidth,
            state.world.width - halfWidth,
        );

        state.camera.y = clamp(
            state.camera.y,
            halfHeight,
            state.world.height - halfHeight,
        );
    }

    function viewBounds() {
        const halfWidth = state.width / (2 * state.scale);
        const halfHeight = state.height / (2 * state.scale);

        return {
            min_x: Math.max(0, Math.floor(state.camera.x - halfWidth) - 1),
            max_x: Math.min(
                state.world.width - 1,
                Math.ceil(state.camera.x + halfWidth) + 1,
            ),
            min_y: Math.max(0, Math.floor(state.camera.y - halfHeight) - 1),
            max_y: Math.min(
                state.world.height - 1,
                Math.ceil(state.camera.y + halfHeight) + 1,
            ),
        };
    }

    function toScreen(x, y) {
        return {
            x: (x - state.camera.x) * state.scale + state.width / 2,
            y: (y - state.camera.y) * state.scale + state.height / 2,
        };
    }

    function toWorld(x, y) {
        return {
            x: state.camera.x + (x - state.width / 2) / state.scale,
            y: state.camera.y + (y - state.height / 2) / state.scale,
        };
    }

    function eventPosition(event) {
        const rect = canvas.getBoundingClientRect();

        return {
            x: event.clientX - rect.left,
            y: event.clientY - rect.top,
        };
    }

    function scheduleDraw() {
        if (state.frame !== null) return;

        state.frame = requestAnimationFrame(() => {
            state.frame = null;
            draw();
        });
    }

    function updateView() {
        clampCamera();

        const x = Math.floor(state.camera.x);
        const y = Math.floor(state.camera.y);

        $("center-label").textContent = `${x} | ${y}`;
        $("continent").textContent = continent(x, y);

        scheduleDraw();

        clearTimeout(state.loadTimer);
        state.loadTimer = setTimeout(loadArea, 140);
    }

    async function loadArea() {
        if (!state.ready) return;

        state.controller?.abort();

        const controller = new AbortController();
        state.controller = controller;

        const requestId = ++state.requestId;
        const query = new URLSearchParams(viewBounds());

        try {
            const data = await api(`/api/map/area?${query}`, {
                signal: controller.signal,
            });

            if (requestId !== state.requestId) return;

            state.villages = data.villages;

            // Seçili köy görünür alandaysa güncel veriyi kullan.
            if (state.selected) {
                const updated = state.villages.find(
                    (village) => village.id === state.selected.id,
                );

                if (updated) selectVillage(updated);
            }

            $("village-count").textContent =
                `${data.villages.length} köy · görüntü alanı ve kenarları`;

            status("Harita güncel · Köyler PostgreSQL'den yükleniyor.");
            scheduleDraw();
        } catch (error) {
            if (error.name === "AbortError") return;
            if (requestId !== state.requestId) return;

            status(
                `${error.message}. Haritayı hareket ettirerek tekrar deneyebilirsin.`,
                true,
            );
        }
    }

    // Görsel arazi: aynı seed ve koordinat aynı sonucu üretir.
    // Oyun kuralları bu araziye bağlı değildir.
    function tileHash(x, y) {
        let value =
            Math.imul(x + 1, 374761393) ^
            Math.imul(y + 1, 668265263) ^
            state.world.map_seed;

        value = Math.imul(value ^ (value >>> 13), 1274126177);

        return (value ^ (value >>> 16)) >>> 0;
    }

    function drawTree(x, y, size) {
        ctx.fillStyle = "#705c3b";
        ctx.fillRect(x - size * 0.06, y, size * 0.12, size * 0.3);

        ctx.fillStyle = "#526e43";
        ctx.beginPath();
        ctx.moveTo(x, y - size * 0.48);
        ctx.lineTo(x - size * 0.3, y + size * 0.08);
        ctx.lineTo(x + size * 0.3, y + size * 0.08);
        ctx.closePath();
        ctx.fill();

        ctx.fillStyle = "#68814e";
        ctx.beginPath();
        ctx.moveTo(x, y - size * 0.48);
        ctx.lineTo(x - size * 0.3, y + size * 0.08);
        ctx.lineTo(x, y);
        ctx.closePath();
        ctx.fill();
    }

    function drawTile(x, y) {
        const point = toScreen(x, y);
        const size = state.scale;
        const hash = tileHash(x, y);

        const shades = [
            "#a8b584",
            "#a4b180",
            "#adba89",
            "#a1af7d",
            "#b0bc8b",
            "#a9b381",
        ];

        ctx.fillStyle = shades[hash % shades.length];
        ctx.fillRect(point.x, point.y, size + 0.5, size + 0.5);

        if (size >= 22) {
            const centerX = point.x + size / 2;
            const centerY = point.y + size / 2;

            if (hash % 17 < 3) {
                drawTree(centerX - size * 0.14, centerY, size * 0.65);
                drawTree(
                    centerX + size * 0.17,
                    centerY + size * 0.15,
                    size * 0.53,
                );
            } else if (hash % 29 === 0) {
                ctx.fillStyle = "#929a75";
                ctx.beginPath();
                ctx.ellipse(
                    centerX,
                    centerY + size * 0.12,
                    size * 0.27,
                    size * 0.13,
                    -0.3,
                    0,
                    Math.PI * 2,
                );
                ctx.fill();

                ctx.fillStyle = "#b7bd94";
                ctx.beginPath();
                ctx.ellipse(
                    centerX - size * 0.04,
                    centerY + size * 0.04,
                    size * 0.17,
                    size * 0.12,
                    -0.3,
                    0,
                    Math.PI * 2,
                );
                ctx.fill();
            }
        }

        if (size >= 28) {
            ctx.strokeStyle = "#52653b12";
            ctx.lineWidth = 1;
            ctx.strokeRect(point.x, point.y, size, size);
        }

        // Kıta sınırları.
        ctx.strokeStyle = "#f4edcb80";
        ctx.lineWidth = 2;

        if (x % 100 === 0) {
            ctx.beginPath();
            ctx.moveTo(point.x, point.y);
            ctx.lineTo(point.x, point.y + size);
            ctx.stroke();
        }

        if (y % 100 === 0) {
            ctx.beginPath();
            ctx.moveTo(point.x, point.y);
            ctx.lineTo(point.x + size, point.y);
            ctx.stroke();
        }
    }

    function drawVillage(village) {
        const point = toScreen(village.x + 0.5, village.y + 0.5);
        const size = state.scale;
        const selected = state.selected?.id === village.id;
        const own = village.affiliation === "own";

        const colors = {
            own: "#b28b40",
            player: "#9c5142",
            barbarian: "#777b68",
        };

        // Köyün oturduğu açıklık.
        ctx.fillStyle = own ? "#c8bc86" : "#b9b68a";
        ctx.beginPath();
        ctx.ellipse(
            point.x,
            point.y + size * 0.1,
            size * 0.42,
            size * 0.28,
            0,
            0,
            Math.PI * 2,
        );
        ctx.fill();

        if (selected || own) {
            ctx.strokeStyle = selected ? "#fff4ce" : "#dbc174";
            ctx.lineWidth = selected ? 2.5 : 1.5;
            ctx.beginPath();
            ctx.ellipse(
                point.x,
                point.y + size * 0.08,
                size * 0.43,
                size * 0.32,
                0,
                0,
                Math.PI * 2,
            );
            ctx.stroke();
        }

        if (size < 24) {
            ctx.fillStyle = colors[village.affiliation];
            ctx.fillRect(
                point.x - size * 0.18,
                point.y - size * 0.2,
                size * 0.36,
                size * 0.36,
            );
            return;
        }

        const width = size * 0.48;
        const height = size * 0.32;

        ctx.fillStyle = "#d8c497";
        ctx.fillRect(
            point.x - width / 2,
            point.y - height * 0.25,
            width,
            height,
        );

        ctx.fillStyle = "#b7a176";
        ctx.fillRect(
            point.x,
            point.y - height * 0.25,
            width / 2,
            height,
        );

        ctx.fillStyle = colors[village.affiliation];
        ctx.beginPath();
        ctx.moveTo(point.x, point.y - size * 0.4);
        ctx.lineTo(point.x - width * 0.65, point.y);
        ctx.lineTo(point.x + width * 0.65, point.y);
        ctx.closePath();
        ctx.fill();

        ctx.fillStyle = "#675a40";
        ctx.fillRect(
            point.x - size * 0.055,
            point.y + size * 0.04,
            size * 0.11,
            size * 0.2,
        );

        if (own) {
            ctx.strokeStyle = "#675634";
            ctx.lineWidth = 1.5;
            ctx.beginPath();
            ctx.moveTo(point.x, point.y - size * 0.38);
            ctx.lineTo(point.x, point.y - size * 0.66);
            ctx.stroke();

            ctx.fillStyle = "#e5c163";
            ctx.beginPath();
            ctx.moveTo(point.x, point.y - size * 0.66);
            ctx.lineTo(point.x + size * 0.23, point.y - size * 0.6);
            ctx.lineTo(point.x, point.y - size * 0.5);
            ctx.closePath();
            ctx.fill();
        }

        if (size >= 55) {
            ctx.font = "9px system-ui";
            ctx.textAlign = "center";

            const label =
                village.name.length > 18
                    ? `${village.name.slice(0, 17)}…`
                    : village.name;

            const labelWidth = ctx.measureText(label).width + 10;
            const labelY = point.y + size * 0.32;

            ctx.fillStyle = "#faf3dfed";
            ctx.fillRect(
                point.x - labelWidth / 2,
                labelY,
                labelWidth,
                15,
            );

            ctx.fillStyle = "#4b5139";
            ctx.fillText(label, point.x, labelY + 11);
        }
    }

    function drawOverview() {
        const width = overview.width;
        const height = overview.height;

        overviewCtx.clearRect(0, 0, width, height);
        overviewCtx.fillStyle = "#a8b382";
        overviewCtx.fillRect(0, 0, width, height);

        overviewCtx.strokeStyle = "#61734a35";
        overviewCtx.lineWidth = 1;

        for (let index = 1; index < 10; index++) {
            overviewCtx.beginPath();
            overviewCtx.moveTo(index * width / 10, 0);
            overviewCtx.lineTo(index * width / 10, height);
            overviewCtx.stroke();

            overviewCtx.beginPath();
            overviewCtx.moveTo(0, index * height / 10);
            overviewCtx.lineTo(width, index * height / 10);
            overviewCtx.stroke();
        }

        const bounds = viewBounds();

        overviewCtx.fillStyle = "#f7edc440";
        overviewCtx.strokeStyle = "#faf0c7";
        overviewCtx.lineWidth = 1.5;

        const x = bounds.min_x / state.world.width * width;
        const y = bounds.min_y / state.world.height * height;
        const w =
            (bounds.max_x - bounds.min_x + 1) / state.world.width * width;
        const h =
            (bounds.max_y - bounds.min_y + 1) / state.world.height * height;

        overviewCtx.fillRect(x, y, w, h);
        overviewCtx.strokeRect(x, y, w, h);

        overviewCtx.fillStyle = "#e9c469";
        overviewCtx.strokeStyle = "#675332";
        overviewCtx.lineWidth = 1;

        overviewCtx.beginPath();
        overviewCtx.arc(
            (state.home.x + 0.5) / state.world.width * width,
            (state.home.y + 0.5) / state.world.height * height,
            3.5,
            0,
            Math.PI * 2,
        );
        overviewCtx.fill();
        overviewCtx.stroke();
    }

    function draw() {
        if (!state.ready) return;

        ctx.setTransform(state.dpr, 0, 0, state.dpr, 0, 0);
        ctx.clearRect(0, 0, state.width, state.height);

        const bounds = viewBounds();

        for (let y = bounds.min_y; y <= bounds.max_y; y++) {
            for (let x = bounds.min_x; x <= bounds.max_x; x++) {
                drawTile(x, y);
            }
        }
        for (const village of state.villages) {
            drawVillage(village);
        }

        drawCoordinateRulers();
        drawOverview();
        positionVillageActions();
    }
    const commandDialog = $("command-dialog");

    async function openCommand(type) {
        if (attackSending) return;

        const selected = state.selected;

        if (!state.ready || !selected) return;

        if (selected.affiliation === "own") {
            status("Kendi köyüne saldıramazsın.", true);
            return;
        }

        // Son gönderimde ağ hatası olduysa önce o isteği sonuçlandır.
        if (
            pendingAttack &&
            (
                type !== "attack" ||
                selected.id !== pendingAttack.target_id
            )
        ) {
            status(
                "Önce son saldırının hedefini seçip gönderimi tekrar dene. " +
                "Aynı istek ikinci bir saldırı oluşturmaz.",
                true,
            );
            return;
        }

        attackTarget = { ...selected };

        const titles = {
            attack: "Saldırı emri",
            support: "Destek gönder",
            resources: "Hammadde gönder",
        };

        if (!titles[type]) return;

        $("command-title").textContent = titles[type];
        $("command-target-name").textContent = attackTarget.name;

        $("command-target-location").textContent =
            `${attackTarget.x} | ${attackTarget.y} · ` +
            continent(attackTarget.x, attackTarget.y);

        const distance = Math.hypot(
            attackTarget.x - state.home.x,
            attackTarget.y - state.home.y,
        );

        $("command-distance").textContent =
            `Mesafe: ${distance.toLocaleString("tr-TR", {
                maximumFractionDigits: 2,
            })} kare`;

        $("attack-result").textContent = "";
        $("attack-fields").hidden = type !== "attack";
        $("command-submit").disabled = true;

        if (!$("command-dialog").open) {
            $("command-dialog").showModal();
        }

        if (type !== "attack") {
            $("command-information").textContent =
                "Bu komut henüz kullanılabilir değil.";

            $("command-submit").textContent = "Henüz kapalı";
            return;
        }

        $("command-information").textContent =
            "Göndereceğin mızrakçı sayısını seç. " +
            "Birlikler köyünden hemen ayrılır ve savaşta kaybedilebilir.";

        $("command-submit").textContent = pendingAttack
            ? "Aynı gönderimi tekrar dene"
            : "Saldırıyı gönder";

        $("attack-spears").readOnly = Boolean(pendingAttack);

        if (pendingAttack) {
            $("attack-spears").value = pendingAttack.spears;
            $("attack-all").disabled = true;
            $("command-submit").disabled = false;
            return;
        }

        $("attack-spears").value = "1";

        await refreshMilitary();

        // Pencere başka komuta geçmiş olabilir.
        if (
            $("command-dialog").open &&
            !$("attack-fields").hidden
        ) {
            updateAttackFields();
        }
    }

    $("village-actions").addEventListener("click", (event) => {
        const button = event.target.closest("[data-command]");

        if (!button || button.disabled) return;

        void openCommand(button.dataset.command);
    });

    $("command-close").addEventListener("click", () => {
        commandDialog.close();
    });

    $("attack-spears").addEventListener("input", () => {
        if (!pendingAttack) updateAttackFields();
    });

    $("attack-all").addEventListener("click", () => {
        if (!militarySnapshot || pendingAttack) return;

        $("attack-spears").value = militarySnapshot.spears;
        updateAttackFields();
    });

    $("command-submit").addEventListener("click", async () => {
        if (
            attackSending ||
            $("attack-fields").hidden ||
            !attackTarget
        ) {
            return;
        }

        if (!pendingAttack) {
            const spears = Number($("attack-spears").value);

            if (
                !Number.isInteger(spears) ||
                spears < 1 ||
                spears > (militarySnapshot?.spears ?? 0)
            ) {
                $("attack-result").textContent =
                    "Geçerli bir asker sayısı seç.";
                return;
            }

            pendingAttack = {
                request_id: crypto.randomUUID(),
                target_id: attackTarget.id,
                spears,
            };
        }

        attackSending = true;

        $("command-submit").disabled = true;
        $("attack-spears").readOnly = true;
        $("attack-all").disabled = true;

        $("attack-result").textContent = "Saldırı gönderiliyor…";

        try {
            const result = await militaryApi("/api/military/attacks", {
                method: "POST",
                body: JSON.stringify(pendingAttack),
            });

            pendingAttack = null;

            $("attack-result").textContent =
                `Ordu yola çıktı. Varış: ${militaryDate(result.arrives_at)}`;

            $("command-dialog").close();

            status(
                `Saldırı gönderildi. Varış: ${militaryDate(result.arrives_at)}`,
            );
        } catch (error) {
            // Kesin doğrulama hatasında yeni seçim yapılabilir.
            // Ağ/5xx hatasında aynı request_id korunur.
            if (error.status >= 400 && error.status < 500) {
                pendingAttack = null;
            }

            $("attack-result").textContent = pendingAttack
                ? `${error.message}. Aynı gönderimi tekrar deneyebilirsin.`
                : error.message;
        } finally {
            attackSending = false;

            $("attack-spears").readOnly = Boolean(pendingAttack);

            await refreshMilitary();

            if (pendingAttack) {
                $("command-submit").textContent = "Aynı gönderimi tekrar dene";
                $("command-submit").disabled = false;
                $("attack-all").disabled = true;
            } else {
                $("command-submit").textContent = "Saldırıyı gönder";
                updateAttackFields();
            }
        }
    });

    function selectVillage(village) {
        state.selected = village;

        $("selected-name").textContent = village.name;
        $("selected-coordinate").textContent =
            `${village.x} | ${village.y} · ${continent(village.x, village.y)}`;

        $("village-type").textContent =
            affiliationLabels[village.affiliation];

        $("selected-owner").textContent = {
            own: "Sana ait",
            player: "Başka oyuncu",
            barbarian: "Sahipsiz",
        }[village.affiliation];

        $("selected-points").textContent =
            village.points.toLocaleString("tr-TR");

        const distance = Math.hypot(
            village.x - state.home.x,
            village.y - state.home.y,
        );

        $("selected-distance").textContent =
            `${distance.toLocaleString("tr-TR", {
                maximumFractionDigits: 2,
            })} kare`;

        $("center-selected").disabled = false;
        $("open-village").hidden = village.affiliation !== "own";

        scheduleDraw();
    }

    function centerOn(x, y) {
        if (!state.ready) return;

        state.camera.x = x + 0.5;
        state.camera.y = y + 0.5;

        $("coordinate-x").value = x;
        $("coordinate-y").value = y;

        updateView();
    }

    function zoom(factor, anchorX, anchorY) {
        if (!state.ready) return;

        const before = toWorld(anchorX, anchorY);
        const min = minimumScale();

        state.scale = clamp(
            state.scale * factor,
            min,
            Math.max(80, min),
        );

        const after = toWorld(anchorX, anchorY);

        state.camera.x += before.x - after.x;
        state.camera.y += before.y - after.y;

        updateView();
    }

    function resize() {
        const rect = stage.getBoundingClientRect();

        state.width = Math.max(1, rect.width);
        state.height = Math.max(1, rect.height);
        state.dpr = Math.min(window.devicePixelRatio || 1, 2);

        canvas.width = Math.round(state.width * state.dpr);
        canvas.height = Math.round(state.height * state.dpr);

        state.scale = Math.max(state.scale, minimumScale());

        if (state.ready) updateView();
    }

    canvas.addEventListener("pointerdown", (event) => {
        if (!state.ready || !event.isPrimary || event.button !== 0) return;

        canvas.focus({ preventScroll: true });

        const point = eventPosition(event);

        state.drag = {
            pointerId: event.pointerId,
            startX: point.x,
            startY: point.y,
            cameraX: state.camera.x,
            cameraY: state.camera.y,
            moved: false,
        };

        canvas.setPointerCapture(event.pointerId);
        canvas.classList.add("dragging");
    });

    canvas.addEventListener("pointermove", (event) => {
        if (!state.ready) return;

        const point = eventPosition(event);
        const drag = state.drag;

        if (drag && event.pointerId === drag.pointerId) {
            const dx = point.x - drag.startX;
            const dy = point.y - drag.startY;

            if (Math.hypot(dx, dy) > 5) drag.moved = true;

            state.camera.x = drag.cameraX - dx / state.scale;
            state.camera.y = drag.cameraY - dy / state.scale;

            updateView();
        }

        const worldPoint = toWorld(point.x, point.y);

        const x = clamp(Math.floor(worldPoint.x), 0, 999);
        const y = clamp(Math.floor(worldPoint.y), 0, 999);

        $("hover-coordinate").textContent =
            `${x} | ${y} · ${continent(x, y)}`;
    });

    canvas.addEventListener("pointerup", (event) => {
        const drag = state.drag;

        if (!drag || event.pointerId !== drag.pointerId) return;

        if (!drag.moved) {
            const point = eventPosition(event);
            const worldPoint = toWorld(point.x, point.y);

            const x = Math.floor(worldPoint.x);
            const y = Math.floor(worldPoint.y);

            const village = state.villages.find(
                (item) => item.x === x && item.y === y,
            );

            if (village) {
                selectVillage(village);
            } else {
                state.selected = null;
                $("village-actions").hidden = true;
                $("center-selected").disabled = true;
                $("open-village").hidden = true;

                $("selected-name").textContent = "Bir köy seç";
                $("selected-coordinate").textContent =
                    "Haritadaki bir yerleşime tıkla.";

                $("selected-owner").textContent = "—";
                $("selected-points").textContent = "—";
                $("selected-distance").textContent = "—";
                $("village-type").textContent = "—";

                scheduleDraw();
            }
        }

        state.drag = null;
        canvas.classList.remove("dragging");

        if (canvas.hasPointerCapture(event.pointerId)) {
            canvas.releasePointerCapture(event.pointerId);
        }
    });

    function drawCoordinateRulers() {
        const size = state.scale;

        const startX = Math.max(
            0,
            Math.floor(state.camera.x - state.width / (2 * size)),
        );

        const endX = Math.min(
            state.world.width - 1,
            Math.ceil(state.camera.x + state.width / (2 * size)),
        );

        const startY = Math.max(
            0,
            Math.floor(state.camera.y - state.height / (2 * size)),
        );

        const endY = Math.min(
            state.world.height - 1,
            Math.ceil(state.camera.y + state.height / (2 * size)),
        );

        const step = Math.max(1, Math.ceil(35 / size));

        ctx.save();

        ctx.fillStyle = "#3f501fcc";
        ctx.fillRect(0, 0, 30, state.height);
        ctx.fillRect(0, state.height - 20, state.width, 20);

        ctx.fillStyle = "#f3edc8";
        ctx.font = "10px Arial";
        ctx.textBaseline = "middle";

        ctx.textAlign = "center";

        for (let x = startX; x <= endX; x++) {
            if (x % step !== 0) continue;

            const point = toScreen(x + 0.5, state.camera.y);

            if (point.x > 36 && point.x < state.width - 10) {
                ctx.fillText(
                    String(x),
                    point.x,
                    state.height - 10,
                );
            }
        }

        for (let y = startY; y <= endY; y++) {
            if (y % step !== 0) continue;

            const point = toScreen(state.camera.x, y + 0.5);

            if (point.y > 8 && point.y < state.height - 25) {
                ctx.fillText(String(y), 15, point.y);
            }
        }

        ctx.restore();
    }

    function cancelDrag() {
        state.drag = null;
        canvas.classList.remove("dragging");
    }

    canvas.addEventListener("pointercancel", cancelDrag);
    canvas.addEventListener("lostpointercapture", cancelDrag);

    canvas.addEventListener("wheel", (event) => {
        if (!state.ready) return;

        event.preventDefault();

        const point = eventPosition(event);
        zoom(event.deltaY < 0 ? 1.15 : 1 / 1.15, point.x, point.y);
    }, { passive: false });

    canvas.addEventListener("keydown", (event) => {
        if (!state.ready) return;

        const directions = {
            ArrowLeft: [-3, 0],
            ArrowRight: [3, 0],
            ArrowUp: [0, -3],
            ArrowDown: [0, 3],
        };

        const direction = directions[event.key];
        if (!direction) return;

        event.preventDefault();

        state.camera.x += direction[0];
        state.camera.y += direction[1];

        updateView();
    });

    $("zoom-in").addEventListener("click", () => {
        zoom(1.25, state.width / 2, state.height / 2);
    });

    $("zoom-out").addEventListener("click", () => {
        zoom(1 / 1.25, state.width / 2, state.height / 2);
    });

    $("go-home").addEventListener("click", () => {
        if (!state.ready) return;

        centerOn(state.home.x, state.home.y);
        selectVillage({ ...state.home, affiliation: "own" });
    });

    $("center-selected").addEventListener("click", () => {
        if (!state.selected) return;

        centerOn(state.selected.x, state.selected.y);
    });

    $("coordinate-form").addEventListener("submit", (event) => {
        event.preventDefault();

        if (!state.ready) return;

        const x = Number($("coordinate-x").value);
        const y = Number($("coordinate-y").value);

        if (
            !Number.isInteger(x) ||
            !Number.isInteger(y) ||
            x < 0 || x > 999 ||
            y < 0 || y > 999
        ) {
            status("Koordinatlar 0–999 arasında tam sayı olmalı.", true);
            return;
        }

        centerOn(x, y);
    });

    function positionVillageActions() {
        const menu = $("village-actions");
        const village = state.selected;

        if (!state.ready || !village) {
            menu.hidden = true;
            return;
        }

        const point = toScreen(
            village.x + 0.5,
            village.y + 0.5,
        );

        const visible =
            point.x >= 0 &&
            point.y >= 0 &&
            point.x <= state.width &&
            point.y <= state.height;

        if (!visible) {
            menu.hidden = true;
            return;
        }

        menu.hidden = false;

        const ownVillage = village.affiliation === "own";

        menu.querySelectorAll("[data-command]").forEach((button) => {
            // İlk sürümde oyuncunun tek köyü var.
            button.disabled = ownVillage;
            button.title = ownVillage
                ? "Bu komut kendi köyüne gönderilemez."
                : "";
        });

        const width = menu.offsetWidth;
        const height = menu.offsetHeight;

        const left = clamp(
            point.x - width / 2,
            6,
            Math.max(6, state.width - width - 6),
        );

        let top = point.y - state.scale * 0.6 - height;

        if (top < 6) {
            top = point.y + state.scale * 0.45;
        }

        top = clamp(
            top,
            6,
            Math.max(6, state.height - height - 6),
        );

        menu.style.left = `${left}px`;
        menu.style.top = `${top}px`;
    }
    let militarySnapshot = null;
    let militaryRefreshing = false;

    let attackTarget = null;
    let attackSending = false;

    // Ağ hatasında aynı isteği aynı kimlikle tekrar göndermek için.
    let pendingAttack = null;

    function escapeMilitaryHtml(value) {
        return String(value).replace(/[&<>"']/g, (character) => ({
            "&": "&amp;",
            "<": "&lt;",
            ">": "&gt;",
            '"': "&quot;",
            "'": "&#39;",
        })[character]);
    }

    async function militaryApi(path, options = {}) {
        const response = await fetch(path, {
            credentials: "same-origin",
            cache: "no-store",
            ...options,
            headers: {
                ...(options.body ? { "Content-Type": "application/json" } : {}),
                ...(options.headers || {}),
            },
        });

        const body = await response.json().catch(() => null);

        if (!response.ok) {
            const error = new Error(
                body?.error || `İstek başarısız: HTTP ${response.status}`,
            );

            error.status = response.status;
            throw error;
        }

        return body;
    }

    function militaryDate(value) {
        return new Date(value).toLocaleString("tr-TR");
    }

    async function refreshMilitary() {
        if (!state.ready || militaryRefreshing) return;

        militaryRefreshing = true;

        try {
            militarySnapshot = await militaryApi("/api/military");

            $("army-home-count").textContent =
                `· Köyde ${militarySnapshot.spears} mızrakçı`;

            const statusNames = {
                outbound: "Hedefe gidiyor",
                returning: "Köye dönüyor",
                completed: "Tamamlandı",
            };

            $("army-orders").innerHTML = militarySnapshot.attacks.length
                ? militarySnapshot.attacks.map((attack) => {
                    const resolved = attack.resolved_at !== null;

                    return `
            <article class="army-order">
              <div>
                <strong>
                  ${escapeMilitaryHtml(attack.target_name)}
                  (${attack.target_x}|${attack.target_y})
                </strong>

                <span>
                  ${escapeMilitaryHtml(statusNames[attack.status])}
                </span>
              </div>

              <p>
                Gönderilen: ${attack.sent_spears} mızrakçı
                · Varış:
                ${escapeMilitaryHtml(militaryDate(attack.arrives_at))}
              </p>

              ${resolved ? `
                <p class="army-report">
                  Sağ kalan: ${attack.surviving_spears}
                  · Kaybedilen:
                  ${attack.sent_spears - attack.surviving_spears}
                  · Savunmacı:
                  ${attack.defender_before} → ${attack.defender_after}
                </p>
              ` : ""}

              ${attack.returns_at ? `
                <p>
                  Dönüş:
                  ${escapeMilitaryHtml(militaryDate(attack.returns_at))}
                  ${attack.returned_at ? " · Köye ulaştı" : ""}
                </p>
              ` : ""}
            </article>
          `;
                }).join("")
                : "<p>Henüz saldırı göndermedin.</p>";

            if (
                $("command-dialog").open &&
                !$("attack-fields").hidden &&
                !attackSending &&
                !pendingAttack
            ) {
                updateAttackFields();
            }
        } catch (error) {
            $("army-home-count").textContent =
                `· ${error.message}`;
        } finally {
            militaryRefreshing = false;
        }
    }

    function updateAttackFields() {
        if (!militarySnapshot) return;

        const count = militarySnapshot.spears;
        const input = $("attack-spears");

        $("available-spears").textContent = count;
        input.max = String(Math.max(1, count));

        const selectedCount = Number(input.value);

        $("command-submit").disabled =
            attackSending ||
            !Number.isInteger(selectedCount) ||
            selectedCount < 1 ||
            selectedCount > count;

        $("attack-all").disabled = attackSending || count === 0;

        if (attackTarget) {
            const distance = Math.hypot(
                attackTarget.x - state.home.x,
                attackTarget.y - state.home.y,
            );

            const seconds = Math.max(
                1,
                Math.ceil(distance * militarySnapshot.seconds_per_tile),
            );

            $("attack-travel").textContent =
                `Tahmini yolculuk: ${seconds} saniye. ` +
                "Kesin varış zamanı gönderimde sunucu tarafından belirlenir.";
        }
    }

    async function start() {
        try {
            const data = await api("/api/map/bootstrap");

            state.world = data.world;
            state.home = data.village;
            if (!state.home) {
                status("Önce ilk obanı kurmalısın. Köy merkezine yönlendiriliyorsun.");
                location.replace("/game");
                return;
            }

            $("world-name").textContent = state.world.name;

            state.camera.x = state.home.x + 0.5;
            state.camera.y = state.home.y + 0.5;
            state.ready = true;
            void refreshMilitary();

            state.villages = [{
                ...state.home,
                affiliation: "own",
            }];

            selectVillage(state.villages[0]);
            centerOn(state.home.x, state.home.y);

            status("Oban hazır. Çevredeki köyler yükleniyor…");
        } catch (error) {
            status(
                `${error.message}. Bağlantıyı kontrol edip sayfayı yenile.`,
                true,
            );
        }
    }

    new ResizeObserver(resize).observe(stage);

    resize();
    void start();
})();