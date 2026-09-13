<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLAttributes } from 'svelte/elements';
  import { cn } from '@shared/classnames';

  type Props = HTMLAttributes<HTMLDivElement> & {
    ref?: HTMLDivElement | null;
    leading?: Snippet;
    actions?: Snippet;
  };

  let { ref = $bindable(null), class: className, leading, actions, ...restProps }: Props = $props();
</script>

<div
  bind:this={ref}
  data-slot="addon-card-footer"
  class={cn(
    'mt-auto flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between',
    className,
  )}
  {...restProps}
>
  {#if leading}
    {@render leading()}
  {/if}

  {#if actions}
    <div class="flex flex-wrap items-center gap-2 sm:ms-auto sm:justify-end">
      {@render actions()}
    </div>
  {/if}
</div>
