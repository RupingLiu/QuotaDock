<script lang="ts">
  import { onMount } from "svelte";
  import { tauriApi } from "$lib/api/tauri";
  import type {
    AppState,
    DeepSeekBalance,
    ProviderId,
    ProviderState,
    ProviderSnapshot,
    QuotaReading,
  } from "$lib/types/usage";
  import {
    capturedAtToEpochMs,
    formatBalance,
    formatPercent,
    formatReset,
    providerErrorLabel,
  } from "$lib/utils/format";

  export let appState: AppState | null;
  export let loading = false;
  export let refreshing = false;
  export let errorMessage: string | null = null;
  export let noticeMessage: string | null = null;
  export let providerAnnouncement: string | null = null;

  type TauriWindowHandle = { startDragging: () => Promise<void> };
  type DisplayState = "fresh" | "busy" | "error" | "stale" | "warning" | "empty";

  const ROTATION_INTERVAL_MS = 8_000;
  const CLOCK_TICK_MS = 30_000;
  const STALE_AFTER_MS = 10 * 60_000;
  const PROVIDER_ORDER: ProviderId[] = ["codex", "deepseek", "kimi"];
  const emptyReading: QuotaReading = {
    remainingPercent: null,
    resetAt: null,
    resetCountdownSeconds: null,
  };

  let tauriWindow: TauriWindowHandle | null = null;
  let tauriWindowLoad: Promise<TauriWindowHandle | null> | null = null;
  let lastDragStartAt = 0;
  let nowMs = Date.now();
  let menuErrorMessage: string | null = null;
  let activeProviderId: ProviderId = "codex";
  let rotationTimer: number | null = null;
  let mounted = false;
  let pointerPaused = false;
  let focusPaused = false;
  let pageHidden = false;
  let reducedMotion = false;

  $: selectedProviderIds = normalizedSelection(appState);
  $: if (!selectedProviderIds.includes(activeProviderId)) {
    activeProviderId = selectedProviderIds[0] ?? "codex";
  }
  $: providerState = appState?.providers[activeProviderId] ?? null;
  $: snapshot = matchingSnapshot(providerState, activeProviderId);
  $: weekly = snapshot?.provider === "codex" ? snapshot.data.weekly : emptyReading;
  $: weeklyIsLow =
    typeof weekly.remainingPercent === "number" && weekly.remainingPercent <= 20;
  $: capturedAt = snapshot?.data.capturedAt ?? null;
  $: displayState = resolveDisplayState(providerState, capturedAt, nowMs, loading, refreshing, menuErrorMessage);
  $: displayStateLabel = stateLabel(displayState, Boolean(snapshot));
  $: freshnessText = menuErrorMessage
    ? "菜单失败"
    : compactFreshness(displayState, capturedAt, nowMs);
  $: primary = primaryMetric(snapshot);
  $: secondary = secondaryMetric(snapshot, weekly, nowMs);
  $: providerLabel = labelForProvider(activeProviderId);
  $: legacyActiveError = activeProviderId === "codex" ? errorMessage : null;
  $: legacyAnnouncement = errorMessage ?? menuErrorMessage ?? noticeMessage ?? "";
  $: providerLiveText =
    providerAnnouncement === legacyAnnouncement ? "" : (providerAnnouncement ?? "");
  $: statusText =
    providerErrorLabel(providerState?.errorCategory ?? null) ??
    legacyActiveError ??
    menuErrorMessage ??
    noticeMessage ??
    appState?.statusMessage ??
    displayStateLabel;
  $: titleText = `${providerLabel} ${primary}${secondary ? ` · ${secondary}` : ""}；${displayStateLabel}${statusText ? `；${statusText}` : ""}`;
  $: rotationKey = `${selectedProviderIds.join("|")}:${activeProviderId}`;
  $: scheduleForKey(rotationKey);

  onMount(() => {
    mounted = true;
    void preloadTauriWindow();
    pageHidden = document.visibilityState === "hidden";
    const media = typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)")
      : null;
    reducedMotion = media?.matches ?? false;
    const onMotionChange = (event: MediaQueryListEvent) => {
      reducedMotion = event.matches;
      restartRotation();
    };
    const onVisibility = () => {
      pageHidden = document.visibilityState === "hidden";
      restartRotation();
    };
    if (media?.addEventListener) media.addEventListener("change", onMotionChange);
    else media?.addListener?.(onMotionChange);
    document.addEventListener("visibilitychange", onVisibility);
    const clock = window.setInterval(() => (nowMs = Date.now()), CLOCK_TICK_MS);
    restartRotation();
    return () => {
      mounted = false;
      clearRotation();
      window.clearInterval(clock);
      if (media?.removeEventListener) media.removeEventListener("change", onMotionChange);
      else media?.removeListener?.(onMotionChange);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  });

  function normalizedSelection(state: AppState | null): ProviderId[] {
    if (!state) return ["codex"];
    const requested = new Set(state.settings.floatingProviderIds);
    const selected = PROVIDER_ORDER.filter(
      (id) => requested.has(id) && state.providers[id].configured,
    );
    return selected.length ? selected : ["codex"];
  }

  function matchingSnapshot(state: ProviderState | null, provider: ProviderId): ProviderSnapshot | null {
    return state?.latestSnapshot?.provider === provider ? state.latestSnapshot : null;
  }

  function clearRotation(): void {
    if (rotationTimer !== null) window.clearTimeout(rotationTimer);
    rotationTimer = null;
  }

  function scheduleForKey(_key: string): void {
    if (mounted) restartRotation();
  }

  function restartRotation(): void {
    clearRotation();
    if (
      !mounted ||
      selectedProviderIds.length < 2 ||
      pointerPaused ||
      focusPaused ||
      pageHidden ||
      reducedMotion
    ) return;
    rotationTimer = window.setTimeout(() => nextProvider(), ROTATION_INTERVAL_MS);
  }

  function nextProvider(): void {
    const current = selectedProviderIds.indexOf(activeProviderId);
    activeProviderId = selectedProviderIds[(current + 1) % selectedProviderIds.length] ?? "codex";
  }

  function onPointerEnter(): void {
    pointerPaused = true;
    clearRotation();
    void preloadTauriWindow();
  }

  function onPointerLeave(): void {
    pointerPaused = false;
    restartRotation();
  }

  function onFocusIn(): void {
    focusPaused = true;
    clearRotation();
  }

  function onFocusOut(event: FocusEvent): void {
    const section = event.currentTarget as HTMLElement;
    if (event.relatedTarget instanceof Node && section.contains(event.relatedTarget)) return;
    focusPaused = false;
    restartRotation();
  }

  function primaryMetric(value: ProviderSnapshot | null): string {
    if (!value) return "--";
    if (value.provider === "codex") return formatPercent(value.data.weekly.remainingPercent);
    if (value.provider === "deepseek") {
      const balance = preferredDeepSeekBalance(value.data.balances);
      return formatBalance(balance?.toppedUpBalance, balance?.currency ?? "CNY");
    }
    return formatBalance(value.data.availableBalance, value.data.currency);
  }

  function secondaryMetric(
    value: ProviderSnapshot | null,
    reading: QuotaReading,
    currentTimeMs: number,
  ): string {
    if (!value) return "";
    if (value.provider === "codex") {
      return formatReset(reading, { capturedAt: value.data.capturedAt, nowMs: currentTimeMs })
        .replace(/^(\d{1,2})月(\d{1,2})日\s+/, "$1/$2 ")
        .replaceAll("小时", "h")
        .replaceAll("分钟", "m")
        .replaceAll("天", "d")
        .replace(/后$/, "");
    }
    if (value.provider === "deepseek" && !value.data.isAvailable) return "不可用";
    return value.provider === "deepseek" ? "充值余额" : "可用余额";
  }

  function preferredDeepSeekBalance(balances: DeepSeekBalance[]): DeepSeekBalance | null {
    return balances.find((balance) => balance.currency === "CNY") ?? balances[0] ?? null;
  }

  function labelForProvider(provider: ProviderId): string {
    if (provider === "deepseek") return "DeepSeek";
    if (provider === "kimi") return "Kimi";
    return "Codex";
  }

  function resolveDisplayState(
    state: ProviderState | null,
    capturedAtValue: string | null,
    currentTimeMs: number,
    isLoading: boolean,
    isRefreshing: boolean,
    menuError: string | null,
  ): DisplayState {
    if (menuError) return "error";
    if (isLoading || state?.health === "refreshing" || Boolean(isRefreshing && state?.configured)) return "busy";
    if (state?.health === "error") return "error";
    if (state?.health === "stale") return "stale";
    const capturedAtMs = capturedAtToEpochMs(capturedAtValue);
    if (
      capturedAtMs !== null &&
      (currentTimeMs - capturedAtMs > STALE_AFTER_MS || currentTimeMs - capturedAtMs < -5 * 60_000)
    ) return "stale";
    if (state?.health === "not-configured") return "empty";
    if (appState?.storageStatus === "recovered" || appState?.storageStatus === "unsupported-version") return "warning";
    return state?.latestSnapshot ? "fresh" : "empty";
  }

  function stateLabel(state: DisplayState, hasSnapshot: boolean): string {
    switch (state) {
      case "fresh": return "数据新鲜";
      case "busy": return "正在读取额度";
      case "error": return hasSnapshot ? "刷新失败，当前显示上次成功数据" : "刷新失败，尚无可显示的数据";
      case "stale": return "数据已陈旧";
      case "warning": return "数据不完整或存储已恢复";
      default: return "等待首次额度更新";
    }
  }

  function compactFreshness(
    state: DisplayState,
    value: string | null,
    currentTimeMs: number,
  ): string {
    if (state === "busy") return "读取";
    if (state === "error") return "失败";
    if (state === "stale") return "陈旧";
    if (state === "warning") return "注意";
    if (state === "empty") return "等待";
    const epoch = capturedAtToEpochMs(value);
    if (epoch === null) return "--";
    const age = Math.max(0, currentTimeMs - epoch);
    if (age < 60_000) return "刚刚";
    if (age < 3_600_000) return `${Math.floor(age / 60_000)}m`;
    if (age < 86_400_000) return `${Math.floor(age / 3_600_000)}h`;
    return `${Math.floor(age / 86_400_000)}d`;
  }

  function startWindowDrag(event: PointerEvent | MouseEvent): void {
    if (event.button !== 0 || !hasTauriRuntime()) return;
    const now = performance.now();
    if (now - lastDragStartAt < 80) return;
    lastDragStartAt = now;
    if (tauriWindow) {
      void tauriWindow.startDragging().catch(() => {});
      return;
    }
    void preloadTauriWindow().then((handle) => void handle?.startDragging().catch(() => {}));
  }

  function preloadTauriWindow(): Promise<TauriWindowHandle | null> {
    if (!hasTauriRuntime()) return Promise.resolve(null);
    if (tauriWindow) return Promise.resolve(tauriWindow);
    if (tauriWindowLoad) return tauriWindowLoad;
    tauriWindowLoad = import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => (tauriWindow = getCurrentWindow()))
      .catch(() => {
        tauriWindowLoad = null;
        return null;
      });
    return tauriWindowLoad;
  }

  function showContextMenu(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    openContextMenu(event.clientX, event.clientY);
  }

  function showContextMenuFromButton(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const bounds = (event.currentTarget as HTMLButtonElement).getBoundingClientRect();
    openContextMenu(bounds.right, bounds.bottom);
  }

  function openContextMenu(x: number, y: number): void {
    menuErrorMessage = null;
    void tauriApi.showDashboardContextMenu(x, y).catch(() => {
      menuErrorMessage = "菜单打开失败，请使用系统托盘菜单。";
    });
  }

  function hasTauriRuntime(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }
