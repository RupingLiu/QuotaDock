<script lang="ts">
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import { formatCapturedAt, formatPercent, formatReset, sourceLabel } from "$lib/utils/format";

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
    (refreshing ? "正在读取 Codex 额度..." : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    "尚未获取额度。请点击自动查询。";
  $: sourceText = sourceLabel(snapshot?.source);
  $: updatedAt = snapshot ? formatCapturedAt(snapshot.capturedAt) : "尚未更新";
  $: busy = loading || refreshing;
</script>

<main class="float-shell">
  <section class="floating-bar" aria-label="QuotaDock 额度悬浮条" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="logo-dot" aria-hidden="true"></span>
      <div>
        <h1>Codex 额度</h1>
        <p>{updatedAt}</p>
      </div>
    </div>

    <div class="metric five" aria-label="5小时额度" data-tauri-drag-region>
      <span>5小时额度</span>
      <strong data-testid="five-hour-value">{formatPercent(fiveHour.remainingPercent)}</strong>
      <small>更新 {formatReset(fiveHour)}</small>
    </div>

    <div class="divider" aria-hidden="true"></div>

    <div class="metric week" aria-label="1周额度" data-tauri-drag-region>
      <span>1周额度</span>
      <strong data-testid="weekly-value">{formatPercent(weekly.remainingPercent)}</strong>
      <small>更新 {formatReset(weekly)}</small>
    </div>

    <div class="status" data-tauri-drag-region>
      <span class="source">{sourceText}</span>
      <span class:error={Boolean(errorMessage)} data-testid="status-message">{statusText}</span>
    </div>

    <button type="button" disabled={busy} on:click={onRefresh}>
      {refreshing ? "查询中" : "自动查询"}
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
    color: #182536;
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
    padding: 8px;
    overflow: hidden;
    background: transparent;
  }

  .floating-bar {
    width: 100%;
    height: 100%;
    min-width: 0;
    display: grid;
    grid-template-columns: 142px minmax(118px, 1fr) 1px minmax(118px, 1fr) minmax(150px, 1.05fr) 96px;
    align-items: center;
    gap: 14px;
    padding: 10px 14px;
    border: 1px solid rgba(75, 92, 106, 0.16);
    border-radius: 18px;
    background: rgba(248, 247, 243, 0.96);
    box-shadow:
      0 18px 44px rgba(43, 55, 70, 0.22),
      inset 0 1px 0 rgba(255, 255, 255, 0.9);
    backdrop-filter: blur(18px);
  }

  .brand {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .logo-dot {
    width: 30px;
    height: 30px;
    flex: 0 0 auto;
    border-radius: 10px;
    background:
      radial-gradient(circle at 62% 34%, #ffffff 0 16%, transparent 17%),
      linear-gradient(135deg, #75a9bd 0%, #385b76 54%, #c8b081 100%);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.65);
  }

  h1,
  p {
    margin: 0;
  }

  h1 {
    color: #102033;
    font-size: 0.94rem;
    line-height: 1.18;
    font-weight: 780;
    white-space: nowrap;
  }

  .brand p,
  .metric small,
  .status span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .brand p {
    margin-top: 3px;
    color: #6f7680;
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.68rem;
    font-weight: 700;
  }

  .metric {
    min-width: 0;
    display: grid;
    grid-template-columns: auto 1fr;
    column-gap: 8px;
    row-gap: 3px;
    align-items: baseline;
  }

  .metric span {
    color: #59636f;
    font-size: 0.76rem;
    white-space: nowrap;
  }

  .metric strong {
    color: #345a75;
    font-family: "SF Pro Display", "Segoe UI", "Microsoft YaHei UI", sans-serif;
    font-size: 2.05rem;
    font-variant-numeric: tabular-nums;
    font-weight: 780;
    line-height: 1;
    justify-self: end;
  }

  .week strong {
    color: #7b6651;
  }

  .metric small {
    grid-column: 1 / -1;
    color: #7b7469;
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.68rem;
    font-weight: 700;
  }

  .divider {
    width: 1px;
    height: 42px;
    background: rgba(75, 92, 106, 0.14);
  }

  .status {
    min-width: 0;
    display: grid;
    gap: 4px;
  }

  .source {
    width: max-content;
    max-width: 100%;
    border-radius: 999px;
    padding: 2px 8px;
    color: #4d6172;
    background: rgba(255, 255, 255, 0.72);
    box-shadow: inset 0 0 0 1px rgba(75, 92, 106, 0.12);
    font-size: 0.68rem;
  }

  .status span:last-child {
    color: #69747d;
    font-size: 0.72rem;
  }

  .status span.error {
    color: #9a5c46;
    font-weight: 700;
  }

  button {
    min-height: 42px;
    border: 0;
    border-radius: 999px;
    padding: 0 14px;
    color: #fffaf0;
    background: #345a75;
    cursor: pointer;
    font-size: 0.88rem;
    font-weight: 760;
    box-shadow: 0 9px 18px rgba(52, 90, 117, 0.22);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.58;
  }

  button:focus-visible {
    outline: 3px solid rgba(126, 163, 181, 0.44);
    outline-offset: 3px;
  }
</style>
