<script>
    import { onMount, tick } from "svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { listen } from "@tauri-apps/api/event";
    import { getCurrentWindow } from "@tauri-apps/api/window";
    import { exit } from "@tauri-apps/plugin-process";
    import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
    import { open as dialogOpen } from "@tauri-apps/plugin-dialog";

    let loading = true;
    let options = null;
    let latestRelease = null;
    let modsList = [];
    let customModList = [];
    let modsShown = false;

    let offlineUsername = "";
    let msAuthState = null;
    let addingAccount = false;
    let accountDropdownShown = false;
    let settingsShown = false;

    let running = false;
    let launched = false;
    let progressText = "";
    let progressValue = 0;
    let progressMax = 0;

    let log = [];
    let logShown = false;

    let systemMemory = 8192;
    let defaultDataFolder = "";

    async function setupOptions() {
        try {
            options = {
                store: async function () {
                    try {
                        await invoke("store_options", { options });
                    } catch (e) {
                        console.error("Failed to store options:", e);
                    }
                },
                ...await invoke("get_options")
            };
            if (!options.start) {
                options.start = { account: null, accounts: [], customDataPath: "", memory: 4096, javaDistribution: { type: "manual", value: "temurin" }, jvmArgs: [] };
                options.launcher = { concurrentDownloads: 10, keepLauncherOpen: false };
            }
            if (!Array.isArray(options.start.accounts)) {
                options.start.accounts = options.start.account ? [options.start.account] : [];
            }
            if (!Array.isArray(options.start.jvmArgs)) {
                options.start.jvmArgs = [];
            }
        } catch (e) {
            console.error("Failed to load options:", e);
        }
    }

    function accountKey(acc) {
        return `${acc.type}:${acc.id || acc.uuid || acc.name}`;
    }

    function accountAvatar(acc) {
        const id = acc.id || acc.uuid;
        return id ? `https://minotar.net/helm/${id}/20.png` : null;
    }

    function hideAvatar(e) {
        e.target.style.display = "none";
    }

    function upsertAccount(account) {
        const key = accountKey(account);
        const idx = options.start.accounts.findIndex(a => accountKey(a) === key);
        if (idx >= 0) options.start.accounts[idx] = account;
        else options.start.accounts = [...options.start.accounts, account];
        options.start.account = account;
    }

    async function handleLogin(username) {
        if (!username || username.trim().length === 0) return;
        try {
            const account = await invoke("login_offline", { username: username.trim() });
            upsertAccount(account);
            addingAccount = false;
            offlineUsername = "";
            await options.store();
        } catch (e) {
            console.error("Login failed:", e);
        }
    }

    async function handleMicrosoftLogin() {
        msAuthState = { code: null, uri: null };
        try {
            const account = await invoke("login_microsoft");
            upsertAccount(account);
            addingAccount = false;
            await options.store();
        } catch (e) {
            console.error("Microsoft login failed:", e);
        }
        msAuthState = null;
    }

    async function switchAccount(account) {
        if (accountKey(account) === accountKey(options.start.account)) return;
        try {
            account = await invoke("refresh", { accountData: account });
            upsertAccount(account);
        } catch (e) {
            console.error("Refresh failed:", e);
            options.start.account = account;
        }
        await options.store();
    }

    async function removeAccount(account) {
        const key = accountKey(account);
        if (options.start.account && accountKey(options.start.account) === key) {
            try {
                await invoke("logout", { accountData: account });
            } catch (e) {
                console.error("Logout failed:", e);
            }
            options.start.account = null;
        }
        options.start.accounts = options.start.accounts.filter(a => accountKey(a) !== key);
        await options.store();
    }

    async function handleLaunch() {
        try {
            await refreshActiveAccount();
            await options.store();
            running = true;
            launched = false;
            log = [];
            await invoke("run_client", { options });
        } catch (e) {
            running = false;
            console.error("Launch failed:", e);
        }
    }

    async function refreshActiveAccount() {
        if (!options.start.account) return;
        try {
            const refreshed = await invoke("refresh", { accountData: options.start.account });
            upsertAccount(refreshed);
            await options.store();
        } catch (e) {
            console.error("Refresh failed:", e);
        }
    }

    async function handleTerminate() {
        try {
            await invoke("terminate");
        } catch (e) {
            console.error("Terminate failed:", e);
        }
    }

    async function handleMinimize() {
        try { await getCurrentWindow().minimize(); } catch (e) {}
    }

    async function handleClose() {
        try { await exit(0); } catch (e) {}
    }

    async function copyCode(code) {
        try {
            await navigator.clipboard.writeText(code);
        } catch (e) {
            console.error("Failed to copy code:", e);
        }
    }

    async function selectDataPath() {
        const path = await dialogOpen({ directory: true, title: "Выберите папку для данных" });
        if (path) {
            options.start.customDataPath = path;
            await options.store();
        }
    }

    async function openGameDir() {
        const dir = options.start.customDataPath || defaultDataFolder;
        if (dir) {
            try {
                await revealItemInDir(dir);
            } catch (e) {
                console.error("Failed to open directory:", e);
            }
        }
    }

    async function loadMods() {
        try {
            modsList = await invoke("get_predefined_mods", { options });
        } catch (e) { console.error("get_predefined_mods:", e); }
        try {
            customModList = await invoke("get_custom_mods", { options });
        } catch (e) { console.error("get_custom_mods:", e); }
    }

    async function toggleMod(id) {
        const mod = modsList.find(m => m.id === id);
        if (!mod) return;
        try {
            const updatedMods = await invoke("set_predefined_mod_enabled", { options, id, enabled: !mod.enabled });
            mod.enabled = !mod.enabled;
            if (updatedMods) {
                options.predefinedMods = updatedMods;
            }
            await loadMods();
        } catch (e) { console.error("toggle_mod:", e); }
    }

    async function addCustomMod() {
        const file = await dialogOpen({
            multiple: false,
            title: "Выберите .jar мод",
            filters: [{ name: "Моды", extensions: ["jar"] }]
        });
        if (!file) return;
        try {
            await invoke("install_custom_mod", { options, sourcePath: file });
            await loadMods();
        } catch (e) { console.error("install_custom_mod:", e); }
    }

    async function deleteCustomMod(filename) {
        try {
            await invoke("delete_custom_mod", { options, filename });
            customModList = customModList.filter(m => m.filename !== filename);
        } catch (e) { console.error("delete_custom_mod:", e); }
    }

    async function toggleCustomMod(filename) {
        const mod = customModList.find(m => m.filename === filename);
        if (!mod) return;
        try {
            await invoke("toggle_custom_mod", { options, filename, enabled: !mod.enabled });
            mod.enabled = !mod.enabled;
            customModList = [...customModList];
        } catch (e) { console.error("toggle_custom_mod:", e); }
    }

    let canvas;
    let animId;

    function setupStars() {
        const ctx = canvas.getContext("2d");
        const dpr = window.devicePixelRatio || 1;
        const w = window.innerWidth;
        const h = window.innerHeight;
        canvas.width = w * dpr;
        canvas.height = h * dpr;
        canvas.style.width = w + "px";
        canvas.style.height = h + "px";
        ctx.scale(dpr, dpr);

        const stars = [];
        const count = 30;
        const angle = Math.PI / 4;

        for (let i = 0; i < count; i++) {
            stars.push({
                x: Math.random() * w,
                y: Math.random() * h,
                s: 0.5 + Math.random() * 1.5,
                sp: 0.8 + Math.random() * 2.5,
                trail: 30 + Math.random() * 80,
                hue: 200 + Math.random() * 40,
                op: 0.15 + Math.random() * 0.6,
            });
        }

        function draw() {
            ctx.clearRect(0, 0, w, h);

            for (const star of stars) {
                const vx = Math.cos(angle) * star.sp;
                const vy = Math.sin(angle) * star.sp;

                const tx = star.x - vx * (star.trail / Math.max(star.sp, 0.1));
                const ty = star.y - vy * (star.trail / Math.max(star.sp, 0.1));

                const gradient = ctx.createLinearGradient(star.x, star.y, tx, ty);
                gradient.addColorStop(0, `rgba(255,255,255,${star.op})`);
                gradient.addColorStop(1, "rgba(255,255,255,0)");

                ctx.beginPath();
                ctx.strokeStyle = gradient;
                ctx.lineWidth = star.s;
                ctx.moveTo(star.x, star.y);
                ctx.lineTo(tx, ty);
                ctx.stroke();

                ctx.beginPath();
                ctx.arc(star.x, star.y, star.s * 1.2, 0, Math.PI * 2);
                ctx.fillStyle = `hsla(${star.hue}, 80%, 80%, ${star.op})`;
                ctx.fill();

                star.x += vx;
                star.y += vy;

                if (star.y > h + 20) { star.x = Math.random() * w; star.y = -10; }
                if (star.x > w + 20) { star.x = -10; }
                if (star.x < -20) { star.x = w + 10; }
                if (star.y < -20) { star.y = h + 10; }
            }

            animId = requestAnimationFrame(draw);
        }

        draw();
    }

    function handleResize() {
        if (animId) cancelAnimationFrame(animId);
        if (canvas) setupStars();
    }

    listen("process-output", (e) => {
        log = [...log, e.payload];
    });
    listen("progress-update", (e) => {
        const u = e.payload;
        if (u.type === "label") {
            progressText = u.value;
            if (u.value === "Запущено") launched = true;
        }
        else if (u.type === "max") progressMax = u.value;
        else if (u.type === "progress") progressValue = u.value;
    });
    listen("client-exited", () => {
        running = false;
        launched = false;
    });
    listen("client-error", () => {
        logShown = true;
    });
    listen("microsoft_code", (e) => {
        msAuthState = { code: e.payload.code, uri: e.payload.uri };
    });

    onMount(async () => {
        await setupOptions();
        try { systemMemory = await invoke("sys_memory"); } catch (e) {}
        try { defaultDataFolder = await invoke("default_data_folder_path"); } catch (e) {}
        try { latestRelease = await invoke("fetch_latest_release"); } catch (e) {}
        await loadMods();
        loading = false;
        refreshActiveAccount();
        await tick();
        if (canvas) setupStars();
        window.addEventListener("resize", handleResize);
        return () => {
            window.removeEventListener("resize", handleResize);
            if (animId) cancelAnimationFrame(animId);
        };
    });
