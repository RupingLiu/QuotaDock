<script lang="ts">
  import UsageSummary from "$lib/components/UsageSummary.svelte";
  import type { Settings, UsageSnapshot } from "$lib/types/usage";

  export let draft: UsageSnapshot | null;
  export let settings: Settings;
  export let parsing = false;
  export let saving = false;
  export let onParse: (rawText: string) => void | Promise<void>;
  export let onSave: () => void | Promise<void>;

  let rawText = "";
  $: canParse = rawText.trim().length > 0 && !parsing;
  $: canSave = Boolean(draft) && !saving;

  async function parse() {
    if (canParse) {
      await onParse(rawText);
    }
  }
</script>

<section class="panel paste-panel" aria-label="Paste status">
  <div class="panel-head">
    <h2>Paste /status</h2>
    <button type="button" on:click={parse} disabled={!canParse}>
      {parsing ? "Parsing" : "Parse"}
    </button>
  </div>

  <textarea
    bind:value={rawText}
    rows="8"
    spellcheck="false"
    placeholder="Codex /status text"
    aria-label="Codex status text"
  ></textarea>

  {#if draft}
    <UsageSummary snapshot={draft} {settings} title="Parsed preview" />
    <button class="save" type="button" on:click={onSave} disabled={!canSave}>
      {saving ? "Saving" : "Save snapshot"}
    </button>
  {/if}
</section>

<style>
  .panel {
    border: 1px solid #d7ded9;
    border-radius: 8px;
    background: #ffffff;
  }

  .paste-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
  }

  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  h2 {
    margin: 0;
    font-size: 1rem;
  }

  textarea {
    width: 100%;
    min-height: 148px;
    box-sizing: border-box;
    resize: vertical;
    border: 1px solid #cfd8d3;
    border-radius: 7px;
    padding: 10px;
    color: #17211e;
    background: #fbfcfb;
    font: inherit;
    line-height: 1.45;
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

  button:disabled {
    border-color: #c3ccc6;
    color: #69756f;
    background: #eef2ef;
    cursor: default;
  }

  .save {
    justify-self: start;
  }
</style>
