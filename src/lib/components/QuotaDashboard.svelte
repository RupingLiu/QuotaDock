<script lang="ts">
  import type { AppState, QuotaReading, QuotaSnapshot } from "$lib/types/usage";
  import {
    formatCapturedAt,
    formatPercent,
    formatReset,
    progressValue,
    sourceLabel,
    storageLabel,
  } from "$lib/utils/format";

  export let appState: AppState | null;
  export let parsedDraft: QuotaSnapshot | null = null;
  export let pasteText = "";
  export let loading = false;
  export let refreshing = false;
  export let parsing = false;
  export let saving = false;
  export let errorMessage: string | null = null;
  export let noticeMessage: string | null = null;
  export let onRefresh: () => void | Promise<void> = () => {};
  export let onParse: () => void | Promise<void> = () => {};
  export let onSave: () => void | Promise<void> = () => {};
  export let onClear: () => void | Promise<void> = () => {};
  export let onPasteInput: (value: string) => void = () => {};

  const emptyReading: QuotaReading = {
    remainingPercent: null,
    resetAt: null,
    resetCountdownSeconds: null,
  };

  $: snapshot = parsedDraft ?? appState?.latestSnapshot ?? null;
  $: fiveHour = snapshot?.fiveHour ?? emptyReading;
  $: weekly = snapshot?.weekly ?? emptyReading;
  $: statusText =
    errorMessage ??
    (refreshing ? "正在后台读取 Codex /status，窗口可继续操作..." : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    "尚未获取额度。请点击自动查询或粘贴 /status。";
  $: sourceText = parsedDraft ? "解析预览" : sourceLabel(snapshot?.source);
  $: updatedAt = snapshot ? formatCapturedAt(snapshot.capturedAt) : "尚未更新";
  $: busy = loading || refreshing || parsing || saving;

  function progressStyle(reading: QuotaReading): string {
    return `--progress: ${progressValue(reading.remainingPercent)}%;`;
  }
</script>

<main class="shell">
  <section class="deck" aria-label="QuotaDock 额度监控">
    <header class="topbar">
      <div>
        <p class="eyebrow">QuotaDock</p>
        <h1>Codex 额度监控舱</h1>
      </div>
      <div class="status-cluster" aria-label="当前状态">
        <span class="pill">{sourceText}</span>
        <span class="pill">{storageLabel(appState?.storageStatus)}</span>
      </div>
    </header>

    <div class="meta-row">
      <span>最后更新</span>
      <strong>{updatedAt}</strong>
    </div>

    <div class="quota-grid" data-testid="quota-grid">
      <article class="quota-cell five" aria-label="5小时额度">
        <div class="cell-head">
          <span>5小时额度</span>
          <small>5H WINDOW</small>
        </div>
        <strong class="quota-value" data-testid="five-hour-value">
          {formatPercent(fiveHour.remainingPercent)}
        </strong>
        <div class="track" aria-hidden="true">
          <span style={progressStyle(fiveHour)}></span>
        </div>
        <p>下次更新 <b>{formatReset(fiveHour)}</b></p>
      </article>

      <article class="quota-cell week" aria-label="1周额度">
        <div class="cell-head">
          <span>1周额度</span>
          <small>1W WINDOW</small>
        </div>
        <strong class="quota-value" data-testid="weekly-value">
          {formatPercent(weekly.remainingPercent)}
        </strong>
        <div class="track" aria-hidden="true">
          <span style={progressStyle(weekly)}></span>
        </div>
        <p>下次更新 <b>{formatReset(weekly)}</b></p>
      </article>
    </div>

    <section class="console" aria-label="操作区">
      <div class="message" class:error={Boolean(errorMessage)} data-testid="status-message">
        {statusText}
      </div>

      <div class="actions">
        <button class="primary" type="button" disabled={busy} on:click={onRefresh}>
          {refreshing ? "查询中" : "自动查询"}
        </button>
        <button type="button" disabled={busy || !pasteText.trim()} on:click={onParse}>
          {parsing ? "解析中" : "粘贴 /status 更新"}
        </button>
        <button type="button" disabled={busy || !parsedDraft} on:click={onSave}>
          {saving && parsedDraft ? "保存中" : "保存解析结果"}
        </button>
        <button class="ghost" type="button" disabled={busy || (!snapshot && !pasteText)} on:click={onClear}>
          清空
        </button>
      </div>

      <textarea
        aria-label="粘贴 /status 内容"
        placeholder="粘贴 Codex /status 输出..."
        value={pasteText}
        on:input={(event) => onPasteInput(event.currentTarget.value)}
      ></textarea>

      {#if snapshot?.warnings.length}
        <ul class="warnings" data-testid="parse-warnings">
          {#each snapshot.warnings as warning}
            <li>{warning.message}</li>
          {/each}
        </ul>
      {/if}
    </section>
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
    color: #f5f7fa;
    background:
      linear-gradient(rgba(34, 230, 209, 0.04) 1px, transparent 1px),
      linear-gradient(90deg, rgba(155, 124, 255, 0.035) 1px, transparent 1px),
      #05070a;
    background-size: 28px 28px;
    font-family:
      "Microsoft YaHei UI", "Microsoft YaHei", "Segoe UI", system-ui, sans-serif;
  }

  :global(button),
  :global(textarea) {
    letter-spacing: 0;
    font: inherit;
  }

  .shell {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 22px;
  }

  .deck {
    width: min(920px, 100%);
    display: grid;
    gap: 14px;
    padding: 18px;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    background:
      linear-gradient(135deg, rgba(34, 230, 209, 0.07), transparent 42%),
      linear-gradient(315deg, rgba(155, 124, 255, 0.08), transparent 48%),
      #0b1114;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.42);
  }

  .topbar,
  .meta-row,
  .cell-head,
  .actions {
    display: flex;
    align-items: center;
  }

  .topbar {
    justify-content: space-between;
    gap: 18px;
  }

  .eyebrow,
  h1 {
    margin: 0;
  }

  .eyebrow {
    color: #22e6d1;
    font-family: Bahnschrift, "Microsoft YaHei UI", sans-serif;
    font-size: 0.78rem;
    font-weight: 700;
  }

  h1 {
    margin-top: 2px;
    font-family: Bahnschrift, "Microsoft YaHei UI", sans-serif;
    font-size: 1.45rem;
    line-height: 1.2;
  }

  .status-cluster,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
  }

  .pill {
    min-height: 28px;
    display: inline-flex;
    align-items: center;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    padding: 0 10px;
    color: #d7e8ea;
    background: rgba(5, 7, 10, 0.72);
    font-size: 0.8rem;
  }

  .meta-row {
    justify-content: space-between;
    gap: 10px;
    min-height: 36px;
    border-block: 1px solid #1d2a2d;
    color: #8ea4a7;
    font-size: 0.84rem;
  }

  .meta-row strong {
    color: #ffcc66;
    font-family: "Cascadia Mono", Consolas, monospace;
  }

  .quota-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .quota-cell {
    position: relative;
    overflow: hidden;
    min-height: 232px;
    display: grid;
    align-content: space-between;
    gap: 14px;
    padding: 16px;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    background: #071014;
  }

  .quota-cell::before {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.06), transparent 36%);
    pointer-events: none;
  }

  .five {
    box-shadow: inset 0 0 0 1px rgba(34, 230, 209, 0.1);
  }

  .week {
    box-shadow: inset 0 0 0 1px rgba(155, 124, 255, 0.12);
  }

  .cell-head {
    position: relative;
    justify-content: space-between;
    gap: 10px;
  }

  .cell-head span {
    color: #f5f7fa;
    font-weight: 700;
  }

  .cell-head small {
    color: #71888c;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 0.72rem;
  }

  .quota-value {
    position: relative;
    color: #22e6d1;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 4.2rem;
    line-height: 1;
  }

  .week .quota-value {
    color: #9b7cff;
  }

  .track {
    position: relative;
    height: 8px;
    overflow: hidden;
    border-radius: 8px;
    background: #121c20;
  }

  .track span {
    display: block;
    width: var(--progress);
    height: 100%;
    border-radius: inherit;
    background: #22e6d1;
  }

  .week .track span {
    background: #9b7cff;
  }

  .quota-cell p {
    position: relative;
    margin: 0;
    color: #8ea4a7;
    font-size: 0.92rem;
  }

  .quota-cell b {
    color: #ffcc66;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-weight: 700;
  }

  .console {
    display: grid;
    gap: 10px;
    padding-top: 2px;
  }

  .message {
    min-height: 36px;
    display: flex;
    align-items: center;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    padding: 0 12px;
    color: #d7e8ea;
    background: rgba(5, 7, 10, 0.7);
    font-size: 0.88rem;
  }

  .message.error {
    border-color: rgba(255, 204, 102, 0.5);
    color: #ffcc66;
  }

  button {
    min-height: 36px;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    padding: 0 13px;
    color: #f5f7fa;
    background: #10181b;
    cursor: pointer;
  }

  button.primary {
    border-color: rgba(34, 230, 209, 0.55);
    color: #051014;
    background: #22e6d1;
    font-weight: 800;
  }

  button.ghost {
    color: #8ea4a7;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.45;
  }

  button:focus-visible,
  textarea:focus-visible {
    outline: 2px solid #ffcc66;
    outline-offset: 2px;
  }

  textarea {
    min-height: 112px;
    resize: vertical;
    border: 1px solid #1d2a2d;
    border-radius: 8px;
    padding: 12px;
    color: #f5f7fa;
    background: #05070a;
    font-family: "Cascadia Mono", Consolas, "Microsoft YaHei UI", monospace;
    line-height: 1.45;
  }

  .warnings {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 10px 12px 10px 28px;
    border: 1px solid rgba(255, 204, 102, 0.45);
    border-radius: 8px;
    color: #ffcc66;
    background: rgba(255, 204, 102, 0.08);
    font-size: 0.84rem;
  }

  @media (max-width: 720px) {
    .shell {
      padding: 12px;
      place-items: start center;
    }

    .topbar {
      display: grid;
      align-items: start;
    }

    .status-cluster,
    .actions {
      justify-content: flex-start;
    }

    .quota-grid {
      grid-template-columns: 1fr;
    }

    .quota-value {
      font-size: 3.4rem;
    }
  }
</style>
