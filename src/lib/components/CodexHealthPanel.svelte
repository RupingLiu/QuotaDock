<script lang="ts">
  import type { CodexHealth } from "$lib/types/usage";
  import { formatDateTime } from "$lib/utils/format";

  export let health: CodexHealth;
  export let probing = false;
  export let onRefresh: () => void | Promise<void>;
  export let onOpenOfficialUsage: () => void | Promise<void>;

  $: authLabel =
    health.authenticated === true ? "Signed in" : health.authenticated === false ? "Signed out" : "Unknown";
</script>

<section class="panel health-panel" aria-label="Codex health">
  <div class="panel-head">
    <div>
      <h2>Codex health</h2>
      <p>{health.status} • {authLabel}</p>
    </div>
    <div class="actions">
      <button type="button" class="secondary" on:click={onOpenOfficialUsage}>Usage</button>
      <button type="button" on:click={onRefresh} disabled={probing}>
        {probing ? "Checking" : "Refresh"}
      </button>
    </div>
  </div>

  <dl>
    <div>
      <dt>CLI</dt>
      <dd>{health.available ? (health.version ?? "Available") : "Unavailable"}</dd>
    </div>
    <div>
      <dt>Doctor</dt>
      <dd>{health.doctorStatus ?? "No status"}</dd>
    </div>
    <div>
      <dt>Checked</dt>
      <dd>{health.checkedAt?.startsWith("unix:") ? health.checkedAt : formatDateTime(health.checkedAt)}</dd>
    </div>
  </dl>

  {#if health.diagnostics.length > 0}
    <ul>
      {#each health.diagnostics as diagnostic}
        <li>{diagnostic}</li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .health-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
  }

  .panel-head,
  .actions {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 10px;
  }

  .actions {
    align-items: center;
  }

  h2,
  p,
  dl,
  ul {
    margin: 0;
  }

  h2 {
    font-size: 1rem;
  }

  p,
  dt,
  li {
    color: #596861;
    font-size: 0.84rem;
  }

  dl {
    display: grid;
    gap: 8px;
  }

  dl div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  dd {
    margin: 0;
    color: #17211e;
    text-align: right;
    overflow-wrap: anywhere;
  }

  ul {
    display: grid;
    gap: 5px;
    padding: 10px 12px 10px 28px;
    border-radius: 7px;
    background: #f8faf9;
  }

  button {
    min-height: 34px;
    border: 1px solid #244e3c;
    border-radius: 7px;
    padding: 0 11px;
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

  @media (max-width: 560px) {
    .panel-head {
      display: grid;
    }
  }
</style>
