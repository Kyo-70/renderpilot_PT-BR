<script lang="ts">
  import { TooltipProvider } from '@shared/ui';

  import LaunchArgumentsCallout from './LaunchArgumentsCallout.svelte';

  type Props = {
    launch: { arguments: string[]; requirement: 'required' | 'recommended' } | null;
    launcher: string;
  };

  const { launch, launcher }: Props = $props();
</script>

<TooltipProvider>
  <LaunchArgumentsCallout {launch} {launcher} />
</TooltipProvider>
