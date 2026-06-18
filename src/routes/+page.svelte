<script lang="ts">
  import { onMount } from "svelte";
  import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
  import { createUsageState } from "$lib/state/usageState.svelte";

  const usage = createUsageState();

  onMount(() => {
    void usage.load();
  });
</script>

<svelte:head>
  <title>QuotaDock 额度监控</title>
</svelte:head>

<QuotaDashboard
  appState={usage.appState}
  parsedDraft={usage.parsedDraft}
  pasteText={usage.pasteText}
  loading={usage.loading}
  refreshing={usage.refreshing}
  parsing={usage.parsing}
  saving={usage.saving}
  errorMessage={usage.errorMessage}
  noticeMessage={usage.noticeMessage}
  onRefresh={() => usage.refreshUsage()}
  onParse={() => usage.parseStatusText()}
  onSave={() => usage.saveParsedDraft()}
  onClear={() => usage.clearSnapshot()}
  onPasteInput={(value) => {
    usage.pasteText = value;
  }}
/>
