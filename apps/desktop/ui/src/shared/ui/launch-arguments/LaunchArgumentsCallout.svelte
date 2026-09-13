<script lang="ts">
  import CopyIcon from '@lucide/svelte/icons/copy';
  import InfoIcon from '@lucide/svelte/icons/info';
  import { t } from '@shared/i18n';
  import { copyWithFeedback } from '@shared/lib';

  import { Alert, AlertDescription, AlertTitle } from '../alert';
  import { Button } from '../button';
  import { Tooltip, TooltipContent, TooltipTrigger } from '../tooltip';
  import {
    hasKnownLaunchArgumentsInstructions,
    includesDx11LaunchArgument,
    launchArgumentsInstructionKey,
    type LaunchArguments,
  } from './launch-arguments';

  type Props = {
    launch: LaunchArguments | null;
    launcher: string;
  };

  const { launch, launcher }: Props = $props();

  const argumentsText = $derived(launch?.arguments.join(' ') ?? '');
  const title = $derived(
    launch?.requirement === 'recommended'
      ? t('gameDetails.addon.launchArguments.recommendedTitle')
      : launch && includesDx11LaunchArgument(launch.arguments)
        ? t('gameDetails.addon.launchArguments.dx11Title')
        : t('gameDetails.addon.launchArguments.requiredTitle'),
  );
  const copyStep = $derived(
    launch?.requirement === 'recommended'
      ? t('gameDetails.addon.launchArguments.copyRecommendedStep')
      : t('gameDetails.addon.launchArguments.copyRequiredStep'),
  );
  const instructions = $derived(t(launchArgumentsInstructionKey(launcher)));
  const hasKnownInstructions = $derived(hasKnownLaunchArgumentsInstructions(launcher));

  async function copyArguments(): Promise<void> {
    await copyWithFeedback(argumentsText, {
      copied: 'gameDetails.addon.launchArguments.copied',
      copyFailed: 'gameDetails.addon.launchArguments.copyFailed',
    });
  }
</script>

{#if launch && launch.arguments.length > 0}
  <Alert variant="default" size="sm" role="note">
    <InfoIcon aria-hidden="true" />
    <AlertTitle>{title}</AlertTitle>
    <AlertDescription>
      <ol class="mt-2 list-decimal space-y-2 ps-4">
        <li class="space-y-1.5">
          <span>{copyStep}</span>
          <div class="relative inline-flex max-w-full items-center">
            <code class="overflow-x-auto rounded-sm bg-muted py-1 ps-1.5 pe-8 text-xs"
              >{argumentsText}</code
            >
            <Tooltip>
              <TooltipTrigger
                type="button"
                onclick={copyArguments}
                aria-label={t('gameDetails.addon.launchArguments.copy')}
              >
                {#snippet child({ props })}
                  <Button
                    {...props}
                    variant="ghost"
                    size="icon"
                    class="absolute inset-e-0.5 size-6"
                  >
                    <CopyIcon class="size-3" aria-hidden="true" />
                  </Button>
                {/snippet}
              </TooltipTrigger>
              <TooltipContent>{t('gameDetails.addon.launchArguments.copy')}</TooltipContent>
            </Tooltip>
          </div>
        </li>
        <li class="space-y-1">
          <p>{instructions}</p>
          {#if hasKnownInstructions}
            <p class="text-xs text-muted-foreground">
              {t('gameDetails.addon.launchArguments.instructions.other')}
            </p>
          {/if}
        </li>
      </ol>
    </AlertDescription>
  </Alert>
{/if}