</script>

<main class="float-shell" on:contextmenu={showContextMenu}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    class="mini-status"
    data-state={displayState}
    data-provider={activeProviderId}
    aria-busy={displayState === "busy"}
    aria-label="QuotaDock 状态栏"
    title={titleText}
    data-tauri-drag-region="deep"
    on:pointerenter={onPointerEnter}
    on:pointerleave={onPointerLeave}
    on:focusin={onFocusIn}
    on:focusout={onFocusOut}
    on:pointerdown={startWindowDrag}
    on:mousedown={startWindowDrag}
  >
    <span class="state-dot" aria-hidden="true"></span>
    <div class:low={activeProviderId === "codex" && weeklyIsLow} class="quota-row">
      <button
        class="provider-button"
        type="button"
        aria-label={`当前显示 ${providerLabel}，切换到下一项`}
        title={selectedProviderIds.length > 1 ? "切换到下一项" : "当前仅显示此项"}
        disabled={selectedProviderIds.length < 2}
        on:click={nextProvider}
        on:pointerdown|stopPropagation
        on:mousedown|stopPropagation
      >{providerLabel}</button>
      <span class="quota-metrics">
        <strong data-testid="provider-value">{primary}</strong>
        {#if activeProviderId === "codex" && weeklyIsLow}
          <span class="low-flag" aria-hidden="true">!</span><span class="sr-only">低额度</span>
        {/if}
        {#if secondary}<span class="secondary" aria-hidden="true">{secondary}</span>{/if}
      </span>
    </div>
    <span class="freshness" aria-hidden="true">{freshnessText}</span>
    <button
      class="menu-button"
      type="button"
      aria-label="打开 QuotaDock 菜单"
      aria-haspopup="menu"
      title="打开菜单"
      on:click={showContextMenuFromButton}
      on:pointerdown|stopPropagation
      on:mousedown|stopPropagation
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <circle cx="3" cy="8" r="1.25"></circle><circle cx="8" cy="8" r="1.25"></circle><circle cx="13" cy="8" r="1.25"></circle>
      </svg>
    </button>
    <span class="sr-only">{providerLabel}，{primary}，{secondary}，{displayStateLabel}</span>
    <span class="sr-only" data-testid="legacy-announcement" role="status" aria-live="polite">{legacyAnnouncement}</span>
    <span class="sr-only" data-testid="provider-announcement" role="status" aria-live="polite">{providerLiveText}</span>
  </section>
</main>

<style>
  :global(html), :global(body), :global(body > div) { width: 100%; height: 100%; overflow: hidden; background: transparent; }
  :global(body) { margin: 0; color: #17242b; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", "Microsoft YaHei UI", sans-serif; user-select: none; -webkit-user-select: none; }
  :global(*) { box-sizing: border-box; }
  :global(button) { font: inherit; letter-spacing: 0; }
  .float-shell { width: 100%; height: 100%; display: grid; place-items: stretch; overflow: hidden; background: transparent; }
  .mini-status { width: 100%; height: 100%; min-width: 0; display: grid; grid-template-columns: 8px minmax(0, 1fr) auto 24px; align-items: center; column-gap: 7px; padding: 3px 5px 3px 8px; border: 1px solid rgba(100,116,139,.26); border-radius: 8px; background: rgba(255,255,255,.985); box-shadow: 0 4px 12px rgba(15,23,42,.13), 0 1px 2px rgba(15,23,42,.1), inset 0 1px 0 rgba(255,255,255,.96); backdrop-filter: blur(12px) saturate(1.08); cursor: grab; transition: border-color 180ms ease, box-shadow 180ms ease, background-color 180ms ease; }
  .mini-status:active { cursor: grabbing; }
  .mini-status[data-state="error"] { border-color: rgba(180,35,24,.55); box-shadow: 0 4px 12px rgba(127,29,29,.14), 0 1px 2px rgba(127,29,29,.1); }
  .mini-status[data-state="stale"], .mini-status[data-state="warning"] { border-color: rgba(146,64,14,.5); }
  .state-dot { width: 7px; height: 7px; border-radius: 50%; background: #0f766e; box-shadow: 0 0 0 2px rgba(15,118,110,.13); }
  .mini-status[data-state="busy"] .state-dot { background: #1d4ed8; box-shadow: 0 0 0 2px rgba(29,78,216,.14); animation: breathe 1.2s ease-in-out infinite; }
  .mini-status[data-state="error"] .state-dot { background: #b42318; box-shadow: 0 0 0 2px rgba(180,35,24,.14); }
  .mini-status[data-state="stale"] .state-dot, .mini-status[data-state="warning"] .state-dot { background: #92400e; box-shadow: 0 0 0 2px rgba(146,64,14,.14); }
  .mini-status[data-state="empty"] .state-dot { background: #64748b; box-shadow: 0 0 0 2px rgba(100,116,139,.14); }
  .quota-row { min-width: 0; height: 100%; display: flex; align-items: center; gap: 5px; }
  .provider-button { min-width: 0; max-width: 72px; flex: 0 1 auto; padding: 2px 3px; overflow: hidden; border: 0; border-radius: 4px; color: #475569; background: transparent; font-size: .7rem; font-weight: 650; line-height: 1; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .provider-button:hover:not(:disabled) { color: #0f766e; background: #e8f3f1; }
  .provider-button:focus-visible, .menu-button:focus-visible { outline: 2px solid #2563eb; outline-offset: 1px; }
  .provider-button:disabled { cursor: default; }
  .quota-metrics { min-width: 0; flex: 1 1 auto; display: flex; align-items: baseline; gap: 5px; }
  strong { min-width: 0; flex: 0 1 auto; overflow: hidden; color: #172033; font-family: "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", sans-serif; font-size: .88rem; font-variant-numeric: tabular-nums; font-weight: 680; line-height: 1; text-overflow: ellipsis; white-space: nowrap; }
  .secondary { min-width: 0; max-width: 66px; overflow: hidden; color: #475569; font-size: .67rem; font-weight: 500; line-height: 1; text-overflow: ellipsis; white-space: nowrap; }
  .low-flag { flex: 0 0 auto; margin-left: -3px; color: #9a3412; font-size: .68rem; font-weight: 800; line-height: 1; }
  .quota-row.low strong { color: #9a3412; }
  .freshness { min-width: 30px; overflow: hidden; color: #475569; font-size: .66rem; font-variant-numeric: tabular-nums; font-weight: 600; line-height: 1; text-align: right; text-overflow: ellipsis; white-space: nowrap; }
  .mini-status[data-state="error"] .freshness { color: #b42318; }
  .mini-status[data-state="stale"] .freshness, .mini-status[data-state="warning"] .freshness { color: #92400e; }
  .menu-button { width: 24px; height: 24px; display: grid; place-items: center; padding: 0; border: 0; border-radius: 6px; color: #475569; background: transparent; cursor: pointer; transition: color 160ms ease, background-color 160ms ease; }
  .menu-button:hover { color: #172033; background: #eef2f6; }
  .menu-button svg { width: 15px; height: 15px; fill: currentColor; }
  @keyframes breathe { 0%, 100% { opacity: .55; } 50% { opacity: 1; } }
  @media (prefers-color-scheme: dark) {
    :global(body) { color: #e5edf5; }
    .mini-status { border-color: rgba(148,163,184,.34); background: rgba(15,23,42,.97); box-shadow: 0 5px 16px rgba(0,0,0,.34), 0 1px 2px rgba(0,0,0,.28), inset 0 1px 0 rgba(255,255,255,.07); }
    .provider-button, .secondary, .freshness, .menu-button { color: #c2cedb; }
    .provider-button:hover:not(:disabled), .menu-button:hover { color: #fff; background: rgba(148,163,184,.18); }
    strong { color: #f8fafc; }
    .quota-row.low strong, .low-flag { color: #fdba74; }
    .mini-status[data-state="error"] .freshness { color: #fda29b; }
    .mini-status[data-state="stale"] .freshness, .mini-status[data-state="warning"] .freshness { color: #fdba74; }
  }
  @media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } }
  @media (max-width: 319px) { .mini-status { column-gap: 4px; } .provider-button { max-width: 64px; } }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }
</style>
