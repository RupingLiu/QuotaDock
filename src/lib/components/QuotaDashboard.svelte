<script lang="ts">
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import { formatCapturedAt, formatPercent, formatReset } from "$lib/utils/format";

  export let appState: AppState | null;
  export let loading = false;
  export let refreshing = false;
  export let errorMessage: string | null = null;
  export let noticeMessage: string | null = null;
  export let onRefresh: () => void | Promise<void> = () => {};

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
  $: updatedAt = snapshot ? formatCapturedAt(snapshot.capturedAt) : "未更新";
  $: busy = loading || refreshing;
  $: titleText = `5小时 ${formatPercent(fiveHour.remainingPercent)} 更新 ${formatReset(fiveHour)}；1周 ${formatPercent(weekly.remainingPercent)} 更新 ${formatReset(weekly)}；${statusText}`;
</script>

<main class="float-shell">
  <section class="mini-status" aria-label="QuotaDock 右下角状态栏" title={titleText} data-tauri-drag-region>
    <span class="logo-dot" aria-hidden="true" data-tauri-drag-region></span>
    <h1 class="sr-only">Codex 额度</h1>

    <div class="quota five" aria-label="5小时额度" data-tauri-drag-region>
      <span class="sr-only">5小时额度</span>
      <span class="label" aria-hidden="true">5H</span>
      <strong data-testid="five-hour-value">{formatPercent(fiveHour.remainingPercent)}</strong>
    </div>

    <div class="quota week" aria-label="1周额度" data-tauri-drag-region>
      <span class="sr-only">1周额度</span>
      <span class="label" aria-hidden="true">1W</span>
      <strong data-testid="weekly-value">{formatPercent(weekly.remainingPercent)}</strong>
    </div>

    <span class:error={Boolean(errorMessage)} class="meta" data-testid="status-message" data-tauri-drag-region>
      {refreshing ? "查询中" : snapshot ? updatedAt : statusText}
    </span>

    <button type="button" disabled={busy} on:click={onRefresh} aria-label="自动查询" title="自动查询">
      <span aria-hidden="true">{refreshing ? "..." : "↻"}</span>
      <span class="sr-only">自动查询</span>
    </button>
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
    color: #27384a;
    background: transparent;
    font-family:
      -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI",
      "Microsoft YaHei UI", "Microsoft YaHei", sans-serif;
  }

  :global(body > div) {
    height: 100%;
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
    padding: 4px;
    overflow: hidden;
    background: transparent;
  }

  .mini-status {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-columns: 30px minmax(64px, 1fr) minmax(64px, 1fr) minmax(46px, 0.72fr) 32px;
    align-items: center;
    gap: 6px;
    padding: 5px 6px;
    border: 1px solid rgba(93, 107, 120, 0.18);
    border-radius: 999px;
    background: rgba(248, 247, 243, 0.92);
    box-shadow:
      0 10px 28px rgba(37, 48, 62, 0.18),
      inset 0 1px 0 rgba(255, 255, 255, 0.86);
    backdrop-filter: blur(16px) saturate(1.2);
  }

  .logo-dot {
    width: 28px;
    height: 28px;
    border-radius: 10px;
    background:
      radial-gradient(circle at 62% 34%, #ffffff 0 15%, transparent 16%),
      linear-gradient(135deg, #82aebb 0%, #3c5f78 56%, #c7ad7b 100%);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.64);
  }

  .quota {
    min-width: 0;
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 4px;
    white-space: nowrap;
  }

  .label {
    color: #7b858e;
    font-size: 0.62rem;
    font-weight: 760;
  }

  strong {
    color: #345a75;
    font-family: "SF Pro Display", "Segoe UI", "Microsoft YaHei UI", sans-serif;
    font-size: 1.12rem;
    font-variant-numeric: tabular-nums;
    font-weight: 820;
    line-height: 1;
  }

  .week strong {
    color: #7d6a54;
  }

  .meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #6d7680;
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.58rem;
    font-weight: 700;
    text-align: center;
  }

  .meta.error {
    color: #9a5c46;
  }

  button {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 999px;
    color: #fffaf0;
    background: #345a75;
    cursor: pointer;
    font-size: 1rem;
    font-weight: 820;
    line-height: 1;
    box-shadow: 0 6px 14px rgba(52, 90, 117, 0.22);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.58;
  }

  button:focus-visible {
    outline: 3px solid rgba(126, 163, 181, 0.42);
    outline-offset: 2px;
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


