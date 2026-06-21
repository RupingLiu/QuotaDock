<script lang="ts">
  import { onMount } from "svelte";
  import { tauriApi } from "$lib/api/tauri";
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import { formatPercent, formatReset } from "$lib/utils/format";

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
    isLow: boolean;
  };
  type TauriWindowHandle = {
    startDragging: () => Promise<void>;
  };

  const emptyReading: QuotaReading = {
    remainingPercent: null,
    resetAt: null,
    resetCountdownSeconds: null,
  };
  let tauriWindow: TauriWindowHandle | null = null;
  let tauriWindowLoad: Promise<TauriWindowHandle | null> | null = null;
  let lastDragStartAt = 0;

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
      resetText: compactFloatingReset(fiveHour),
      isLow:
        typeof fiveHour.remainingPercent === "number" &&
        fiveHour.remainingPercent < 20,
    },
    {
      id: "week",
      label: "1 周",
      ariaLabel: "1周额度",
      valueTestId: "weekly-value",
      resetTestId: "weekly-reset",
      remainingPercent: weekly.remainingPercent,
      resetText: compactFloatingReset(weekly),
      isLow:
        typeof weekly.remainingPercent === "number" &&
        weekly.remainingPercent < 20,
    },
  ] satisfies QuotaRow[];
  $: statusText =
    errorMessage ??
    (refreshing ? "读取中..." : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    null;
  $: busy = loading || refreshing;
  $: titleText = `5小时 ${formatPercent(fiveHour.remainingPercent)} 刷新 ${formatReset(fiveHour)}；1周 ${formatPercent(weekly.remainingPercent)} 刷新 ${formatReset(weekly)}${statusText ? `；${statusText}` : ""}`;

  onMount(() => {
    void preloadTauriWindow();
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
    void tauriApi.showDashboardContextMenu(event.clientX, event.clientY);
  }

  function hasTauriRuntime(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }

  function compactFloatingReset(reading: QuotaReading): string {
    return formatReset(reading)
      .replace(/^(\d{1,2})月(\d{1,2})日\s+/, "$1/$2 ")
      .replaceAll("小时", "h")
      .replaceAll("分钟", "m")
      .replaceAll("天", "d")
      .replace(/后$/, "");
  }
</script>

<main class="float-shell" on:contextmenu={showContextMenu}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <section
    class:error={Boolean(errorMessage)}
    class="mini-status"
    aria-busy={busy}
    aria-label="QuotaDock 状态栏"
    title={titleText}
    data-tauri-drag-region="deep"
    on:pointerenter={primeWindowDrag}
    on:pointerdown={startWindowDrag}
    on:mousedown={startWindowDrag}
  >
    {#each quotaRows as row (row.id)}
      <div
        class:low={row.isLow}
        class="quota-row"
        aria-label={row.ariaLabel}
      >
        <span class="sr-only">{row.ariaLabel}</span>
        <span
          class:first={row.id === "five"}
          class="quota-label"
          aria-hidden="true"
        >
          {row.label}
        </span>
        <span class="quota-metrics">
          <strong data-testid={row.valueTestId}>
            {formatPercent(row.remainingPercent)}
          </strong>
          <span class="reset-time" data-testid={row.resetTestId}>
            {row.resetText}
          </span>
        </span>
      </div>
    {/each}
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
    padding: 1px;
    overflow: hidden;
    background: transparent;
  }

  .mini-status {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    align-items: center;
    column-gap: 5px;
    padding: 2px 6px;
    border: 1px solid rgba(133, 154, 162, 0.18);
    border-radius: 7px;
    background: rgba(252, 253, 252, 0.96);
    box-shadow:
      0 5px 10px rgba(43, 59, 68, 0.1),
      0 1px 2px rgba(43, 59, 68, 0.08),
      inset 0 1px 0 rgba(255, 255, 255, 0.92);
    backdrop-filter: blur(12px) saturate(1.05);
    cursor: move;
  }

  .mini-status.error {
    border-color: rgba(199, 126, 43, 0.3);
  }

  .quota-row {
    min-width: 0;
    height: 100%;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .quota-row + .quota-row {
    border-left: 1px solid rgba(133, 154, 162, 0.22);
    padding-left: 6px;
  }

  .quota-label {
    min-width: 0;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    color: #1d2a31;
    font-size: 0.68rem;
    font-weight: 640;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quota-label:not(.first) {
    padding-left: 0;
  }

  .quota-metrics {
    min-width: 0;
    flex: 1 1 auto;
    display: grid;
    grid-template-columns: 27px minmax(0, 1fr);
    align-items: baseline;
    column-gap: 3px;
    justify-content: stretch;
  }

  strong,
  .reset-time {
    color: #687781;
    font-family:
      "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",
      "Microsoft YaHei", sans-serif;
    font-size: 0.68rem;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    font-weight: 540;
    line-height: 1;
  }

  .reset-time {
    overflow: hidden;
    max-width: 64px;
    font-weight: 450;
    text-align: right;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quota-row.low strong {
    color: #c77e2b;
  }

  @media (prefers-reduced-motion: reduce) {
    * {
      transition: none;
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


