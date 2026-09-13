<script lang="ts">
  import AddonCardFooter from './AddonCardFooter.svelte';

  type Props = {
    showLeading?: boolean;
    showActions?: boolean;
    class?: string;
  };

  const { showLeading = true, showActions = true, class: className = '' }: Props = $props();
</script>

<AddonCardFooter class={className}>
  {#snippet leading()}
    {#if showLeading}
      <span data-testid="leading-content">Attribution Info</span>
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if showActions}
      <button type="button" data-testid="action-btn">Action</button>
    {/if}
  {/snippet}
</AddonCardFooter>
