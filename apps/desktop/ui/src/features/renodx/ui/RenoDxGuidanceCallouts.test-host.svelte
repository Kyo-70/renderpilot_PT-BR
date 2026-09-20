<script lang="ts">
  import { TooltipProvider } from '@shared/ui';

  import type { RenoDxGuidance } from '../model/types';
  import RenoDxGuidanceCallouts from './RenoDxGuidanceCallouts.svelte';

  type Props = {
    guidance: RenoDxGuidance[];
    presentation?: 'callouts' | 'engine-ini-dialog';
  };

  const { guidance, presentation }: Props = $props();
</script>

<TooltipProvider>
  <RenoDxGuidanceCallouts {guidance} {presentation} />
</TooltipProvider>