</script>

<canvas bind:this={canvas} id="starCanvas"></canvas>

<div class="drag-zone" data-tauri-drag-region></div>

<div class="window-controls">
    <button class="win-btn win-btn-minimize" on:click={handleMinimize} title="свернуть">
        <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0" y="4.5" width="10" height="1" fill="rgba(255,255,255,0.4)"/></svg>
    </button>
    <button class="win-btn win-btn-close" on:click={handleClose} title="закрыть">
        <svg width="10" height="10" viewBox="0 0 10 10">
            <line x1="1" y1="1" x2="9" y2="9" stroke="rgba(255,255,255,0.4)" stroke-width="1"/>
            <line x1="9" y1="1" x2="1" y2="9" stroke="rgba(255,255,255,0.4)" stroke-width="1"/>
        </svg>
    </button>
</div>

<div class="hero-container">
    {#if loading}
        <div class="loading-text">загрузка...</div>
    {:else}
        <h1 class="hero-title">
            ancient <span class="version-text">1.21.4</span>
        </h1>

        {#if msAuthState}
            <div class="card">
                <div class="card-content">
                    <div class="ms-code-text">
                        Откройте <a href={msAuthState.uri || "https://microsoft.com/link"} on:click|preventDefault={() => openUrl(msAuthState.uri || "https://microsoft.com/link")}>{msAuthState.uri || "microsoft.com/link"}</a>
                        и введите код:
                    </div>
                    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
                    <div class="ms-code" on:click={() => msAuthState.code && copyCode(msAuthState.code)} title="нажмите чтобы скопировать">
                        {msAuthState.code || "получение кода..."}
                    </div>
                    <button class="btn btn-primary" disabled>
                        ожидание авторизации...
                    </button>
                </div>
            </div>
        {:else if addingAccount || !options?.start?.accounts?.length}
            <div class="card">
                <div class="card-content">
                    <div class="input-group">
                        <input type="text" class="text-input" placeholder="никнейм" bind:value={offlineUsername} maxlength="16" />
                    </div>
                    <button class="btn btn-primary" on:click={() => handleLogin(offlineUsername)} disabled={!offlineUsername.trim()}>
                        войти в оффлайн
                    </button>
                    <button class="btn btn-secondary" on:click={handleMicrosoftLogin}>
                        microsoft
                    </button>
                    {#if addingAccount && options?.start?.accounts?.length}
                        <button class="btn btn-link" on:click={() => addingAccount = false}>отмена</button>
                    {/if}
                </div>
            </div>
        {:else}
            <div class="card card-wide">
                <div class="card-header">
                    <div class="account-dropdown-wrap">
                        <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
                        <div class="user-info user-info-btn" on:click={() => accountDropdownShown = !accountDropdownShown}>
                            {#if options.start.account && accountAvatar(options.start.account)}
                                <img class="avatar" src={accountAvatar(options.start.account)} alt="" on:error={hideAvatar} />
                            {/if}
                            <span class="user-name">{options.start.account?.name || "выберите аккаунт"}</span>
                            {#if options.start.account}
                                <span class="user-type">{options.start.account.type}</span>
                            {/if}
                            <svg class="dropdown-arrow" class:dropdown-arrow-open={accountDropdownShown} width="10" height="10" viewBox="0 0 10 10">
                                <path d="M2 3.5 L5 6.5 L8 3.5" stroke="rgba(255,255,255,0.4)" stroke-width="1.2" fill="none" stroke-linecap="round"/>
                            </svg>
                        </div>
                        {#if accountDropdownShown}
                            <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
                            <div class="dropdown-overlay" on:click={() => accountDropdownShown = false}></div>
                            <div class="account-dropdown">
                                {#each options.start.accounts as acc}
                                    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
                                    <div
                                        class="account-row"
                                        class:account-active={options.start.account && accountKey(acc) === accountKey(options.start.account)}
                                        on:click={() => { switchAccount(acc); accountDropdownShown = false; }}
                                    >
                                        {#if accountAvatar(acc)}
                                            <img class="avatar" src={accountAvatar(acc)} alt="" on:error={hideAvatar} />
                                        {/if}
                                        <span class="account-name">{acc.name}</span>
                                        <span class="account-type">{acc.type}</span>
                                        <button class="btn-icon btn-icon-sm account-remove" on:click|stopPropagation={() => removeAccount(acc)} title="удалить">
                                            <svg width="10" height="10" viewBox="0 0 10 10">
                                                <line x1="1" y1="1" x2="9" y2="9" stroke="rgba(255,255,255,0.4)" stroke-width="1"/>
                                                <line x1="9" y1="1" x2="1" y2="9" stroke="rgba(255,255,255,0.4)" stroke-width="1"/>
                                            </svg>
                                        </button>
                                    </div>
                                {/each}
                                <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
                                <div class="account-row account-add" on:click={() => { addingAccount = true; offlineUsername = ""; accountDropdownShown = false; }}>
                                    <span class="account-add-text">+ добавить аккаунт</span>
                                </div>
                            </div>
                        {/if}
                    </div>
                </div>

                <div class="card-content">
                    <div class="slider-group">
                        <div class="slider-label">
                            <span>память</span>
                            <span class="slider-value">{options.start.memory} MB</span>
                        </div>
                        <div class="slider-container">
                            <input type="range" class="thick-slider" min="1024" max={systemMemory} step="128" bind:value={options.start.memory} />
                        </div>
                    </div>

                    <div class="split-buttons">
                        <button class="btn btn-secondary" on:click={() => modsShown = true}>моды</button>
                        <button class="btn btn-secondary" on:click={() => settingsShown = true}>настройки</button>
                    </div>

                    {#if running}
                        <div class="progress-section">
                            {#if progressText}
                                <div class="progress-text">{progressText}</div>
                            {/if}
                            <progress class="progress-bar" value={progressValue} max={progressMax}></progress>
                            <div class="running-buttons">
                                <button class="btn btn-danger" on:click={handleTerminate}>{launched ? "выйти" : "отмена"}</button>
                                <button class="btn btn-secondary" on:click={() => logShown = !logShown}>лог</button>
                            </div>
                        </div>
                    {:else}
                        <button class="btn btn-primary btn-launch" on:click={handleLaunch} disabled={!options.start.account}>
                            запустить
                        </button>
                    {/if}
                </div>

                {#if latestRelease}
                    <div class="card-footer">
                        последний релиз: {latestRelease.tag_name}
                    </div>
                {/if}
            </div>
        {/if}
    {/if}
</div>

{#if logShown && log.length > 0}
    <div class="log-overlay">
        <div class="log-header">
            <span>лог</span>
            <button class="btn-link" on:click={() => logShown = false}>закрыть</button>
        </div>
        <div class="log-content">
            {#each log as line}
                <pre class="log-line">{line}</pre>
            {/each}
        </div>
    </div>
{/if}

{#if modsShown}
    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
    <div class="modal-overlay" on:click={() => modsShown = false} role="presentation">
        <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions a11y-no-noninteractive-element-interactions -->
        <div class="modal-card" on:click|stopPropagation role="dialog">
            <div class="modal-header">
                <span>моды</span>
                <button class="btn-link" on:click={() => modsShown = false}>✕</button>
            </div>
            <div class="modal-body">
                {#each modsList as mod}
                    <div class="mod-row">
                        <span class="mod-name">{mod.name}</span>
                        <div class="mod-actions">
                            <label class="switch">
                                <input type="checkbox" checked={mod.enabled} on:change={() => toggleMod(mod.id)} />
                                <span class="slider-switch"></span>
                            </label>
                        </div>
                    </div>
                {/each}
                {#each customModList as mod}
                    <div class="mod-row">
                        <span class="mod-name">{mod.filename}</span>
                        <div class="mod-actions">
                            <label class="switch">
                                <input type="checkbox" checked={mod.enabled} on:change={() => toggleCustomMod(mod.filename)} />
                                <span class="slider-switch"></span>
                            </label>
                            <button class="btn-icon btn-icon-sm" on:click={() => deleteCustomMod(mod.filename)} title="удалить">
                                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.4)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                    <polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                                </svg>
                            </button>
                        </div>
                    </div>
                {/each}
                {#if modsList.length === 0 && customModList.length === 0}
                    <div class="mods-empty">моды не выбраны</div>
                {/if}
            </div>
            <div class="modal-footer">
                <button class="btn-link" on:click={addCustomMod}>+ добавить .jar</button>
            </div>
        </div>
    </div>
{/if}

{#if settingsShown}
    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
    <div class="modal-overlay" on:click={() => settingsShown = false} role="presentation">
        <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions a11y-no-noninteractive-element-interactions -->
        <div class="modal-card" on:click|stopPropagation role="dialog">
            <div class="modal-header">
                <span>настройки</span>
                <button class="btn-link" on:click={() => settingsShown = false}>✕</button>
            </div>
            <div class="modal-body modal-settings">
                <div class="path-group">
                    <span class="path-label">путь к данным</span>
                    <div class="path-row">
                        <span class="path-value">{options.start.customDataPath || "по умолчанию"}</span>
                        <div class="path-buttons">
                            <button class="btn-icon" on:click={openGameDir} title="открыть папку">
                                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="rgba(255,255,255,0.4)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                                </svg>
                            </button>
                            <button class="btn-link" on:click={selectDataPath}>изменить</button>
                        </div>
                    </div>
                </div>

                <label class="switch-label">
                    <label class="switch">
                        <input type="checkbox" bind:checked={options.launcher.keepLauncherOpen} on:change={() => options.store()} />
                        <span class="slider-switch"></span>
                    </label>
                    <span>не закрывать лаунчер</span>
                </label>

                <div class="path-group">
                    <span class="path-label">jvm аргументы</span>
                    <input
                        type="text"
                        class="text-input jvm-input"
                        placeholder="-XX:+UseG1GC -Dfile.encoding=UTF-8"
                        value={(options.start.jvmArgs || []).join(" ")}
                        on:change={(e) => {
                            options.start.jvmArgs = e.target.value.trim().split(/\s+/).filter(Boolean);
                            options.store();
                        }}
                    />
                </div>
            </div>
        </div>
    </div>
{/if}

<style>
    #starCanvas {
        position: fixed;
        top: 0; left: 0;
        width: 100vw; height: 100vh;
        z-index: 1;
        opacity: 0.65;
        pointer-events: none;
    }

    .drag-zone {
        position: fixed;
        top: 0; left: 0;
        width: 100vw;
        height: 40px;
        z-index: 15;
    }

    .window-controls {
        position: absolute;
        top: 8px; right: 8px;
        z-index: 20;
        display: flex;
        gap: 4px;
    }

    .win-btn {
        width: 28px; height: 28px;
        display: flex; align-items: center; justify-content: center;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 6px;
        cursor: pointer;
        transition: background 0.2s;
    }

    .win-btn:hover {
        background: rgba(255, 255, 255, 0.10);
    }

    .win-btn-close:hover {
        background: rgba(255, 60, 40, 0.25);
        border-color: rgba(255, 60, 40, 0.3);
    }

    .win-btn-close:hover svg line {
        stroke: rgba(255, 255, 255, 0.8);
    }

    .hero-container {
        position: absolute;
        top: 50%; left: 50%;
        transform: translate(-50%, -50%);
        z-index: 5;
        text-align: center;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 24px;
        min-width: 340px;
    }

    .hero-title {
        font-size: 2.8rem;
        font-weight: 300;
        letter-spacing: -0.5px;
        color: rgba(255, 255, 255, 0.92);
        text-shadow: 0 0 35px rgba(255, 255, 255, 0.08);
    }

    .version-text {
        font-size: 1.4rem;
        font-weight: 200;
        color: rgba(255, 255, 255, 0.35);
    }

    .loading-text {
        color: rgba(255, 255, 255, 0.4);
        font-size: 0.9rem;
    }

    .card {
        background: rgba(255, 255, 255, 0.03);
        backdrop-filter: blur(16px);
        -webkit-backdrop-filter: blur(16px);
        border: 1px solid rgba(255, 255, 255, 0.06);
        border-radius: 10px;
        width: 100%;
        min-width: 300px;
        max-width: 400px;
    }

    .card-wide {
        min-width: 360px;
        max-width: 440px;
    }

    .card-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 16px 20px 0;
    }

    .user-info {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .user-name {
        font-size: 0.9rem;
        font-weight: 500;
    }

    .user-type {
        font-size: 0.7rem;
        color: rgba(255, 255, 255, 0.35);
        padding: 2px 6px;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 4px;
    }

    .card-content {
        padding: 20px;
        display: flex;
        flex-direction: column;
        gap: 16px;
    }

    .header-actions {
        display: flex;
        align-items: center;
        gap: 12px;
    }

    .avatar {
        width: 20px;
        height: 20px;
        border-radius: 4px;
        image-rendering: pixelated;
    }

    .split-buttons {
        display: flex;
        gap: 8px;
    }

    .split-buttons .btn {
        flex: 1;
    }

    .modal-settings {
        display: flex;
        flex-direction: column;
        gap: 16px;
    }

    .switch-label {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
        cursor: pointer;
    }

    .jvm-input {
        text-align: left;
        font-family: monospace;
        font-size: 0.75rem;
    }

    .account-dropdown-wrap {
        position: relative;
        z-index: 30;
    }

    .user-info-btn {
        cursor: pointer;
        padding: 4px 8px;
        margin: -4px -8px;
        border-radius: 6px;
        transition: background 0.2s;
    }

    .user-info-btn:hover {
        background: rgba(255, 255, 255, 0.05);
    }

    .dropdown-arrow {
        transition: transform 0.2s;
    }

    .dropdown-arrow-open {
        transform: rotate(180deg);
    }

    .dropdown-overlay {
        position: fixed;
        inset: 0;
        z-index: 25;
    }

    .account-dropdown {
        position: absolute;
        top: calc(100% + 6px);
        left: -8px;
        z-index: 30;
        min-width: 180px;
        background: #0c0c0c;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 8px;
        padding: 4px;
        display: flex;
        flex-direction: column;
        gap: 2px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
    }

    .account-add {
        border-top: 1px solid rgba(255, 255, 255, 0.05);
        border-radius: 0 0 6px 6px;
    }

    .account-add-text {
        font-size: 0.78rem;
        color: rgba(255, 255, 255, 0.45);
        padding: 2px 0;
        width: 100%;
        text-align: left;
    }

    .account-add:hover .account-add-text {
        color: rgba(255, 255, 255, 0.75);
    }

    .account-row {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 3px 6px;
        border-radius: 5px;
        cursor: pointer;
        transition: background 0.2s;
        border: 1px solid transparent;
    }

    .account-row:hover {
        background: rgba(255, 255, 255, 0.04);
    }

    .account-active {
        background: rgba(255, 255, 255, 0.05);
        border-color: rgba(255, 255, 255, 0.08);
    }

    .account-name {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.75);
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        text-align: left;
    }

    .account-type {
        font-size: 0.65rem;
        color: rgba(255, 255, 255, 0.3);
    }

    .account-remove {
        opacity: 0;
        transition: opacity 0.2s;
    }

    .account-row:hover .account-remove {
        opacity: 1;
    }

    .card-footer {
        padding: 10px 20px;
        font-size: 0.7rem;
        color: rgba(255, 255, 255, 0.28);
        border-top: 1px solid rgba(255, 255, 255, 0.04);
    }

    .input-group {
        width: 100%;
    }

    .text-input {
        width: 100%;
        padding: 12px 14px;
        font-size: 0.9rem;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 6px;
        color: rgba(255, 255, 255, 0.9);
        transition: border-color 0.2s;
        text-align: center;
        font-family: "Inter", sans-serif;
    }

    .text-input:focus {
        border-color: rgba(255, 255, 255, 0.2);
    }

    .text-input::placeholder {
        color: rgba(255, 255, 255, 0.25);
    }

    .btn {
        width: 100%;
        padding: 12px;
        font-size: 0.85rem;
        font-family: "Inter", sans-serif;
        font-weight: 400;
        border: none;
        border-radius: 6px;
        cursor: pointer;
        transition: background 0.2s, color 0.2s;
    }

    .btn-primary {
        background: rgba(255, 255, 255, 0.06);
        color: rgba(255, 255, 255, 0.85);
        border: 1px solid rgba(255, 255, 255, 0.08);
    }

    .btn-primary:hover:not(:disabled) {
        background: rgba(255, 255, 255, 0.1);
    }

    .btn-primary:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .btn-secondary {
        background: transparent;
        color: rgba(255, 255, 255, 0.5);
        border: 1px solid rgba(255, 255, 255, 0.06);
    }

    .btn-secondary:hover {
        background: rgba(255, 255, 255, 0.04);
        color: rgba(255, 255, 255, 0.8);
    }

    .btn-danger {
        background: rgba(255, 80, 60, 0.15);
        color: rgba(255, 80, 60, 0.8);
        border: 1px solid rgba(255, 80, 60, 0.15);
    }

    .btn-danger:hover {
        background: rgba(255, 80, 60, 0.25);
    }

    .btn-launch {
        padding: 14px;
        font-size: 0.95rem;
    }

    .btn-link {
        background: none;
        border: none;
        color: rgba(255, 255, 255, 0.35);
        font-size: 0.75rem;
        font-family: "Inter", sans-serif;
        cursor: pointer;
        transition: color 0.2s;
        padding: 0;
    }

    .btn-link:hover {
        color: rgba(255, 255, 255, 0.7);
    }

    .slider-group {
        width: 100%;
    }

    .slider-label {
        display: flex;
        justify-content: space-between;
        align-items: center;
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
        margin-bottom: 8px;
    }

    .slider-value {
        font-size: 1rem;
        font-weight: 500;
        color: rgba(255, 255, 255, 0.8);
    }

    .slider-container {
        width: 100%;
        padding: 4px 0;
    }

    .thick-slider {
        -webkit-appearance: none;
        appearance: none;
        width: 100%;
        height: 32px;
        background: rgba(255, 255, 255, 0.10);
        border-radius: 6px;
        border: 1px solid rgba(255, 255, 255, 0.10);
        cursor: pointer;
        outline: none;
        transition: border-color 0.2s;
    }

    .thick-slider:hover {
        border-color: rgba(255, 255, 255, 0.12);
    }

    .thick-slider::-webkit-slider-runnable-track {
        height: 32px;
        border-radius: 6px;
    }

    .thick-slider::-webkit-slider-thumb {
        -webkit-appearance: none;
        appearance: none;
        width: 24px;
        height: 44px;
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid rgba(255, 255, 255, 0.12);
        border-radius: 6px;
        cursor: pointer;
        transition: background 0.2s;
        margin-top: -6px;
    }

    .thick-slider::-webkit-slider-thumb:hover {
        background: rgba(255, 255, 255, 0.14);
    }

    .thick-slider::-moz-range-track {
        height: 32px;
        border-radius: 6px;
        background: rgba(255, 255, 255, 0.10);
        border: 1px solid rgba(255, 255, 255, 0.10);
    }

    .thick-slider::-moz-range-thumb {
        width: 24px;
        height: 44px;
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid rgba(255, 255, 255, 0.12);
        border-radius: 6px;
        cursor: pointer;
    }

    .path-group {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .path-label {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
    }

    .path-row {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: 8px;
    }

    .path-buttons {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .btn-icon {
        background: none;
        border: none;
        cursor: pointer;
        padding: 4px;
        display: flex;
        align-items: center;
        justify-content: center;
        border-radius: 4px;
        transition: background 0.2s;
    }

    .btn-icon:hover {
        background: rgba(255, 255, 255, 0.06);
    }

    .btn-icon:hover svg {
        stroke: rgba(255, 255, 255, 0.7);
    }

    .path-value {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.4);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex: 1;
        text-align: left;
    }

    .modal-overlay {
        position: fixed;
        inset: 0;
        z-index: 100;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        align-items: center;
        justify-content: center;
    }

    .modal-card {
        background: #0a0a0a;
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: 10px;
        width: 380px;
        max-height: 60vh;
        display: flex;
        flex-direction: column;
        backdrop-filter: blur(16px);
    }

    .modal-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 12px 16px;
        font-size: 0.85rem;
        color: rgba(255, 255, 255, 0.7);
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
    }

    .modal-body {
        flex: 1;
        overflow-y: auto;
        padding: 12px 16px;
    }

    .modal-footer {
        padding: 10px 16px;
        border-top: 1px solid rgba(255, 255, 255, 0.04);
    }

    .mods-empty {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.3);
        text-align: center;
        padding: 8px;
    }

    .mod-row {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 4px 0;
    }

    .mod-name {
        font-size: 0.85rem;
        color: rgba(255, 255, 255, 0.7);
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .mod-actions {
        display: flex;
        align-items: center;
        gap: 8px;
    }


    .btn-icon-sm {
        width: 24px;
        height: 24px;
    }

    .switch {
        position: relative;
        display: inline-block;
        width: 36px;
        height: 20px;
        cursor: pointer;
    }

    .switch input {
        opacity: 0;
        width: 0;
        height: 0;
    }

    .slider-switch {
        position: absolute;
        inset: 0;
        background: rgba(255, 255, 255, 0.10);
        border: 1px solid rgba(255, 255, 255, 0.10);
        border-radius: 6px;
        transition: background 0.2s, border-color 0.2s;
    }

    .slider-switch::before {
        content: "";
        position: absolute;
        width: 16px;
        height: 16px;
        left: 1px;
        bottom: 1px;
        background: rgba(255, 255, 255, 0.08);
        border: 1px solid rgba(255, 255, 255, 0.12);
        border-radius: 4px;
        transition: transform 0.2s, background 0.2s, border-color 0.2s;
    }

    .switch input:checked + .slider-switch {
        background: rgba(100, 180, 255, 0.18);
        border-color: rgba(100, 180, 255, 0.25);
    }

    .switch input:checked + .slider-switch::before {
        transform: translateX(16px);
        background: rgba(100, 180, 255, 0.25);
        border-color: rgba(100, 180, 255, 0.35);
    }

    .progress-section {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }

    .progress-text {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
    }

    .progress-bar {
        width: 100%;
        height: 4px;
        border: none;
        border-radius: 2px;
        background: rgba(255, 255, 255, 0.06);
    }

    .progress-bar::-webkit-progress-bar {
        background: rgba(255, 255, 255, 0.06);
        border-radius: 2px;
    }

    .progress-bar::-webkit-progress-value {
        background: rgba(255, 255, 255, 0.25);
        border-radius: 2px;
        transition: width 0.3s;
    }

    .running-buttons {
        display: flex;
        gap: 8px;
    }

    .running-buttons .btn {
        flex: 1;
    }

    .ms-code-text {
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
        line-height: 1.4;
    }

    .ms-code-text a {
        color: rgba(255, 255, 255, 0.7);
        text-decoration: underline;
        cursor: pointer;
    }

    .ms-code {
        font-size: 1.6rem;
        font-weight: 500;
        letter-spacing: 6px;
        color: rgba(255, 255, 255, 0.9);
        padding: 12px;
        background: rgba(255, 255, 255, 0.03);
        border-radius: 6px;
        border: 1px solid rgba(255, 255, 255, 0.06);
        font-family: monospace;
        cursor: pointer;
        transition: background 0.2s;
    }

    .ms-code:hover {
        background: rgba(255, 255, 255, 0.06);
    }

    .log-overlay {
        position: absolute;
        bottom: 0; left: 0; right: 0;
        z-index: 20;
        background: rgba(5, 5, 5, 0.92);
        backdrop-filter: blur(12px);
        border-top: 1px solid rgba(255, 255, 255, 0.06);
        max-height: 40vh;
        display: flex;
        flex-direction: column;
    }

    .log-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 10px 16px;
        font-size: 0.8rem;
        color: rgba(255, 255, 255, 0.5);
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
    }

    .log-content {
        overflow-y: auto;
        padding: 8px 16px;
        flex: 1;
    }

    .log-line {
        font-family: monospace;
        font-size: 0.75rem;
        color: rgba(255, 255, 255, 0.6);
        user-select: text;
        white-space: pre-wrap;
        word-break: break-all;
    }
</style>
