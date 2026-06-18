<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppState } from "$lib/types/usage";

  let appState: AppState | null = null;
  let errorMessage: string | null = null;

  onMount(async () => {
    try {
      appState = await invoke<AppState>("get_app_state");
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
    }
  });
</script>

<svelte:head>
  <title>QuotaDock</title>
</svelte:head>

<main>
  <header>
    <p class="eyebrow">QuotaDock</p>
    <h1>Codex usage status</h1>
  </header>

  <section class="status-panel" aria-label="Current app state">
    <div>
      <span class="label">State</span>
      <strong>{appState?.storageStatus ?? (errorMessage ? "unavailable" : "loading")}</strong>
    </div>
    <p>
      {errorMessage ??
        (appState?.latestSnapshot
          ? "Latest local usage snapshot loaded."
          : "No local usage snapshot yet. Paste or enter Codex status details in the next MVP tasks.")}
    </p>
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    min-width: 320px;
    min-height: 100vh;
    color: #18201d;
    background: #f5f7f4;
    font-family:
      Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  }

  main {
    width: min(720px, calc(100vw - 48px));
    margin: 0 auto;
    padding: 48px 0;
  }

  header {
    margin-bottom: 24px;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: #4b675b;
    font-size: 0.78rem;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  h1 {
    margin: 0;
    font-size: 2rem;
    line-height: 1.15;
  }

  .status-panel {
    display: grid;
    gap: 12px;
    padding: 20px;
    border: 1px solid #dce5dd;
    border-radius: 8px;
    background: #ffffff;
  }

  .status-panel div {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .label {
    color: #5f6d67;
    font-size: 0.9rem;
  }

  strong {
    color: #24533c;
    text-transform: capitalize;
  }

  p {
    margin: 0;
  }

  @media (max-width: 520px) {
    main {
      width: min(100vw - 32px, 720px);
      padding: 32px 0;
    }
  }
</style>
