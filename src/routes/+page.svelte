<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import DetailsPanel from "$lib/components/DetailsPanel.svelte";
  import QuotaDashboard from "$lib/components/QuotaDashboard.svelte";
  import { createUsageState } from "$lib/state/usageState.svelte";
  import type { RefreshProvidersResult, RefreshUsageResult } from "$lib/types/usage";

  const usage = createUsageState();
  let surface: "pending" | "main" | "details" = "pending";

  onMount(() => {
    const canRefresh = hasTauriRuntime();
    if (canRefresh) {
      void import("@tauri-apps/api/window")
        .then(({ getCurrentWindow }) => {
          surface = getCurrentWindow().label === "details" ? "details" : "main";
        })
        .catch(() => {
          surface = "main";
        });
    } else {
      surface =
        new URLSearchParams(window.location.search).get("surface") === "details"
          ? "details"
          : "main";
    }
    const unlisten = hasTauriRuntime()
      ? listen<RefreshUsageResult>("usage-state-changed", (event) => {
          usage.applyRefreshResult(event.payload);
        })
      : null;
    const unlistenProviders = hasTauriRuntime()
      ? listen<RefreshProvidersResult>("provider-state-changed", (event) => {
          usage.applyProviderResult(event.payload);
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
      if (unlistenProviders) {
        void unlistenProviders.then((dispose) => dispose());
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

{#if surface === "details"}
  <DetailsPanel
    appState={usage.appState}
    loading={usage.loading}
    refreshing={usage.refreshing}
    errorMessage={usage.errorMessage}
    noticeMessage={usage.noticeMessage}
    providerAnnouncement={usage.providerAnnouncement}
    onRefresh={() => usage.refreshProviders()}
    onStateChange={(state) => usage.applyAppState(state)}
  />
{:else if surface === "main"}
  <QuotaDashboard
    appState={usage.appState}
    loading={usage.loading}
    refreshing={usage.refreshing}
    errorMessage={usage.errorMessage}
    noticeMessage={usage.noticeMessage}
    providerAnnouncement={usage.providerAnnouncement}
  />
{/if}
