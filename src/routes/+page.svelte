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
  loading={usage.loading}
  refreshing={usage.refreshing}
  errorMessage={usage.errorMessage}
  noticeMessage={usage.noticeMessage}
  onRefresh={() => usage.refreshUsage()}
/>
