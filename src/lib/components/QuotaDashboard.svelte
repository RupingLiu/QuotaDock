<script lang="ts">
  import type { AppState, QuotaReading } from "$lib/types/usage";
  import {
    formatCapturedAt,
    formatPercent,
    formatReset,
    progressValue,
    sourceLabel,
    storageLabel,
  } from "$lib/utils/format";

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
  $: storageText = storageLabel(appState?.storageStatus);
  $: updatedAt = snapshot ? formatCapturedAt(snapshot.capturedAt) : "尚未更新";
  $: busy = loading || refreshing;

  function progressStyle(reading: QuotaReading): string {
    return `--progress: ${progressValue(reading.remainingPercent)}%;`;
  }
</script>

<main class="shell">
  <section class="window" aria-label="QuotaDock 额度监控">
    <aside class="sidebar">
      <div>
        <p class="eyebrow">QuotaDock</p>
        <h1>Codex 额度</h1>
        <p class="summary">
          最后更新
          <strong>{updatedAt}</strong>
        </p>
      </div>

      <div class="sidebar-bottom">
        <div class="badges" aria-label="当前状态">
          <span>{sourceText}</span>
          <span>{storageText}</span>
        </div>
        <button class="primary" type="button" disabled={busy} on:click={onRefresh}>
          {refreshing ? "查询中" : "自动查询"}
        </button>
      </div>
    </aside>

    <section class="content" aria-label="额度详情">
      <div class="quota-list" data-testid="quota-grid">
        <article class="quota-line five" aria-label="5小时额度">
          <div class="quota-copy">
            <div class="line-head">
              <span>5小时额度</span>
              <b>下次更新 {formatReset(fiveHour)}</b>
            </div>
            <div class="track" aria-hidden="true">
              <span style={progressStyle(fiveHour)}></span>
            </div>
          </div>
          <strong class="quota-value" data-testid="five-hour-value">
            {formatPercent(fiveHour.remainingPercent)}
          </strong>
        </article>

        <article class="quota-line week" aria-label="1周额度">
          <div class="quota-copy">
            <div class="line-head">
              <span>1周额度</span>
              <b>下次更新 {formatReset(weekly)}</b>
            </div>
            <div class="track" aria-hidden="true">
              <span style={progressStyle(weekly)}></span>
            </div>
          </div>
          <strong class="quota-value" data-testid="weekly-value">
            {formatPercent(weekly.remainingPercent)}
          </strong>
        </article>
      </div>

      <div class="status-row">
        <span class:error={Boolean(errorMessage)} data-testid="status-message">{statusText}</span>
      </div>
    </section>
  </section>
</main>

