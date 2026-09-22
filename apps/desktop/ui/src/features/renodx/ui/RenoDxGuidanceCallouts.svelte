<script lang="ts">
  import CopyIcon from '@lucide/svelte/icons/copy';
  import FileCode2Icon from '@lucide/svelte/icons/file-code-2';
  import InfoIcon from '@lucide/svelte/icons/info';
  import Settings2Icon from '@lucide/svelte/icons/settings-2';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
  import WrenchIcon from '@lucide/svelte/icons/wrench';
  import { t, translateExternalMessage, type MessageKeyWithoutParams } from '@shared/i18n';
  import { copyWithFeedback } from '@shared/lib';
  import {
    Alert,
    AlertDescription,
    AlertTitle,
    Button,
    Tooltip,
    TooltipContent,
    TooltipTrigger,
  } from '@shared/ui';

  import { AddonStateMessage } from '@entities/addon';
  import type { RenoDxGuidance, RenoDxGuidanceKind } from '../model/types';

  type Props = {
    guidance?: RenoDxGuidance[];
    presentation?: 'callouts' | 'engine-ini-dialog';
  };

  const { guidance = [], presentation = 'callouts' }: Props = $props();

  const visibleGuidance = $derived(
    guidance.filter((item) =>
      presentation === 'engine-ini-dialog'
        ? item.kind === 'engine_ini'
        : item.kind !== 'engine_ini',
    ),
  );

  const TITLE_KEYS = {
    game_setting: 'gameDetails.renodx.guidance.gameSetting',
    addon_setting: 'gameDetails.renodx.guidance.addonSetting',
    engine_ini: 'gameDetails.renodx.guidance.engineIni',
    warning: 'gameDetails.renodx.guidance.warning',
    compatibility: 'gameDetails.renodx.guidance.compatibility',
    external_tool: 'gameDetails.renodx.guidance.externalTool',
  } as const satisfies Record<RenoDxGuidanceKind, MessageKeyWithoutParams>;

  const ICONS = {
    game_setting: Settings2Icon,
    addon_setting: Settings2Icon,
    engine_ini: FileCode2Icon,
    warning: TriangleAlertIcon,
    compatibility: InfoIcon,
    external_tool: WrenchIcon,
  } as const;

  function textFor(item: RenoDxGuidance): string {
    return translateExternalMessage({
      key: item.message_id || item.id,
      fallback: item.fallback_text,
    });
  }

  function isPlainCompatibility(item: RenoDxGuidance): boolean {
    return item.kind === 'compatibility' && !item.code && item.settings.length === 0 && !item.url;
  }

  async function copy(item: RenoDxGuidance): Promise<void> {
    if (!item.code) {
      return;
    }

    await copyWithFeedback(item.code, {
      copied: 'gameDetails.renodx.guidance.copied',
      copyFailed: 'gameDetails.renodx.guidance.copyFailed',
    });
  }
</script>

{#snippet guidanceContent(item: RenoDxGuidance)}
  {#if !(item.settings.length > 0 && (item.kind === 'game_setting' || item.kind === 'addon_setting'))}
    <span>{textFor(item)}</span>
  {/if}
  {#if item.settings.length > 0}
    <dl class="grid gap-1.5 text-xs">
      {#each item.settings as setting (`${item.id}:${setting.name}`)}
        <div
          class="flex min-w-0 items-baseline justify-between gap-3 rounded-sm bg-muted px-2 py-1"
        >
          <dt class="truncate text-muted-foreground">{setting.name}</dt>
          <dd><code>{setting.value}</code></dd>
        </div>
      {/each}
    </dl>
  {/if}
  {#if item.code}
    <div class="relative min-w-0">
      <pre class="overflow-x-auto rounded-sm bg-muted p-2 pe-10 text-xs"><code>{item.code}</code
        ></pre>
      <Tooltip>
        <TooltipTrigger
          type="button"
          onclick={() => copy(item)}
          aria-label={t('gameDetails.renodx.guidance.copy')}
        >
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon" class="absolute inset-e-1 top-1 size-6">
              <CopyIcon class="size-3" aria-hidden="true" />
            </Button>
          {/snippet}
        </TooltipTrigger>
        <TooltipContent>{t('gameDetails.renodx.guidance.copy')}</TooltipContent>
      </Tooltip>
    </div>
  {/if}
  {#if item.url}
    <a
      href={item.url}
      target="_blank"
      rel="noreferrer"
      class="mt-2 block w-fit text-sm font-medium underline underline-offset-2"
    >
      {t('gameDetails.renodx.guidance.openLink')}
    </a>
  {/if}
{/snippet}

{#each visibleGuidance as item (item.id)}
  {#if presentation === 'engine-ini-dialog'}
    <div class="grid w-full gap-2 text-sm">
      <!-- eslint-disable-next-line @typescript-eslint/no-confusing-void-expression -->
      {@render guidanceContent(item)}
    </div>
  {:else if item.settings.length > 0 && (item.kind === 'game_setting' || item.kind === 'addon_setting')}
    {@const Icon = ICONS[item.kind]}
    <div class="grid w-full gap-2 rounded-md border border-border/60 bg-muted/30 p-2.5 text-sm">
      <div class="flex items-center gap-1.5 text-xs font-medium tracking-tight text-foreground">
        <Icon class="size-3.5 text-muted-foreground" aria-hidden="true" />
        <span>{t(TITLE_KEYS[item.kind])}</span>
      </div>
      <!-- eslint-disable-next-line @typescript-eslint/no-confusing-void-expression -->
      {@render guidanceContent(item)}
    </div>
  {:else if isPlainCompatibility(item) || item.kind === 'game_setting' || item.kind === 'addon_setting'}
    <AddonStateMessage tone="default" icon="info" message={textFor(item)} />
  {:else if item.kind === 'external_tool'}
    {@const Icon = ICONS[item.kind]}
    <div class="grid w-full gap-2 rounded-md border border-border/60 bg-muted/30 p-2.5 text-sm">
      <div class="flex items-center gap-1.5 text-xs font-medium tracking-tight text-foreground">
        <Icon class="size-3.5 text-muted-foreground" aria-hidden="true" />
        <span>{t(TITLE_KEYS[item.kind])}</span>
      </div>
      <!-- eslint-disable-next-line @typescript-eslint/no-confusing-void-expression -->
      {@render guidanceContent(item)}
    </div>
  {:else}
    {@const Icon = ICONS[item.kind]}
    <Alert variant={item.kind === 'warning' ? 'warning' : 'default'} size="sm" role="note">
      <Icon aria-hidden="true" />
      {#if item.kind !== 'warning'}
        <AlertTitle>{t(TITLE_KEYS[item.kind])}</AlertTitle>
      {/if}
      <AlertDescription>
        <!-- eslint-disable-next-line @typescript-eslint/no-confusing-void-expression -->
        {@render guidanceContent(item)}
      </AlertDescription>
    </Alert>
  {/if}
{/each}
