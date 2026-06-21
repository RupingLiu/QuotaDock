<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
  import { createUsageState } from "$lib/state/usageState.svelte";
  import type { RefreshUsageResult } from "$lib/types/usage";

  const usage = createUsageState();

  onMount(() => {
    const canRefresh = hasTauriRuntime();
    const unlisten = hasTauriRuntime()
      ? listen<RefreshUsageResult>("usage-state-changed", (event) => {
          usage.applyRefreshResult(event.payload);
        })
      : null;
    const refreshIfVisible = () => {
      if (document.visibilityState === "visible") {
        void usage.refreshIfStale();
      }
    };
    const refreshOnFocus = () => {
      void usage.refreshIfStale();
    };

    void usage.load().then(() => {
      if (canRefresh) {
        void usage.refreshIfStale();
      }
    });
    if (canRefresh) {
      document.addEventListener("visibilitychange", refreshIfVisible);
      window.addEventListener("focus", refreshOnFocus);
    }

    return () => {
      if (unlisten) {
        void unlisten.then((dispose) => dispose());
      }
      if (canRefresh) {
        document.removeEventListener("visibilitychange", refreshIfVisible);
        window.removeEventListener("focus", refreshOnFocus);
      }
    };
  });

  function hasTauriRuntime(): boolean {
    return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }
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
/>
