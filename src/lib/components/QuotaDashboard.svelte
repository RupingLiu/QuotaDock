<script lang="ts">
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

  const emptyReading: QuotaReading = {
    remainingPercent: null,
    resetAt: null,
    resetCountdownSeconds: null,
  };

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
      resetText: formatReset(fiveHour),
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
      resetText: formatReset(weekly),
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

  async function startWindowDrag(event: PointerEvent): Promise<void> {
    if (event.button !== 0 || !hasTauriRuntime()) {
      return;
    }

    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().startDragging();
    } catch {
      // The declarative Tauri drag region remains the primary drag path.
    }
  }

  function hasTauriRuntime(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }
</script>

<main class="float-shell" on:contextmenu|preventDefault>
  <section
    class:error={Boolean(errorMessage)}
    class="mini-status"
    aria-busy={busy}
    aria-label="QuotaDock 状态栏"
    title={titleText}
    data-tauri-drag-region
    on:pointerdown={startWindowDrag}
  >
    <div class="panel-title" data-tauri-drag-region>
      <span class="quota-icon" aria-hidden="true" data-tauri-drag-region></span>
      <h1 data-tauri-drag-region>剩余用量</h1>
    </div>

    {#each quotaRows as row (row.id)}
      <div
        class:low={row.isLow}
        class="quota-row"
        aria-label={row.ariaLabel}
        data-tauri-drag-region
      >
        <span class="sr-only">{row.ariaLabel}</span>
        <span class="quota-label" aria-hidden="true" data-tauri-drag-region>
          {row.label}
        </span>
        <span class="quota-metrics" data-tauri-drag-region>
          <strong data-testid={row.valueTestId} data-tauri-drag-region>
            {formatPercent(row.remainingPercent)}
          </strong>
          <span
            class="reset-time"
            data-testid={row.resetTestId}
            data-tauri-drag-region
          >
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
    padding: 5px;
    overflow: hidden;
    background: transparent;
  }

  .mini-status {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-rows: 24px 1fr 1fr;
    gap: 2px;
    padding: 10px 14px 11px;
    border: 1px solid rgba(133, 154, 162, 0.18);
    border-radius: 10px;
    background: rgba(252, 253, 252, 0.96);
    box-shadow:
      0 12px 24px rgba(43, 59, 68, 0.12),
      0 1px 2px rgba(43, 59, 68, 0.08),
      inset 0 1px 0 rgba(255, 255, 255, 0.92);
    backdrop-filter: blur(18px) saturate(1.08);
    cursor: move;
  }

  .mini-status.error {
    border-color: rgba(199, 126, 43, 0.3);
  }

  .panel-title {
    min-width: 0;
    display: grid;
    grid-template-columns: 18px 1fr;
    align-items: center;
    gap: 8px;
  }

  .quota-icon {
    position: relative;
    width: 16px;
    height: 16px;
    border-radius: 999px;
    background:
      radial-gradient(circle at center, #fbfcfc 0 5px, transparent 5.5px),
      conic-gradient(from 220deg, #168f95 0 68%, rgba(22, 143, 149, 0.16) 68% 100%);
  }

  .quota-icon::after {
    content: "";
    position: absolute;
    right: 1px;
    bottom: 1px;
    width: 4px;
    height: 4px;
    border-radius: 999px;
    background: #ebb46b;
    box-shadow: 0 0 0 2px rgba(251, 252, 252, 0.9);
  }

  h1 {
    margin: 0;
    color: #17242b;
    font-size: 0.94rem;
    font-weight: 680;
    letter-spacing: 0;
    line-height: 1.1;
  }

  .quota-row {
    min-width: 0;
    display: grid;
    grid-template-columns: minmax(62px, 1fr) auto;
    align-items: center;
    column-gap: 12px;
    padding-left: 26px;
  }

  .quota-label {
    min-width: 0;
    overflow: hidden;
    color: #1d2a31;
    font-size: 0.88rem;
    font-weight: 640;
    line-height: 1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .quota-metrics {
    min-width: 0;
    display: grid;
    grid-template-columns: 42px minmax(100px, auto);
    align-items: baseline;
    column-gap: 10px;
    justify-content: end;
  }

  strong,
  .reset-time {
    color: #687781;
    font-family:
      "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",
      "Microsoft YaHei", sans-serif;
    font-size: 0.88rem;
    font-variant-numeric: tabular-nums;
    font-feature-settings: "tnum";
    font-weight: 540;
    line-height: 1;
  }

  .reset-time {
    overflow: hidden;
    max-width: 110px;
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


