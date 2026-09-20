<script lang="ts">
  import type { EngineConfigAvailability } from '../model/types';
  import EngineConfigStatusRow from './EngineConfigStatusRow.svelte';

  type Props = {
    availability: EngineConfigAvailability;
    withGuidance?: boolean;
  };

  const { availability, withGuidance = false }: Props = $props();
</script>

<EngineConfigStatusRow {availability}>
  {#snippet manualGuidance()}
    {#if withGuidance}
      <div data-testid="manual-guidance" class="grid w-full gap-2 text-sm">
        <span>Set these values manually.</span>
        <pre><code>[SystemSettings]\nr.Test=1</code></pre>
        <button type="button">Copy</button>
      </div>
    {/if}
  {/snippet}
</EngineConfigStatusRow>
