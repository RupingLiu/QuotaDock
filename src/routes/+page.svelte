<script lang="ts">
  import { onMount } from "svelte";
  import CodexHealthPanel from "$lib/components/CodexHealthPanel.svelte";
  import HistoryList from "$lib/components/HistoryList.svelte";
  import ManualEntryForm from "$lib/components/ManualEntryForm.svelte";
  import PasteStatusDialog from "$lib/components/PasteStatusDialog.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import UsageSummary from "$lib/components/UsageSummary.svelte";
  import { createUsageState } from "$lib/state/usageState.svelte";
  import type { Settings } from "$lib/types/usage";

  const usage = createUsageState();
  const fallbackSettings: Settings = {
    staleAfterMinutes: 60,
    notifyBelowPercent: [20, 10],
    clipboardMonitoring: false,
  };

  $: settings = usage.settings ?? fallbackSettings;
  $: health = usage.appState?.codexHealth ?? {
    status: "unknown" as const,
    available: false,
    authenticated: null,
    version: null,
    doctorStatus: null,
    checkedAt: null,
    diagnostics: [],
  };

  onMount(() => {
    void usage.load();
  });
</script>

<svelte:head>
  <title>QuotaDock</title>
</svelte:head>

<main>
  <header class="topbar">
    <div>
      <p class="eyebrow">QuotaDock</p>
      <h1>Codex usage panel</h1>
    </div>
    <div class="status-strip" aria-label="App status">
      <span>{usage.loading ? "loading" : (usage.appState?.storageStatus ?? "unavailable")}</span>
      <span>{usage.saving ? "saving" : "idle"}</span>
    </div>
  </header>

  {#if usage.errorMessage}
    <p class="error" role="alert">{usage.errorMessage}</p>
  {/if}

  <div class="dashboard">
    <section class="primary-column" aria-label="Usage workspace">
      <UsageSummary snapshot={usage.latestSnapshot} {settings} />
      <PasteStatusDialog
        draft={usage.parsedDraft}
        {settings}
        parsing={usage.parsing}
        saving={usage.saving}
        onParse={(rawText) => usage.parseStatusText(rawText)}
        onSave={() => usage.saveParsedDraft()}
      />
      <ManualEntryForm
        snapshot={usage.latestSnapshot}
        saving={usage.saving}
        onSubmit={(input) => usage.updateManualFields(input)}
      />
    </section>

    <aside class="side-column" aria-label="Support panels">
      <CodexHealthPanel
        {health}
        probing={usage.probing}
        onRefresh={() => usage.refreshProbe()}
        onOpenOfficialUsage={() => usage.openOfficialUsage()}
      />
      <HistoryList history={usage.history} />
      <SettingsPanel
        {settings}
        saving={usage.saving}
        storagePath={usage.appState?.storagePath ?? null}
        backupPath={usage.appState?.backupPath ?? null}
        storageStatus={usage.appState?.storageStatus ?? "unavailable"}
        onSave={(nextSettings) => usage.updateSettings(nextSettings)}
        onReset={() => usage.backupAndResetStore()}
      />
    </aside>
  </div>
</main>

<style>
  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
    color: #17211e;
    background: #f4f6f3;
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  :global(button),
  :global(input),
  :global(textarea) {
    letter-spacing: 0;
  }

  main {
    width: min(1180px, calc(100vw - 40px));
    margin: 0 auto;
    padding: 24px 0 40px;
  }

  .topbar {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 20px;
    margin-bottom: 16px;
  }

  .eyebrow {
    margin: 0 0 4px;
    color: #4f665c;
    font-size: 0.78rem;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    color: #16201d;
    font-size: 1.55rem;
    line-height: 1.2;
  }

  .status-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    justify-content: flex-end;
  }

  .status-strip span {
    min-height: 28px;
    display: inline-flex;
    align-items: center;
    border: 1px solid #cfd8d3;
    border-radius: 7px;
    padding: 0 10px;
    color: #40544b;
    background: #ffffff;
    font-size: 0.82rem;
    font-weight: 700;
  }

  .error {
    margin: 0 0 14px;
    padding: 10px 12px;
    border: 1px solid #e0aa9a;
    border-radius: 8px;
    color: #73331e;
    background: #fff0eb;
  }

  .dashboard {
    display: grid;
    grid-template-columns: minmax(0, 1.65fr) minmax(320px, 0.95fr);
    gap: 16px;
    align-items: start;
  }

  .primary-column,
  .side-column {
    display: grid;
    gap: 16px;
  }

  @media (max-width: 980px) {
    .dashboard {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 560px) {
    main {
      width: min(100vw - 24px, 1180px);
      padding-top: 16px;
    }

    .topbar {
      display: grid;
      align-items: start;
    }

    .status-strip {
      justify-content: start;
    }
  }
</style>
