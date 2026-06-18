<script lang="ts">
  import ConfidenceBadge from "$lib/components/ConfidenceBadge.svelte";
  import type { UsageSnapshot } from "$lib/types/usage";
  import { formatDateTime, formatPercent } from "$lib/utils/format";

  export let history: UsageSnapshot[] = [];
</script>

<section class="panel history-panel" aria-label="Snapshot history">
  <div class="panel-head">
    <h2>History</h2>
    <span>{history.length} saved</span>
  </div>

  {#if history.length === 0}
    <p>No saved snapshots</p>
  {:else}
    <ol>
      {#each history.slice(0, 8) as snapshot}
        <li>
          <div>
            <strong>{formatPercent(snapshot.remainingPercent)}</strong>
            <span>{formatDateTime(snapshot.resetAt)}</span>
          </div>
          <ConfidenceBadge state={snapshot.confidence} warningCount={snapshot.warnings.length} />
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .history-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
  }

  .panel-head,
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  h2,
  p,
  ol {
    margin: 0;
  }

  h2 {
    font-size: 1rem;
  }

  .panel-head span,
  p,
  li span {
    color: #596861;
    font-size: 0.84rem;
  }

  ol {
    display: grid;
    gap: 8px;
    padding: 0;
    list-style: none;
  }

  li {
    min-height: 42px;
    padding: 8px 0;
    border-top: 1px solid #eef2ef;
  }

  li div {
    display: grid;
    gap: 3px;
  }

  strong {
    color: #17211e;
  }
</style>
