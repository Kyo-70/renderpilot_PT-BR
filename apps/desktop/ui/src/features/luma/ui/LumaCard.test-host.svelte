<script lang="ts">
  import type { ComponentProps } from 'svelte';

  import { TooltipProvider } from '@shared/ui';

  import LumaCard from './LumaCard.svelte';

  const props: ComponentProps<typeof LumaCard> = $props();
</script>

<TooltipProvider delayDuration={0}>
  <LumaCard {...props} />
</TooltipProvider>
