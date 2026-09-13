<script lang="ts">
  import type { Snippet } from 'svelte';

  import AddonCardFooter from './AddonCardFooter.svelte';
  import AddonSignalBadge from './AddonSignalBadge.svelte';
  import AddonStateMessage from './AddonStateMessage.svelte';
  import { t, type MessageKeyWithoutParams } from '@shared/i18n';
  import { Button, Spinner } from '@shared/ui';
  import DownloadIcon from '@lucide/svelte/icons/download';

  import type { MatchConfidence } from '../model/types';
  import type { AddonInstallableLabels } from '../model/presenters';
  import { actionDisabledMessage } from '../model/presenters';
  import type { AddonStoreView } from '../model/store-view';

  type ViewStore = Pick<
    AddonStoreView,
    'busy' | 'isInstallable' | 'confidence' | 'hostDetection' | 'hostFacts' | 'hostActions'
  >;

  type Props = {
    gameId: string;
    store: ViewStore;
    busy: boolean;
    labels: AddonInstallableLabels;
    confidenceLabelKey: Record<MatchConfidence, MessageKeyWithoutParams>;
    onInstall: (gameId: string) => void;
    preConflictWarnings?: Snippet;
    midCallouts?: Snippet;
    actionRowLeading?: Snippet;
    confidenceTrailing?: Snippet;
  };

  const {
    gameId,
    store,
    busy,
    labels,
    confidenceLabelKey,
    onInstall,
    preConflictWarnings,
    midCallouts,
    actionRowLeading,
    confidenceTrailing,
  }: Props = $props();

  const hostConflict = $derived(store.hostDetection === 'conflict');
  const customBuild = $derived(store.hostFacts.is_custom_build);

  const installAction = $derived(store.hostActions.install);
  const installDisabledByHost = $derived(installAction?.enabled === false);
  // When the host-conflict banner already explains the block, do not also show
  // the raw humanized `blocked_by_conflict` reason (duplicate warning).
  const installDisabledMessage = $derived.by((): string => {
    const message = actionDisabledMessage(installAction) ?? '';
    if (!message) {
      return '';
    }
    if (
      store.hostDetection === 'conflict' &&
      installAction?.disabled_reason === 'blocked_by_conflict'
    ) {
      return '';
    }
    return message;
  });

  const installBlocked = $derived(installDisabledByHost || customBuild);
  const canStartInstall = $derived(store.isInstallable && !busy && !installBlocked);

  const installLabel = $derived(store.busy ? t(labels.installing) : t(labels.installAction));

  function startInstall(): void {
    if (!canStartInstall) {
      return;
    }

    onInstall(gameId);
  }
</script>

<div class="flex w-full flex-1 flex-col gap-3">
  {#if store.confidence}
    <div class="flex flex-wrap items-center gap-2">
      <AddonSignalBadge
        tone={store.confidence}
        fieldLabel={t(labels.confidenceLabel)}
        valueLabel={t(confidenceLabelKey[store.confidence])}
      />
      {@render confidenceTrailing?.()}
    </div>
  {/if}

  {@render preConflictWarnings?.()}

  {#if customBuild}
    <AddonStateMessage tone="warning" icon="warning" message={t(labels.hostCustomBuild)} />
  {:else if hostConflict}
    <AddonStateMessage
      tone="warning"
      icon="warning"
      message={t(labels.hostConflictBlocksInstall)}
    />
  {/if}

  {@render midCallouts?.()}

  {#if installDisabledMessage}
    <AddonStateMessage tone="warning" icon="warning" message={installDisabledMessage} />
  {/if}

  <AddonCardFooter>
    {#snippet leading()}
      {@render actionRowLeading?.()}
    {/snippet}

    {#snippet actions()}
      {#if installBlocked}
        <Button type="button" size="sm" disabled>
          <DownloadIcon class="size-4" aria-hidden="true" />
          {t(labels.installAction)}
        </Button>
      {:else}
        <Button type="button" size="sm" disabled={busy} onclick={startInstall}>
          {#if store.busy}
            <Spinner class="size-4" />
          {:else}
            <DownloadIcon class="size-4" aria-hidden="true" />
          {/if}

          {installLabel}
        </Button>
      {/if}
    {/snippet}
  </AddonCardFooter>
</div>
