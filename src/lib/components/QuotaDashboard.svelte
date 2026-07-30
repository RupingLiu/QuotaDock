<script lang="ts">
  import { onMount } from "svelte";
  import { tauriApi } from "$lib/api/tauri";
  import { isSnapshotStale } from "$lib/state/usageState.svelte";
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import {
    capturedAtToEpochMs,
    formatPercent,
    formatReset,
  } from "$lib/utils/format";

  export let appState: AppState | null;
  export let loading = false;
  export let refreshing = false;
  export let errorMessage: string | null = null;
  export let noticeMessage: string | null = null;

  type QuotaRow = {
    id: "five" | "week";
    label: string;
    ariaLabel: string;
    valueTestId: string;
    resetTestId: string;
    remainingPercent: number | null;
    resetText: string;
    resetAriaText: string;
    isLow: boolean;
  };
  type TauriWindowHandle = {
    startDragging: () => Promise<void>;
  };
  type DisplayState =
    | "fresh"
    | "busy"
    | "error"
    | "stale"
    | "warning"
    | "empty";

  const DASHBOARD_STALE_AFTER_MS = 10 * 60 * 1000;
  const CLOCK_TICK_MS = 30 * 1000;
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

  $: snapshot = appState?.latestSnapshot ?? null;
  $: fiveHour = snapshot?.fiveHour ?? emptyReading;
  $: weekly = snapshot?.weekly ?? emptyReading;
  $: quotaRows = [
    {
      id: "five",
      label: "5 小时",
      ariaLabel: "5小时额度",
      valueTestId: "five-hour-value",
      resetTestId: "five-hour-reset",
      remainingPercent: fiveHour.remainingPercent,
      resetText: compactFloatingReset(fiveHour, snapshot?.capturedAt, nowMs),
      resetAriaText: accessibleReset(fiveHour, snapshot?.capturedAt, nowMs),
      isLow:
        typeof fiveHour.remainingPercent === "number" &&
        fiveHour.remainingPercent <= 20,
    },
    {
      id: "week",
      label: "1 周",
      ariaLabel: "1周额度",
      valueTestId: "weekly-value",
      resetTestId: "weekly-reset",
      remainingPercent: weekly.remainingPercent,
      resetText: compactFloatingReset(weekly, snapshot?.capturedAt, nowMs),
      resetAriaText: accessibleReset(weekly, snapshot?.capturedAt, nowMs),
      isLow:
        typeof weekly.remainingPercent === "number" &&
        weekly.remainingPercent <= 20,
    },
  ] satisfies QuotaRow[];
  $: statusText =
    errorMessage ??
    menuErrorMessage ??
    (refreshing ? "读取中..." : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    null;
  $: busy = loading || refreshing;
  $: stale = snapshot
    ? isSnapshotStale(snapshot, nowMs, DASHBOARD_STALE_AFTER_MS)
    : false;
  $: importantWarning = Boolean(
    snapshot?.warnings.some((warning) => warning.code !== "unknown-lines"),
  );
  $: storageWarning =
    appState?.storageStatus === "recovered" ||
    appState?.storageStatus === "unsupported-version";
  $: displayState = resolveDisplayState(
    busy,
    Boolean(errorMessage),
    Boolean(snapshot),
    stale,
    importantWarning || storageWarning || Boolean(menuErrorMessage),
  );
  $: displayStateLabel =
    menuErrorMessage && !errorMessage
      ? "菜单打开失败，请使用系统托盘菜单"
      : stateLabel(displayState, Boolean(snapshot));
  $: freshnessText =
    menuErrorMessage && !errorMessage
      ? "菜单失败"
      : compactFreshness(displayState, snapshot?.capturedAt, nowMs);
  $: liveStatusText = [
    displayStateLabel,
    displayState === "fresh"
      ? accessibleFreshness(snapshot?.capturedAt, nowMs)
      : null,
    statusText && statusText !== displayStateLabel ? statusText : null,
  ]
    .filter(Boolean)
    .join("，");
  $: titleText = `5小时 ${formatPercent(fiveHour.remainingPercent)} 刷新 ${formatReset(fiveHour, { capturedAt: snapshot?.capturedAt, nowMs })}；1周 ${formatPercent(weekly.remainingPercent)} 刷新 ${formatReset(weekly, { capturedAt: snapshot?.capturedAt, nowMs })}；${displayStateLabel}${statusText ? `；${statusText}` : ""}`;

  onMount(() => {
    void preloadTauriWindow();
    const clock = window.setInterval(() => {
      nowMs = Date.now();
    }, CLOCK_TICK_MS);

    return () => {
      window.clearInterval(clock);
    };
  });

  function startWindowDrag(event: PointerEvent | MouseEvent): void {
    if (event.button !== 0 || !hasTauriRuntime()) {
      return;
    }

    const now = performance.now();
    if (now - lastDragStartAt < 80) {
      return;
    }
    lastDragStartAt = now;

    if (tauriWindow) {
      void tauriWindow.startDragging().catch(() => {});
      return;
    }

    void preloadTauriWindow().then((windowHandle) => {
      void windowHandle?.startDragging().catch(() => {});
    });
  }

  function preloadTauriWindow(): Promise<TauriWindowHandle | null> {
    if (!hasTauriRuntime()) {
      return Promise.resolve(null);
    }
    if (tauriWindow) {
      return Promise.resolve(tauriWindow);
    }
    if (tauriWindowLoad) {
      return tauriWindowLoad;
    }

    tauriWindowLoad = import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) => {
        tauriWindow = getCurrentWindow();
        return tauriWindow;
      })
      .catch(() => {
        tauriWindowLoad = null;
        return null;
      });
    return tauriWindowLoad;
  }

  function primeWindowDrag(): void {
    void preloadTauriWindow();
  }

  function showContextMenu(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    openContextMenu(event.clientX, event.clientY);
  }

  function showContextMenuFromButton(event: MouseEvent): void {
    event.preventDefault();
    event.stopPropagation();
    const button = event.currentTarget as HTMLButtonElement;
    const bounds = button.getBoundingClientRect();
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

  function compactFloatingReset(
    reading: QuotaReading,
    capturedAt: string | null | undefined,
    currentTimeMs: number,
  ): string {
    return formatReset(reading, { capturedAt, nowMs: currentTimeMs })
      .replace(/^(\d{1,2})月(\d{1,2})日\s+/, "$1/$2 ")
      .replaceAll("小时", "h")
      .replaceAll("分钟", "m")
      .replaceAll("天", "d")
      .replace(/后$/, "");
  }

  function accessibleReset(
    reading: QuotaReading,
    capturedAt: string | null | undefined,
    currentTimeMs: number,
  ): string {
    const reset = formatReset(reading, { capturedAt, nowMs: currentTimeMs });
    return reset === "--" ? "重置时间未知" : `重置 ${reset}`;
  }

  function resolveDisplayState(
    isBusy: boolean,
    hasError: boolean,
    hasSnapshot: boolean,
    isStale: boolean,
    hasWarning: boolean,
  ): DisplayState {
    if (hasError) return "error";
    if (isBusy) return "busy";
    if (isStale) return "stale";
    if (hasWarning) return "warning";
    if (!hasSnapshot) return "empty";
    return "fresh";
  }

  function stateLabel(state: DisplayState, hasSnapshot: boolean): string {
    switch (state) {
      case "fresh":
        return "数据新鲜";
      case "busy":
        return "正在读取额度";
      case "error":
        return hasSnapshot
          ? "刷新失败，当前显示上次成功数据"
          : "刷新失败，尚无可显示的额度数据";
      case "stale":
        return "数据已陈旧";
      case "warning":
        return "数据不完整或存储已恢复";
      case "empty":
        return "等待首次额度更新";
    }
  }

  function compactFreshness(
    state: DisplayState,
    capturedAt: string | null | undefined,
    currentTimeMs: number,
  ): string {
    if (state === "busy") return "读取";
    if (state === "error") return "失败";
    if (state === "stale") return "陈旧";
    if (state === "warning") return "注意";
    if (state === "empty") return "等待";

    const capturedAtMs = capturedAtToEpochMs(capturedAt);
    if (capturedAtMs === null) return "--";
    const ageMs = Math.max(0, currentTimeMs - capturedAtMs);
    if (ageMs < 60_000) return "刚刚";
    if (ageMs < 60 * 60_000) return `${Math.floor(ageMs / 60_000)}m`;
    if (ageMs < 24 * 60 * 60_000) {
      return `${Math.floor(ageMs / (60 * 60_000))}h`;
    }
    return `${Math.floor(ageMs / (24 * 60 * 60_000))}d`;
  }

  function accessibleFreshness(
    capturedAt: string | null | undefined,
    currentTimeMs: number,
  ): string {
    const capturedAtMs = capturedAtToEpochMs(capturedAt);
    if (capturedAtMs === null) return "更新时间未知";
    const ageMs = Math.max(0, currentTimeMs - capturedAtMs);
    if (ageMs < 60_000) return "刚刚更新";
    if (ageMs < 60 * 60_000) {
      return `${Math.floor(ageMs / 60_000)} 分钟前更新`;
    }
    if (ageMs < 24 * 60 * 60_000) {
      return `${Math.floor(ageMs / (60 * 60_000))} 小时前更新`;
    }
    return `${Math.floor(ageMs / (24 * 60 * 60_000))} 天前更新`;
  }
</script>

<main class="float-shell" on:contextmenu={showContextMenu}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    class="mini-status"
    data-state={displayState}
    aria-busy={busy}
    aria-label="QuotaDock 状态栏"
    title={titleText}
    data-tauri-drag-region="deep"
    on:pointerenter={primeWindowDrag}
    on:pointerdown={startWindowDrag}
    on:mousedown={startWindowDrag}
  >
    <span class="state-dot" aria-hidden="true"></span>
    {#each quotaRows as row (row.id)}
      <div
        class:low={row.isLow}
        class="quota-row"
      >
        <span class="sr-only">{row.ariaLabel}</span>
        <span class="quota-label" aria-hidden="true">{row.label}</span>
        <span class="quota-metrics">
          <strong data-testid={row.valueTestId}>
            {formatPercent(row.remainingPercent)}
          </strong>
          {#if row.isLow}
            <span class="low-flag" aria-hidden="true">!</span>
            <span class="sr-only">低额度</span>
          {/if}
          <span
            class="reset-time"
            data-testid={row.resetTestId}
            aria-hidden="true"
          >
            {row.resetText}
          </span>
          <span class="sr-only">{row.resetAriaText}</span>
        </span>
      </div>
    {/each}
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
        <circle cx="3" cy="8" r="1.25"></circle>
        <circle cx="8" cy="8" r="1.25"></circle>
        <circle cx="13" cy="8" r="1.25"></circle>
      </svg>
    </button>
    <span class="sr-only" role="status" aria-live="polite">
      {liveStatusText}
    </span>
  </section>
</main>

<style>
  :global(html) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  :global(body) {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    color: #17242b;
    background: transparent;
    font-family:
      -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI",
      "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
    user-select: none;
    -webkit-user-select: none;
  }

  :global(body > div) {
    height: 100%;
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(button) {
    letter-spacing: 0;
    font: inherit;
  }

  .float-shell {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: stretch;
    padding: 0;
    overflow: hidden;
    background: transparent;
  }

  .mini-status {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-columns: 8px minmax(0, 1fr) minmax(0, 1fr) auto 24px;
    align-items: center;
    column-gap: 7px;
    padding: 3px 5px 3px 8px;
    border: 1px solid rgba(100, 116, 139, 0.26);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.985);
    box-shadow:
      0 4px 12px rgba(15, 23, 42, 0.13),
      0 1px 2px rgba(15, 23, 42, 0.1),
      inset 0 1px 0 rgba(255, 255, 255, 0.96);
    backdrop-filter: blur(12px) saturate(1.08);
    cursor: grab;
    transition:
      border-color 180ms ease,
      box-shadow 180ms ease,
      background-color 180ms ease;
  }

  .mini-status:active {
    cursor: grabbing;
  }

  .mini-status[data-state="error"] {
    border-color: rgba(180, 35, 24, 0.55);
    box-shadow:
      0 4px 12px rgba(127, 29, 29, 0.14),
      0 1px 2px rgba(127, 29, 29, 0.1);
  }

  .mini-status[data-state="stale"],
  .mini-status[data-state="warning"] {
    border-color: rgba(146, 64, 14, 0.5);
  }

  .state-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #0f766e;
    box-shadow: 0 0 0 2px rgba(15, 118, 110, 0.13);
  }

  .mini-status[data-state="busy"] .state-dot {
    background: #1d4ed8;
    box-shadow: 0 0 0 2px rgba(29, 78, 216, 0.14);
    animation: breathe 1.2s ease-in-out infinite;
  }

  .mini-status[data-state="error"] .state-dot {
    background: #b42318;
    box-shadow: 0 0 0 2px rgba(180, 35, 24, 0.14);
  }

  .mini-status[data-state="stale"] .state-dot,
  .mini-status[data-state="warning"] .state-dot {
    background: #92400e;
    box-shadow: 0 0 0 2px rgba(146, 64, 14, 0.14);
  }

  .mini-status[data-state="empty"] .state-dot {
    background: #64748b;
    box-shadow: 0 0 0 2px rgba(100, 116, 139, 0.14);
  }

  .quota-row {
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .quota-row + .quota-row {
    border-left: 1px solid rgba(100, 116, 139, 0.24);
    padding-left: 8px;
  }

  .quota-label {
    min-width: 0;
    flex: 0 0 auto;
    overflow: hidden;
    color: #475569;
    font-size: 0.72rem;
    font-weight: 600;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-transform: uppercase;
  }

  .quota-metrics {
    min-width: 0;
    flex: 1 1 auto;
    display: flex;
    align-items: baseline;
    gap: 5px;
  }

  strong {
    flex: 0 0 auto;
    color: #172033;
    font-family:
      "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",
      "Microsoft YaHei", sans-serif;
    font-size: 0.88rem;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    font-weight: 680;
    line-height: 1;
  }

  .low-flag {
    flex: 0 0 auto;
    margin-left: -3px;
    color: #9a3412;
    font-size: 0.68rem;
    font-weight: 800;
    line-height: 1;
  }

  .reset-time {
    min-width: 0;
    max-width: 62px;
    overflow: hidden;
    color: #475569;
    font-family:
      "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",
      "Microsoft YaHei", sans-serif;
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    font-weight: 500;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quota-row.low strong {
    color: #9a3412;
  }

  .freshness {
    min-width: 30px;
    overflow: hidden;
    color: #475569;
    font-size: 0.66rem;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    font-weight: 600;
    line-height: 1;
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mini-status[data-state="error"] .freshness {
    color: #b42318;
  }

  .mini-status[data-state="stale"] .freshness,
  .mini-status[data-state="warning"] .freshness {
    color: #92400e;
  }

  .menu-button {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 6px;
    color: #475569;
    background: transparent;
    cursor: pointer;
    transition:
      color 160ms ease,
      background-color 160ms ease;
  }

  .menu-button:hover {
    color: #172033;
    background: #eef2f6;
  }

  .menu-button:focus-visible {
    outline: 2px solid #2563eb;
    outline-offset: 1px;
  }

  .menu-button svg {
    width: 15px;
    height: 15px;
    fill: currentColor;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 0.55;
    }
    50% {
      opacity: 1;
    }
  }

  @media (prefers-color-scheme: dark) {
    :global(body) {
      color: #e5edf5;
    }

    .mini-status {
      border-color: rgba(148, 163, 184, 0.34);
      background: rgba(15, 23, 42, 0.97);
      box-shadow:
        0 5px 16px rgba(0, 0, 0, 0.34),
        0 1px 2px rgba(0, 0, 0, 0.28),
        inset 0 1px 0 rgba(255, 255, 255, 0.07);
    }

    .quota-row + .quota-row {
      border-left-color: rgba(148, 163, 184, 0.28);
    }

    .quota-label,
    .reset-time,
    .freshness {
      color: #c2cedb;
    }

    strong {
      color: #f8fafc;
    }

    .quota-row.low strong,
    .low-flag {
      color: #fdba74;
    }

    .menu-button {
      color: #c2cedb;
    }

    .menu-button:hover {
      color: #ffffff;
      background: rgba(148, 163, 184, 0.18);
    }

    .mini-status[data-state="error"] {
      border-color: rgba(253, 162, 155, 0.64);
    }

    .mini-status[data-state="stale"],
    .mini-status[data-state="warning"] {
      border-color: rgba(253, 186, 116, 0.58);
    }

    .mini-status[data-state="error"] .state-dot {
      background: #fda29b;
      box-shadow: 0 0 0 2px rgba(253, 162, 155, 0.18);
    }

    .mini-status[data-state="stale"] .state-dot,
    .mini-status[data-state="warning"] .state-dot {
      background: #fdba74;
      box-shadow: 0 0 0 2px rgba(253, 186, 116, 0.18);
    }

    .mini-status[data-state="error"] .freshness {
      color: #fda29b;
    }

    .mini-status[data-state="stale"] .freshness,
    .mini-status[data-state="warning"] .freshness {
      color: #fdba74;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    * {
      animation: none !important;
      transition: none !important;
    }
  }

  @media (max-width: 319px) {
    .mini-status {
      grid-template-columns: 8px minmax(0, 1fr) minmax(0, 1fr) auto 24px;
      column-gap: 4px;
    }

    .reset-time {
      display: none;
    }
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>