<style>
  :global(html) {
    min-width: 320px;
    height: 100%;
    overflow: hidden;
    background: #f6f4ef;
  }

  :global(body) {
    margin: 0;
    min-width: 320px;
    height: 100%;
    overflow: hidden;
    color: #16212c;
    background: #f6f4ef;
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

  .shell {
    height: 100vh;
    min-height: 420px;
    overflow: hidden;
    background: #f6f4ef;
  }

  .window {
    width: 100%;
    height: 100%;
    min-height: 0;
    display: grid;
    grid-template-columns: clamp(218px, 24vw, 292px) minmax(0, 1fr);
    gap: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: #f6f4ef;
    box-shadow: none;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: 24px;
    min-width: 0;
    padding: 34px 28px 30px;
    border-right: 1px solid rgba(111, 103, 88, 0.18);
    background:
      linear-gradient(180deg, rgba(255, 255, 255, 0.52), transparent 58%),
      #f1eee7;
  }

  .eyebrow,
  h1,
  .summary {
    margin: 0;
  }

  .eyebrow {
    color: #7c7569;
    font-size: 0.78rem;
    font-weight: 650;
  }

  h1 {
    margin-top: 6px;
    color: #12202c;
    font-size: 1.5rem;
    line-height: 1.18;
    font-weight: 780;
  }

  .summary {
    margin-top: 12px;
    display: grid;
    gap: 2px;
    color: #7c7569;
    font-size: 0.78rem;
    line-height: 1.45;
  }

  .summary strong {
    color: #334e68;
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-weight: 700;
  }

  .sidebar-bottom {
    display: grid;
    gap: 14px;
  }

  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 7px;
  }

  .badges span {
    min-height: 24px;
    display: inline-flex;
    align-items: center;
    border-radius: 999px;
    padding: 0 9px;
    color: #596777;
    background: rgba(255, 255, 255, 0.66);
    font-size: 0.72rem;
    box-shadow: inset 0 0 0 1px rgba(92, 103, 113, 0.1);
  }

  button {
    min-height: 40px;
    border: 0;
    border-radius: 999px;
    padding: 0 20px;
    color: #fffaf0;
    background: #334e68;
    cursor: pointer;
    font-weight: 760;
    box-shadow: 0 8px 18px rgba(51, 78, 104, 0.18);
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.58;
  }

  button:focus-visible {
    outline: 3px solid rgba(126, 163, 181, 0.44);
    outline-offset: 3px;
  }

  .content {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 26px;
    min-width: 0;
    min-height: 0;
    padding: clamp(28px, 5vw, 62px) clamp(32px, 6vw, 72px) 34px;
  }

  .quota-list {
    display: grid;
    width: min(1080px, 100%);
    min-height: 0;
  }

  .quota-line {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) auto;
    align-items: center;
    gap: clamp(22px, 4vw, 54px);
    min-height: 190px;
    padding-block: 28px;
    border-bottom: 1px solid rgba(111, 103, 88, 0.16);
  }

  .quota-line:first-child {
    border-top: 0;
  }

  .quota-copy {
    min-width: 0;
  }

  .line-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 14px;
    margin-bottom: 16px;
  }

  .line-head span {
    color: #53606c;
    font-size: 0.88rem;
  }

  .line-head b {
    color: #7c7569;
    font-family: "SF Mono", "Cascadia Mono", Consolas, monospace;
    font-size: 0.78rem;
    font-weight: 700;
    white-space: nowrap;
  }

  .quota-value {
    color: #334e68;
    font-family: "SF Pro Display", "Segoe UI", "Microsoft YaHei UI", sans-serif;
    font-size: clamp(4.2rem, 9vw, 6.8rem);
    font-variant-numeric: tabular-nums;
    font-weight: 760;
    line-height: 1;
  }

  .week .quota-value {
    color: #7b6651;
  }

  .track {
    height: 7px;
    overflow: hidden;
    border-radius: 999px;
    background: rgba(77, 89, 101, 0.12);
  }

  .track span {
    display: block;
    width: var(--progress);
    height: 100%;
    border-radius: inherit;
    background: #7ea3b5;
  }

  .week .track span {
    background: #c3aa84;
  }

  .status-row {
    width: min(1080px, 100%);
    min-height: 28px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    color: #68727c;
    font-size: 0.8rem;
  }

  .status-row span.error {
    color: #9a5c46;
    font-weight: 650;
  }

  @media (max-width: 720px) {
    :global(html),
    :global(body) {
      overflow: auto;
    }

    .shell {
      height: auto;
      min-height: 100vh;
      overflow: visible;
    }

    .window {
      grid-template-columns: 1fr;
      gap: 22px;
      min-height: 100vh;
    }

    .sidebar {
      padding: 22px;
      border-right: 0;
      border-bottom: 1px solid rgba(111, 103, 88, 0.18);
    }

    .content {
      padding: 0 22px 24px;
    }

    .quota-line {
      grid-template-columns: 1fr;
      gap: 12px;
      min-height: 142px;
      padding-block: 18px;
    }

    .line-head {
      display: grid;
      gap: 4px;
      margin-bottom: 12px;
    }

    .quota-value {
      font-size: 3.8rem;
    }
  }
</style>
