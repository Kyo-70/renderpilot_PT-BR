<script lang="ts">
  import DownloadIcon from '@lucide/svelte/icons/download';
  import { Button } from '@shared/ui';

  import type { AddonKind, ExclusiveAddonKind } from '@shared/model';
  import AddonAttribution from './AddonAttribution.svelte';
  import AddonBlockedMessage from './AddonBlockedMessage.svelte';
  import AddonCardFooter from './AddonCardFooter.svelte';
  import type { AddonAttributionProps } from './types';

  type Props = {
    blockedAddon: ExclusiveAddonKind;
    installedAddon: AddonKind | null;
    fallbackInstalledAddon: ExclusiveAddonKind;
    unmanaged?: boolean;
    selfUnmanagedMessage?: string | null;
    attribution: AddonAttributionProps;
    installLabel: string;
  };

  const {
    blockedAddon,
    installedAddon,
    fallbackInstalledAddon,
    unmanaged = false,
    selfUnmanagedMessage = null,
    attribution,
    installLabel,
  }: Props = $props();
</script>

<div class="flex w-full flex-1 flex-col gap-4">
  <AddonBlockedMessage
    {blockedAddon}
    {installedAddon}
    {fallbackInstalledAddon}
    {unmanaged}
    {selfUnmanagedMessage}
  />

  <AddonCardFooter>
    {#snippet leading()}
      <AddonAttribution {...attribution} />
    {/snippet}

    {#snippet actions()}
      <Button type="button" size="sm" disabled>
        <DownloadIcon class="size-4" aria-hidden="true" />
        {installLabel}
      </Button>
    {/snippet}
  </AddonCardFooter>
</div>
