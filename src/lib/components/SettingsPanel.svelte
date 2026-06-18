<script lang="ts">
  import type { Settings } from "$lib/types/usage";

  export let settings: Settings;
  export let saving = false;
  export let storagePath: string | null = null;
  export let backupPath: string | null = null;
  export let storageStatus = "ready";
  export let onSave: (settings: Settings) => void | Promise<void>;
  export let onReset: () => void | Promise<void>;

  let staleAfterMinutes = settings.staleAfterMinutes;
  let notifyBelowPercent = settings.notifyBelowPercent.join(", ");
  let clipboardMonitoring = settings.clipboardMonitoring;
  let notificationsEnabled = settings.notificationsEnabled;

  $: if (settings) {
    staleAfterMinutes = settings.staleAfterMinutes;
    notifyBelowPercent = settings.notifyBelowPercent.join(", ");
    clipboardMonitoring = settings.clipboardMonitoring;
    notificationsEnabled = settings.notificationsEnabled;
  }

  async function save() {
    await onSave({
      staleAfterMinutes: Math.max(1, Math.round(Number(staleAfterMinutes))),
      notifyBelowPercent: parseThresholds(notifyBelowPercent),
      clipboardMonitoring,
      notificationsEnabled,
    });
  }

  function parseThresholds(value: string): number[] {
    const parsed = value
      .split(",")
      .map((part) => Number(part.trim()))
      .filter((part) => Number.isFinite(part))
      .map((part) => Math.min(100, Math.max(0, Math.round(part))));
    return [...new Set(parsed)].sort((a, b) => b - a);
  }
</script>

<section class="panel settings-panel" aria-label="Settings">
  <div>
    <h2>Settings</h2>
    <p>{storageStatus}</p>
  </div>

  <div class="fields">
    <label>
      Stale after minutes
      <input bind:value={staleAfterMinutes} type="number" min="1" inputmode="numeric" />
    </label>
    <label>
      Notify below %
      <input bind:value={notifyBelowPercent} type="text" />
    </label>
    <label class="toggle">
      <input bind:checked={clipboardMonitoring} type="checkbox" />
      Clipboard monitoring
    </label>
    <label class="toggle">
      <input bind:checked={notificationsEnabled} type="checkbox" />
      Notifications
    </label>
  </div>

  <div class="actions">
    <button type="button" on:click={save} disabled={saving}>
      {saving ? "Saving" : "Save settings"}
    </button>
    <button type="button" class="secondary" on:click={onReset} disabled={saving}>Backup reset</button>
  </div>

  <dl>
    <div>
      <dt>Store</dt>
      <dd>{storagePath ?? "Not created"}</dd>
    </div>
    {#if backupPath}
      <div>
        <dt>Backup</dt>
        <dd>{backupPath}</dd>
      </div>
    {/if}
  </dl>
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .settings-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
  }

  h2,
  p,
  dl {
    margin: 0;
  }

  h2 {
    font-size: 1rem;
  }

  p,
  label,
  dt {
    color: #596861;
    font-size: 0.84rem;
  }

  .fields {
    display: grid;
    gap: 10px;
  }

  label {
    display: grid;
    gap: 5px;
  }

  .toggle {
    display: flex;
    min-height: 34px;
    align-items: center;
    gap: 9px;
  }

  input[type="number"],
  input[type="text"] {
    min-height: 34px;
    box-sizing: border-box;
    border: 1px solid #cfd8d3;
    border-radius: 7px;
    padding: 0 9px;
    color: #17211e;
    background: #fbfcfb;
    font: inherit;
  }

  input[type="checkbox"] {
    width: 18px;
    height: 18px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  button {
    min-height: 34px;
    border: 1px solid #244e3c;
    border-radius: 7px;
    padding: 0 12px;
    color: #ffffff;
    background: #244e3c;
    font: inherit;
    font-weight: 700;
    cursor: pointer;
  }

  .secondary {
    color: #244e3c;
    background: #ffffff;
  }

  button:disabled {
    border-color: #c3ccc6;
    color: #69756f;
    background: #eef2ef;
    cursor: default;
  }

  dl {
    display: grid;
    gap: 6px;
  }

  dl div {
    display: grid;
    gap: 2px;
  }

  dd {
    margin: 0;
    color: #17211e;
    font-size: 0.78rem;
    overflow-wrap: anywhere;
  }
</style>
