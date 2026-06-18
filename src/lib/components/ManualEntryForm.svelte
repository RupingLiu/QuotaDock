<script lang="ts">
  import type { ManualUpdateInput, UsageSnapshot } from "$lib/types/usage";

  export let snapshot: UsageSnapshot | null;
  export let saving = false;
  export let onSubmit: (input: ManualUpdateInput) => void | Promise<void>;

  let remainingPercent = "";
  let resetAt = "";
  let creditsBalance = "";
  let notes = "";

  $: if (snapshot) {
    remainingPercent = snapshot.remainingPercent?.toString() ?? "";
    resetAt = toLocalDatetime(snapshot.resetAt);
    creditsBalance = snapshot.creditsBalance?.toString() ?? "";
    notes = snapshot.notes ?? "";
  }

  async function submit() {
    await onSubmit({
      remainingPercent: parsePercent(remainingPercent),
      resetAt: fromLocalDatetime(resetAt),
      creditsBalance: parseNumber(creditsBalance),
      notes,
    });
  }

  function parsePercent(value: string): number | null {
    if (!value.trim()) {
      return null;
    }
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.min(100, Math.max(0, Math.round(parsed))) : null;
  }

  function parseNumber(value: string): number | null {
    if (!value.trim()) {
      return null;
    }
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function toLocalDatetime(value: string | null): string {
    if (!value) {
      return "";
    }
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
      return "";
    }
    const offset = date.getTimezoneOffset() * 60000;
    return new Date(date.getTime() - offset).toISOString().slice(0, 16);
  }

  function fromLocalDatetime(value: string): string | null {
    if (!value.trim()) {
      return null;
    }
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date.toISOString();
  }
</script>

<section class="panel manual-panel" aria-label="Manual fields">
  <div>
    <h2>Manual fields</h2>
    <p>Saved values are marked manual.</p>
  </div>

  <div class="grid">
    <label>
      Remaining %
      <input bind:value={remainingPercent} type="number" min="0" max="100" inputmode="numeric" />
    </label>
    <label>
      Reset time
      <input bind:value={resetAt} type="datetime-local" />
    </label>
    <label>
      Credits
      <input bind:value={creditsBalance} type="number" step="0.01" inputmode="decimal" />
    </label>
    <label class="notes">
      Notes
      <input bind:value={notes} type="text" />
    </label>
  </div>

  <button type="button" on:click={submit} disabled={saving}>
    {saving ? "Saving" : "Save manual"}
  </button>
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .manual-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    font-size: 1rem;
  }

  p,
  label {
    color: #596861;
    font-size: 0.84rem;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }

  label {
    display: grid;
    gap: 5px;
  }

  .notes {
    grid-column: 1 / -1;
  }

  input {
    min-height: 34px;
    box-sizing: border-box;
    border: 1px solid #cfd8d3;
    border-radius: 7px;
    padding: 0 9px;
    color: #17211e;
    background: #fbfcfb;
    font: inherit;
  }

  button {
    justify-self: start;
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

  button:disabled {
    border-color: #c3ccc6;
    color: #69756f;
    background: #eef2ef;
    cursor: default;
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>
