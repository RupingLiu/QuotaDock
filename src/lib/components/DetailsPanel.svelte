<script lang="ts">
  import { onMount } from "svelte";
  import { tauriApi } from "$lib/api/tauri";
  import type {
    AppDiagnostics,
    AppState,
    SettingsPatch,
    UpdateStatus,
    UsageHistoryPoint,
  } from "$lib/types/usage";
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
  export let onStateChange: (state: AppState) => void = () => {};

  let diagnostics: AppDiagnostics | null = null;
  let actionError: string | null = null;
  let savingSetting: string | null = null;
  let startupEnabled = false;
  let updateStatus: UpdateStatus | null = null;
  let updateStatusRevision = 0;

  $: snapshot = appState?.latestSnapshot ?? null;
  $: history = appState?.history ?? [];
  $: weeklyPath = sparklinePath(history);
  $: statusText =
    actionError ??
    errorMessage ??
    (refreshing ? "正在从 Codex 读取额度…" : null) ??
    noticeMessage ??
    appState?.statusMessage ??
    "等待首次额度更新。";
  $: updateBusy =
    updateStatus?.phase === "checking" ||
    updateStatus?.phase === "downloading" ||
    updateStatus?.phase === "installing";

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void loadDiagnostics();
    void (async () => {
      try {
        const stopListening = await tauriApi.onUpdateStatus((status) => {
          updateStatusRevision += 1;
          if (!disposed) updateStatus = status;
        });
        if (disposed) {
          stopListening();
          return;
        }
        unlisten = stopListening;

        const revisionBeforeSnapshot = updateStatusRevision;
        const snapshot = await tauriApi.getUpdateStatus();
        if (!disposed && revisionBeforeSnapshot === updateStatusRevision) {
          updateStatus = snapshot;
        }
      } catch (error) {
        if (!disposed) actionError = errorText(error);
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function loadDiagnostics(): Promise<void> {
    try {
      diagnostics = await tauriApi.getDiagnostics();
      startupEnabled = diagnostics.startupEnabled;
    } catch (error) {
      actionError = errorText(error);
    }
  }

  async function checkUpdates(): Promise<void> {
    actionError = null;
    const actionRevision = ++updateStatusRevision;
    try {
      const status = await tauriApi.checkForUpdates();
      if (actionRevision === updateStatusRevision) {
        updateStatus = status;
        updateStatusRevision += 1;
      }
    } catch (error) {
      actionError = errorText(error);
    }
  }

  async function installDownloadedUpdate(): Promise<void> {
    actionError = null;
    const actionRevision = ++updateStatusRevision;
    try {
      const status = await tauriApi.installDownloadedUpdate();
      if (actionRevision === updateStatusRevision) {
        updateStatus = status;
        updateStatusRevision += 1;
      }
    } catch (error) {
      actionError = errorText(error);
    }
  }

  async function runUpdateAction(): Promise<void> {
    if (updateStatus?.phase === "ready") {
      await installDownloadedUpdate();
      return;
    }
    await checkUpdates();
  }

  async function openLatestRelease(): Promise<void> {
    try {
      await tauriApi.openLatestRelease();
    } catch (error) {
      actionError = errorText(error);
    }
  }

  async function saveSettings(patch: SettingsPatch, key: string): Promise<void> {
    actionError = null;
    savingSetting = key;
    try {
      const next = await tauriApi.updateSettings(patch);
      onStateChange(next);
    } catch (error) {
      actionError = errorText(error);
    } finally {
      savingSetting = null;
    }
  }

  async function setStartup(enabled: boolean): Promise<void> {
    actionError = null;
    savingSetting = "startup";
    try {
      startupEnabled = await tauriApi.setStartupEnabled(enabled);
      if (diagnostics) {
        diagnostics = { ...diagnostics, startupEnabled };
      }
    } catch (error) {
      actionError = errorText(error);
    } finally {
      savingSetting = null;
    }
  }

  async function acknowledgeRecovery(): Promise<void> {
    actionError = null;
    savingSetting = "recovery";
    try {
      onStateChange(await tauriApi.acknowledgeRecovery());
    } catch (error) {
      actionError = errorText(error);
    } finally {
      savingSetting = null;
    }
  }

  async function closeDetails(): Promise<void> {
    try {
      await tauriApi.hideDetails();
    } catch (error) {
      actionError = errorText(error);
    }
  }

  async function openOfficialUsage(): Promise<void> {
    try {
      await tauriApi.openOfficialUsage();
    } catch (error) {
      actionError = errorText(error);
    }
  }

  function sparklinePath(points: UsageHistoryPoint[]): string {
    const values = points
      .slice(-48)
      .map((point) => point.weeklyRemainingPercent)
      .filter((value): value is number => typeof value === "number");
    if (values.length === 0) return "";
    if (values.length === 1) return `M 0 ${100 - values[0]} L 240 ${100 - values[0]}`;
    return values
      .map((value, index) => {
        const x = (index / (values.length - 1)) * 240;
        const y = 100 - Math.max(0, Math.min(100, value));
        return `${index === 0 ? "M" : "L"} ${x.toFixed(1)} ${y.toFixed(1)}`;
      })
      .join(" ");
  }

  function errorText(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function updatePhaseLabel(status: UpdateStatus | null): string {
    switch (status?.phase) {
      case "checking":
        return "检查中";
      case "up-to-date":
        return "已是最新";
      case "downloading":
        return "下载中";
      case "ready":
        return "可安装";
      case "installing":
        return "安装中";
      case "error":
        return "需要处理";
      default:
        return "未检查";
    }
  }

  function updateActionLabel(status: UpdateStatus | null): string {
    switch (status?.phase) {
      case "checking":
        return "正在检查…";
      case "downloading":
        return status.progressPercent === null
          ? "正在下载…"
          : `下载中 ${status.progressPercent}%`;
      case "ready":
        return "立即安装";
      case "installing":
        return "正在安装…";
      case "error":
        return "重新检查";
      default:
        return "检查更新";
    }
  }
</script>

<main class="details-shell" aria-busy={loading || refreshing}>
  <header class="hero">
    <div>
      <p class="eyebrow">QUOTADOCK</p>
      <h1>额度详情</h1>
    </div>
    <div class="header-actions">
      <button
        class="icon-button"
        type="button"
        aria-label="刷新额度"
        title="刷新额度"
        disabled={refreshing}
        on:click={() => onRefresh()}
      >
        <svg class:spin={refreshing} viewBox="0 0 24 24" aria-hidden="true">
          <path d="M20 11a8 8 0 1 0-2.34 5.66M20 5v6h-6"></path>
        </svg>
      </button>
      <button
        class="icon-button"
        type="button"
        aria-label="关闭详情"
        title="关闭"
        on:click={closeDetails}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M6 6l12 12M18 6 6 18"></path>
        </svg>
      </button>
    </div>
  </header>

  <section class:error={Boolean(errorMessage || actionError)} class="status-strip" aria-live="polite">
    <span class="status-dot" aria-hidden="true"></span>
    <span>{statusText}</span>
  </section>

  {#if appState?.recoveryNotice}
    <section class="recovery-card" aria-label="存储恢复提示">
      <div>
        <strong>本地状态已安全恢复</strong>
        <p>{appState.recoveryNotice.message}</p>
        <p class="path" title={appState.recoveryNotice.backupPath}>
          备份：{appState.recoveryNotice.backupPath}
        </p>
      </div>
      <button
        type="button"
        disabled={savingSetting === "recovery"}
        on:click={acknowledgeRecovery}
      >知道了</button>
    </section>
  {/if}

  <section class="quota-grid" aria-label="额度概览">
    <article class:low={(snapshot?.weekly.remainingPercent ?? 101) <= 20} class="quota-card">
      <div class="quota-heading">
        <span>1 周额度</span>
        <strong>{formatPercent(snapshot?.weekly.remainingPercent ?? null)}</strong>
      </div>
      <div class="progress-track" aria-hidden="true">
        <span style={`width: ${progressValue(snapshot?.weekly.remainingPercent ?? null)}%`}></span>
      </div>
      <p>重置：{snapshot ? formatReset(snapshot.weekly, { capturedAt: snapshot.capturedAt }) : "--"}</p>
    </article>
  </section>

  <section class="meta-row" aria-label="数据状态">
    <div><span>来源</span><strong>{sourceLabel(snapshot?.source)}</strong></div>
    <div><span>上次成功</span><strong>{formatCapturedAt(snapshot?.capturedAt)}</strong></div>
    <div><span>存储</span><strong>{storageLabel(appState?.storageStatus)}</strong></div>
  </section>

  {#if snapshot?.planType || snapshot?.creditsBalance || snapshot?.resetCreditsAvailable}
    <section class="account-row" aria-label="账户信息">
      {#if snapshot.planType}<span>计划 <strong>{snapshot.planType}</strong></span>{/if}
      {#if snapshot.creditsBalance}<span>Credits <strong>{snapshot.creditsBalance}</strong></span>{/if}
      {#if snapshot.resetCreditsAvailable !== null}
        <span>重置次数 <strong>{snapshot.resetCreditsAvailable}</strong></span>
      {/if}
    </section>
  {/if}

  <section
    class:error={updateStatus?.phase === "error"}
    class="panel update-panel"
    aria-labelledby="software-update-heading"
  >
    <div class="section-heading update-heading">
      <div>
        <p class="section-kicker">UPDATE</p>
        <h2 id="software-update-heading">软件更新</h2>
      </div>
      <span
        class:attention={updateStatus?.phase === "error"}
        class:ready={updateStatus?.phase === "ready"}
        class="update-badge"
      >
        {updatePhaseLabel(updateStatus)}
      </span>
    </div>
    <div class="update-summary">
      <div class="update-copy" role="status" aria-live="polite" aria-atomic="true">
        <p class="update-message">{updateStatus?.message ?? "正在读取更新状态…"}</p>
        <small>
          当前 v{updateStatus?.currentVersion ?? diagnostics?.appVersion ?? "--"}
          · 上次检查 {formatCapturedAt(updateStatus?.checkedAt ?? null)}
        </small>
      </div>
      <button
        class="update-action primary"
        type="button"
        disabled={updateBusy}
        on:click={runUpdateAction}
      >
        {updateActionLabel(updateStatus)}
      </button>
    </div>
    {#if updateStatus?.phase === "downloading" && updateStatus.progressPercent !== null}
      <div
        class="update-progress"
        role="progressbar"
        aria-label="更新下载进度"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={updateStatus.progressPercent}
      >
        <span style={`width: ${updateStatus.progressPercent}%`}></span>
      </div>
    {/if}
    {#if updateStatus?.phase === "error"}
      <div class="update-recovery">
        <button class="update-action secondary" type="button" on:click={openLatestRelease}>
          浏览器下载
        </button>
        {#if updateStatus.technicalDetail}
          <details>
            <summary>技术详情</summary>
            <code>{updateStatus.technicalDetail}</code>
          </details>
        {/if}
      </div>
    {/if}
  </section>

  <section class="panel history-panel">
    <div class="section-heading">
      <div>
        <p class="section-kicker">HISTORY</p>
        <h2>1 周额度趋势</h2>
      </div>
      <span>{history.length} 个采样点</span>
    </div>
    {#if weeklyPath}
      <div class="chart">
        <svg viewBox="0 0 240 100" preserveAspectRatio="none" role="img" aria-label="1 周剩余额度趋势">
          <path class="grid" d="M0 25H240M0 50H240M0 75H240"></path>
          <path class="series" d={weeklyPath}></path>
        </svg>
      </div>
    {:else}
      <p class="empty-state">刷新几次后，这里会显示剩余额度变化。</p>
    {/if}
  </section>

  <section class="panel settings-panel">
    <div class="section-heading">
      <div>
        <p class="section-kicker">CONTROL</p>
        <h2>行为设置</h2>
      </div>
    </div>

    <label class="setting-row">
      <span>
        <strong>自动检查更新</strong>
        <small>启动后及每 6 小时静默检查并准备更新，完成后轻提示</small>
      </span>
      <input
        type="checkbox"
        checked={appState?.settings.automaticUpdateChecks ?? true}
        disabled={savingSetting === "updates"}
        on:change={(event) =>
          saveSettings(
            { automaticUpdateChecks: event.currentTarget.checked },
            "updates",
          )}
      />
    </label>

    <label class="setting-row">
      <span>
        <strong>低额度通知</strong>
        <small>剩余额度首次降至 20% 或以下时通知</small>
      </span>
      <input
        type="checkbox"
        checked={appState?.settings.lowQuotaNotifications ?? false}
        disabled={savingSetting === "notifications"}
        on:change={(event) =>
          saveSettings(
            { lowQuotaNotifications: event.currentTarget.checked },
            "notifications",
          )}
      />
    </label>

    <label class="setting-row">
      <span>
        <strong>开机自启动</strong>
        <small>登录 Windows 后在托盘中启动 QuotaDock</small>
      </span>
      <input
        type="checkbox"
        checked={startupEnabled}
        disabled={!diagnostics || savingSetting === "startup"}
        on:change={(event) => setStartup(event.currentTarget.checked)}
      />
    </label>
  </section>

  <section class="panel diagnostics-panel">
    <div class="section-heading">
      <div>
        <p class="section-kicker">DIAGNOSTICS</p>
        <h2>连接与安全</h2>
      </div>
      <span class="security-badge">签名更新</span>
    </div>
    <dl>
      <div><dt>QuotaDock</dt><dd>{diagnostics?.appVersion ?? "读取中…"}</dd></div>
      <div><dt>Codex CLI</dt><dd>{diagnostics?.codexVersion ?? "未检测到"}</dd></div>
      <div>
        <dt>Codex 路径</dt>
        <dd class="path" title={diagnostics?.codexPath ?? ""}>{diagnostics?.codexPath ?? "--"}</dd>
      </div>
      <div>
        <dt>状态文件</dt>
        <dd class="path" title={diagnostics?.storagePath ?? ""}>{diagnostics?.storagePath ?? "--"}</dd>
      </div>
    </dl>
    <button class="official-link" type="button" on:click={openOfficialUsage}>
      打开 Codex 官方 Usage Dashboard
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M14 5h5v5M19 5l-8 8M19 13v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5"></path>
      </svg>
    </button>
  </section>
</main>

<style>
  :global(html) {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: #eef2f5;
  }

  :global(body) {
    width: 100%;
    height: 100%;
    margin: 0;
    overflow: hidden;
    color: #17242b;
    background: #eef2f5;
    font-family:
      "Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI",
      "Microsoft YaHei", sans-serif;
  }

  :global(body > div) {
    height: 100%;
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(button),
  :global(input) {
    font: inherit;
  }

  .details-shell {
    width: 100%;
    height: 100%;
    padding: 20px 20px 28px;
    overflow-x: hidden;
    overflow-y: auto;
    background:
      radial-gradient(circle at 88% 0%, rgba(15, 118, 110, 0.12), transparent 27%),
      linear-gradient(180deg, #f8fafb 0%, #eef2f5 100%);
    scrollbar-color: #b7c2cb transparent;
    scrollbar-width: thin;
  }

  .hero,
  .section-heading,
  .quota-heading,
  .setting-row,
  .meta-row,
  .account-row {
    display: flex;
    align-items: center;
  }

  .hero {
    justify-content: space-between;
    margin-bottom: 14px;
  }

  .eyebrow,
  .section-kicker {
    margin: 0 0 3px;
    color: #0f766e;
    font-size: 0.63rem;
    font-weight: 750;
    letter-spacing: 0.14em;
  }

  h1,
  h2,
  p {
    margin-top: 0;
  }

  h1 {
    margin-bottom: 0;
    font-size: 1.5rem;
    font-weight: 690;
    letter-spacing: -0.035em;
  }

  h2 {
    margin-bottom: 0;
    font-size: 0.98rem;
    font-weight: 670;
  }

  .header-actions {
    display: flex;
    gap: 6px;
  }

  .icon-button {
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid #d3dce3;
    border-radius: 9px;
    color: #425466;
    background: rgba(255, 255, 255, 0.88);
    cursor: pointer;
  }

  .icon-button:hover {
    color: #0f766e;
    border-color: #9bc8c3;
    background: white;
  }

  .icon-button:focus-visible,
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid #0f766e;
    outline-offset: 2px;
  }

  .icon-button:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .icon-button svg,
  .official-link svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.8;
  }

  .spin {
    animation: spin 0.9s linear infinite;
  }

  .status-strip {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    min-height: 34px;
    margin-bottom: 12px;
    padding: 9px 11px;
    border: 1px solid #d7e1e5;
    border-radius: 10px;
    color: #425466;
    background: rgba(255, 255, 255, 0.68);
    font-size: 0.77rem;
    line-height: 1.35;
  }

  .status-strip.error {
    color: #9f2d20;
    border-color: #efc3bd;
    background: #fff8f7;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    flex: 0 0 auto;
    margin-top: 2px;
    border-radius: 50%;
    background: #0f766e;
    box-shadow: 0 0 0 3px rgba(15, 118, 110, 0.12);
  }

  .status-strip.error .status-dot {
    background: #b42318;
    box-shadow: 0 0 0 3px rgba(180, 35, 24, 0.1);
  }

  .recovery-card {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    justify-content: space-between;
    margin-bottom: 12px;
    padding: 12px;
    border: 1px solid #f0c58c;
    border-radius: 11px;
    color: #78350f;
    background: #fff9ec;
  }

  .recovery-card strong {
    font-size: 0.82rem;
  }

  .recovery-card p {
    margin: 4px 0 0;
    font-size: 0.72rem;
    line-height: 1.35;
  }

  .recovery-card button {
    flex: 0 0 auto;
    padding: 5px 9px;
    border: 0;
    border-radius: 7px;
    color: #fff;
    background: #92400e;
    font-size: 0.72rem;
    cursor: pointer;
  }

  .quota-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    margin-bottom: 10px;
  }

  .quota-card,
  .panel {
    border: 1px solid rgba(148, 163, 184, 0.27);
    background: rgba(255, 255, 255, 0.92);
    box-shadow:
      0 5px 16px rgba(15, 23, 42, 0.045),
      0 1px 2px rgba(15, 23, 42, 0.04);
  }

  .quota-card {
    min-width: 0;
    padding: 13px;
    border-radius: 12px;
  }

  .quota-heading {
    justify-content: space-between;
    gap: 8px;
    color: #475569;
    font-size: 0.75rem;
  }

  .quota-heading strong {
    color: #17242b;
    font-size: 1.18rem;
    font-variant-numeric: tabular-nums;
  }

  .progress-track {
    height: 5px;
    margin: 10px 0 8px;
    overflow: hidden;
    border-radius: 99px;
    background: #e3e9ed;
  }

  .progress-track span {
    height: 100%;
    display: block;
    border-radius: inherit;
    background: linear-gradient(90deg, #0f766e, #3aa79d);
  }

  .quota-card.low .progress-track span {
    background: #c2410c;
  }

  .quota-card.low .quota-heading strong {
    color: #b93815;
  }

  .quota-card p {
    margin-bottom: 0;
    overflow: hidden;
    color: #64748b;
    font-size: 0.69rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta-row {
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 10px;
    padding: 0 3px;
  }

  .meta-row div {
    min-width: 0;
  }

  .meta-row span,
  .meta-row strong {
    display: block;
  }

  .meta-row span {
    margin-bottom: 2px;
    color: #64748b;
    font-size: 0.62rem;
  }

  .meta-row strong {
    max-width: 130px;
    overflow: hidden;
    color: #334155;
    font-size: 0.67rem;
    font-weight: 620;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .account-row {
    flex-wrap: wrap;
    gap: 6px 14px;
    margin: 0 3px 10px;
    color: #64748b;
    font-size: 0.67rem;
  }

  .account-row strong {
    color: #334155;
    font-weight: 650;
  }

  .panel {
    margin-top: 10px;
    padding: 14px;
    border-radius: 13px;
  }

  .section-heading {
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .section-heading > span {
    color: #64748b;
    font-size: 0.66rem;
  }

  .chart {
    height: 94px;
    overflow: hidden;
  }

  .chart svg {
    width: 100%;
    height: 100%;
    overflow: visible;
  }

  .grid,
  .series {
    fill: none;
    vector-effect: non-scaling-stroke;
  }

  .grid {
    stroke: #e6ebef;
    stroke-width: 1;
  }

  .series {
    stroke: #0f766e;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 2;
  }

  .empty-state {
    margin: 18px 0 10px;
    color: #64748b;
    font-size: 0.75rem;
    text-align: center;
  }

  .setting-row {
    justify-content: space-between;
    gap: 18px;
    min-height: 48px;
    padding: 8px 0;
    cursor: pointer;
  }

  .setting-row + .setting-row {
    border-top: 1px solid #edf1f4;
  }

  .setting-row span {
    min-width: 0;
  }

  .setting-row strong,
  .setting-row small {
    display: block;
  }

  .setting-row strong {
    margin-bottom: 3px;
    color: #253443;
    font-size: 0.78rem;
  }

  .setting-row small {
    color: #64748b;
    font-size: 0.66rem;
    line-height: 1.35;
  }

  .setting-row input {
    width: 35px;
    height: 20px;
    flex: 0 0 auto;
    appearance: none;
    border: 1px solid #b9c4cc;
    border-radius: 99px;
    background: #dbe2e7;
    cursor: pointer;
    transition: 150ms ease;
  }

  .setting-row input::before {
    width: 14px;
    height: 14px;
    display: block;
    margin: 2px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 1px 3px rgba(15, 23, 42, 0.22);
    content: "";
    transition: transform 150ms ease;
  }

  .setting-row input:checked {
    border-color: #0f766e;
    background: #0f766e;
  }

  .setting-row input:checked::before {
    transform: translateX(15px);
  }

  .setting-row input:disabled {
    cursor: wait;
    opacity: 0.55;
  }

  .security-badge {
    padding: 4px 7px;
    border-radius: 99px;
    color: #0f6b63 !important;
    background: #e5f5f2;
    font-size: 0.62rem !important;
    font-weight: 700;
  }

  .update-panel {
    border-color: rgba(15, 118, 110, 0.24);
    background: linear-gradient(145deg, #ffffff 0%, #f3fbfa 100%);
  }

  .update-panel.error {
    border-color: #efc3bd;
    background: linear-gradient(145deg, #ffffff 0%, #fff8f7 100%);
  }

  .update-heading {
    margin-bottom: 9px;
  }

  .update-badge {
    padding: 4px 7px;
    border-radius: 99px;
    color: #0f6b63 !important;
    background: #e5f5f2;
    font-size: 0.62rem !important;
    font-weight: 700;
  }

  .update-badge.attention {
    color: #9a3412 !important;
    background: #fff0e7;
  }

  .update-badge.ready {
    color: #166534 !important;
    background: #eaf7ee;
  }

  .update-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
  }

  .update-copy {
    min-width: 0;
  }

  .update-message {
    margin-bottom: 3px;
    color: #253443;
    font-size: 0.75rem;
    font-weight: 650;
    line-height: 1.4;
  }

  .update-summary small {
    color: #64748b;
    font-size: 0.64rem;
  }

  .update-action {
    min-height: 32px;
    flex: 0 0 auto;
    padding: 6px 10px;
    border-radius: 8px;
    font-size: 0.69rem;
    font-weight: 650;
    cursor: pointer;
    transition: 150ms ease;
  }

  .update-action.primary {
    border: 1px solid #0f766e;
    color: white;
    background: #0f766e;
  }

  .update-action.primary:hover:not(:disabled) {
    border-color: #0b5f59;
    background: #0b5f59;
  }

  .update-action.secondary {
    border: 1px solid #c6d4da;
    color: #334155;
    background: white;
  }

  .update-action.secondary:hover {
    color: #0f6b63;
    border-color: #86b9b3;
    background: #f4fbfa;
  }

  .update-action:disabled {
    cursor: wait;
    opacity: 0.62;
  }

  .update-progress {
    height: 5px;
    margin-top: 10px;
    overflow: hidden;
    border-radius: 99px;
    background: #dfe8e8;
  }

  .update-progress span {
    height: 100%;
    display: block;
    border-radius: inherit;
    background: #0f766e;
    transition: width 180ms ease;
  }

  .update-recovery {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid rgba(148, 163, 184, 0.24);
  }

  .update-recovery details {
    min-width: 0;
    flex: 1 1 180px;
    color: #64748b;
    font-size: 0.65rem;
  }

  .update-recovery summary {
    width: max-content;
    cursor: pointer;
    user-select: none;
  }

  .update-recovery code {
    display: block;
    max-height: 92px;
    margin-top: 7px;
    padding: 8px;
    overflow: auto;
    border-radius: 7px;
    color: #7f1d1d;
    background: rgba(255, 255, 255, 0.78);
    font-family: Consolas, "Cascadia Mono", monospace;
    font-size: 0.61rem;
    line-height: 1.45;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  dl {
    margin: 0;
  }

  dl div {
    display: grid;
    grid-template-columns: 78px minmax(0, 1fr);
    gap: 10px;
    padding: 5px 0;
    font-size: 0.68rem;
  }

  dt {
    color: #64748b;
  }

  dd {
    min-width: 0;
    margin: 0;
    color: #334155;
    font-weight: 550;
    text-align: right;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .official-link {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 7px;
    margin-top: 12px;
    padding: 8px 10px;
    border: 1px solid #bfd5d2;
    border-radius: 8px;
    color: #0f6b63;
    background: #f4fbfa;
    font-size: 0.72rem;
    font-weight: 620;
    cursor: pointer;
  }

  .official-link:hover {
    border-color: #86b9b3;
    background: #eaf7f5;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    * {
      animation: none !important;
      transition: none !important;
    }
  }
</style>
