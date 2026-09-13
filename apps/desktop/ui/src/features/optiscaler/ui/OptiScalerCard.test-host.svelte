<script lang="ts">
  import type { ComponentProps } from 'svelte';

  import { TooltipProvider } from '@shared/ui';

  import OptiScalerCard from './OptiScalerCard.svelte';

  const props: ComponentProps<typeof OptiScalerCard> = $props();
</script>

<TooltipProvider delayDuration={0}>
  <OptiScalerCard {...props} />
</TooltipProvider>
