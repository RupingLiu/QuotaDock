<script lang="ts">
  import type { ConfidenceState } from "$lib/types/usage";
  import { confidenceLabel } from "$lib/utils/freshness";

  export let state: ConfidenceState;
  export let warningCount = 0;
</script>

<span class={`badge state-${state}`} data-testid="confidence-badge">
  {confidenceLabel(state)}
  {#if warningCount > 0}
    <span class="warnings">{warningCount} warning{warningCount === 1 ? "" : "s"}</span>
  {/if}
</span>

<style>
  .badge {
    display: inline-flex;
    min-height: 28px;
    align-items: center;
    gap: 8px;
    padding: 4px 9px;
    border: 1px solid #cfd7d2;
    border-radius: 7px;
    color: #17211e;
    background: #f7faf8;
    font-size: 0.82rem;
    font-weight: 700;
    letter-spacing: 0;
  }

  .state-fresh {
    border-color: #9fc7b0;
    color: #17442c;
    background: #eaf6ef;
  }

  .state-partial {
    border-color: #d8c46a;
    color: #5e4b0c;
    background: #fff8d6;
  }

  .state-manual {
    border-color: #a9b6d8;
    color: #243b75;
    background: #eef3ff;
  }

  .state-stale,
  .state-unavailable {
    border-color: #ddb7a6;
    color: #73331e;
    background: #fff0eb;
  }

  .warnings {
    color: inherit;
    font-size: 0.75rem;
    font-weight: 600;
  }
</style>
