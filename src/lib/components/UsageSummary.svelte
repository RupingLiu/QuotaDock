<script lang="ts">
  import ConfidenceBadge from "$lib/components/ConfidenceBadge.svelte";
  import type { Settings, UsageSnapshot } from "$lib/types/usage";
  import { evaluateSnapshotFreshness, formatAge } from "$lib/utils/freshness";
  import { formatCredits, formatDateTime, formatDuration, formatPercent } from "$lib/utils/format";

  export let snapshot: UsageSnapshot | null;
  export let settings: Settings;
  export let title = "Current usage";
  export let now = new Date();

  $: freshness = evaluateSnapshotFreshness(snapshot, settings, now);
  $: manualFields = snapshot?.manualFields ?? [];
</script>

<section class="panel usage-summary" aria-label={title}>
  <div class="panel-head">
    <div>
      <h2>{title}</h2>
      <p>{snapshot ? `${snapshot.source} • ${formatAge(freshness.ageMinutes)}` : "No snapshot"}</p>
    </div>
    <ConfidenceBadge state={freshness.state} warningCount={snapshot?.warnings.length ?? 0} />
  </div>

  <div class="metrics" data-testid="usage-summary">
    <div class="metric primary">
      <span>Remaining</span>
      <strong>{formatPercent(snapshot?.remainingPercent ?? null)}</strong>
    </div>
    <div class="metric">
      <span>Reset</span>
      <strong>{formatDateTime(snapshot?.resetAt ?? null)}</strong>
      <small>{formatDuration(snapshot?.resetCountdownSeconds ?? null)}</small>
    </div>
    <div class="metric">
      <span>Credits</span>
      <strong>{formatCredits(snapshot?.creditsBalance ?? null)}</strong>
    </div>
    <div class="metric">
      <span>Model</span>
      <strong>{snapshot?.model ?? "No model"}</strong>
      <small>{snapshot?.contextWindow ?? "No context"}</small>
    </div>
  </div>

  {#if manualFields.length > 0}
    <p class="manual-marker" data-testid="manual-marker">
      Manual fields: {manualFields.join(", ")}
    </p>
  {/if}

  {#if snapshot?.warnings.length}
    <ul class="warnings" data-testid="parse-warnings">
      {#each snapshot.warnings as warning}
        <li><strong>{warning.code}</strong> {warning.message}</li>
      {/each}
    </ul>
  {/if}

  {#if snapshot?.notes}
    <p class="notes">{snapshot.notes}</p>
  {/if}
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .usage-summary {
    display: grid;
    gap: 16px;
    padding: 16px;
  }

  .panel-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
  }

  h2,
  p {
    margin: 0;
  }

  h2 {
    color: #17211e;
    font-size: 1rem;
    line-height: 1.25;
  }

  .panel-head p,
  small {
    color: #69756f;
    font-size: 0.82rem;
  }

  .metrics {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 10px;
  }

  .metric {
    display: grid;
    min-height: 78px;
    align-content: space-between;
    gap: 6px;
    padding: 10px;
    border: 1px solid #e2e8e4;
    border-radius: 7px;
    background: #fbfcfb;
  }

  .metric span,
  .manual-marker,
  .notes {
    color: #596861;
    font-size: 0.82rem;
  }

  strong {
    overflow-wrap: anywhere;
    color: #16201d;
    font-size: 0.98rem;
    line-height: 1.25;
  }

  .primary strong {
    color: #17442c;
    font-size: 1.6rem;
  }

  .warnings {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 10px 12px 10px 28px;
    border: 1px solid #ead8a3;
    border-radius: 7px;
    color: #5e4b0c;
    background: #fff9df;
    font-size: 0.84rem;
  }

  .manual-marker,
  .notes {
    padding: 10px;
    border-radius: 7px;
    background: #f5f7fb;
  }

  @media (max-width: 820px) {
    .metrics {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 520px) {
    .panel-head {
      display: grid;
    }

    .metrics {
      grid-template-columns: 1fr;
    }
  }
</style>
