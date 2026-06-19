<script lang="ts">
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import {
    formatPercent,
    formatReset,
    progressValue,
  } from "$lib/utils/format";

  export let appState: AppState | null;
  export let loading = false;
  export let refreshing = false;
  export let errorMessage: string | null = null;
  export let noticeMessage: string | null = null;

  const emptyReading: QuotaReading = {
    remainingPercent: null,
    resetAt: null,
    resetCountdownSeconds: null,
  };

  $: snapshot = appState?.latestSnapshot ?? null;
  $: fiveHour = snapshot?.fiveHour ?? emptyReading;
  $: weekly = snapshot?.weekly ?? emptyReading;
  $: statusText =
    errorMessage ??
    (refreshing ? "读取中..." : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    "点击查询";
  $: busy = loading || refreshing;
  $: weeklyResetText = `周更 ${formatReset(weekly)}`;
  $: titleText = `5小时 ${formatPercent(fiveHour.remainingPercent)} 更新 ${formatReset(fiveHour)}；1周 ${formatPercent(weekly.remainingPercent)} 更新 ${formatReset(weekly)}；${statusText}`;
</script>

<main class="float-shell" on:contextmenu|preventDefault>
  <section class="mini-status" aria-label="QuotaDock 状态栏" title={titleText} data-tauri-drag-region>
    <span class="logo-dot" aria-hidden="true" data-tauri-drag-region>
      <span data-tauri-drag-region></span>
    </span>
    <h1 class="sr-only">Codex 额度</h1>

    <div
      class:low={typeof fiveHour.remainingPercent === "number" && fiveHour.remainingPercent < 20}
      class="quota five"
      aria-label="5小时额度"
      style={`--value: ${progressValue(fiveHour.remainingPercent)}%`}
      data-tauri-drag-region
    >
      <span class="sr-only">5小时额度</span>
      <span class="quota-head" data-tauri-drag-region>
        <span class="label" aria-hidden="true" data-tauri-drag-region>5H</span>
        <strong data-testid="five-hour-value" data-tauri-drag-region>
          {formatPercent(fiveHour.remainingPercent)}
        </strong>
      </span>
      <span class="meter" aria-hidden="true" data-tauri-drag-region></span>
    </div>

    <div
      class:low={typeof weekly.remainingPercent === "number" && weekly.remainingPercent < 20}
      class="quota week"
      aria-label="1周额度"
      style={`--value: ${progressValue(weekly.remainingPercent)}%`}
      data-tauri-drag-region
    >
      <span class="sr-only">1周额度</span>
      <span class="quota-head" data-tauri-drag-region>
        <span class="label" aria-hidden="true" data-tauri-drag-region>1W</span>
        <strong data-testid="weekly-value" data-tauri-drag-region>
          {formatPercent(weekly.remainingPercent)}
        </strong>
      </span>
      <span class="meter" aria-hidden="true" data-tauri-drag-region></span>
    </div>

    <span
      class:error={Boolean(errorMessage)}
      class="meta"
      data-testid="status-message"
      aria-live="polite"
      data-tauri-drag-region
    >
      {busy ? "查询中" : weeklyResetText}
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
    color: #e8f3f5;
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
    place-items: center;
    padding: 3px;
    overflow: hidden;
    background: transparent;
  }

  .mini-status {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-columns: 24px minmax(58px, 1fr) minmax(58px, 1fr) minmax(58px, 0.82fr);
    align-items: center;
    gap: 4px;
    padding: 3px 4px;
    border: 1px solid rgba(134, 226, 232, 0.26);
    border-radius: 16px;
    background: rgba(9, 14, 18, 0.88);
    box-shadow:
      0 10px 26px rgba(0, 0, 0, 0.28),
      inset 0 1px 0 rgba(255, 255, 255, 0.08);
    backdrop-filter: blur(18px) saturate(1.25);
  }

  .logo-dot {
    position: relative;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 1px solid rgba(118, 222, 229, 0.54);
    border-radius: 8px;
    background: #102126;
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
  }

  .logo-dot::before,
  .logo-dot::after,
  .logo-dot span {
    content: "";
    position: absolute;
    border-radius: 999px;
    background: #76dee5;
  }

  .logo-dot::before {
    width: 11px;
    height: 3px;
    top: 6px;
    left: 5px;
  }

  .logo-dot::after {
    width: 3px;
    height: 10px;
    right: 6px;
    bottom: 5px;
    background: #c4f05d;
  }

  .logo-dot span {
    width: 6px;
    height: 6px;
    left: 6px;
    bottom: 6px;
    background: #f0b45d;
  }

  .quota {
    --accent: #76dee5;
    --track: rgba(233, 246, 247, 0.12);
    min-width: 0;
    height: 26px;
    display: grid;
    grid-template-rows: 1fr 3px;
    align-content: center;
    gap: 3px;
    padding: 3px 5px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.045);
  }

  .week {
    --accent: #c4f05d;
  }

  .quota.low {
    --accent: #f0b45d;
  }

  .quota-head {
    min-width: 0;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 4px;
    white-space: nowrap;
  }

  .label {
    color: rgba(232, 243, 245, 0.58);
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.52rem;
    font-weight: 800;
    line-height: 1;
  }

  strong {
    color: #f3fbfb;
    font-family: "SF Pro Display", "Segoe UI", "Microsoft YaHei UI", sans-serif;
    font-size: 0.86rem;
    font-variant-numeric: tabular-nums;
    font-weight: 850;
    line-height: 1;
  }

  .meter {
    position: relative;
    display: block;
    height: 3px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--track);
  }

  .meter::after {
    content: "";
    position: absolute;
    inset: 0 auto 0 0;
    width: var(--value);
    border-radius: inherit;
    background: var(--accent);
  }

  .meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: rgba(232, 243, 245, 0.62);
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.52rem;
    font-weight: 760;
    text-align: center;
  }

  .meta.error {
    color: #f0b45d;
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


