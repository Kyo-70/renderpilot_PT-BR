<script lang="ts">
  import { cn } from '@shared/classnames';
  import { t, type MessageKeyWithoutParams } from '@shared/i18n';
  import {
    Badge,
    Button,
    Checkbox,
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
    Label,
  } from '@shared/ui';

  import { optiscalerControlId, toggleSelectedModule } from '../model/presentation';
  import type { OptiScalerAvailability } from '../model/types';

  type SaveResult = boolean | undefined;
  type Props = {
    open: boolean;
    gameId: string;
    report: OptiScalerAvailability;
    selected: string[];
    busy: boolean;
    onOpenChange: (open: boolean) => void;
    onSave: (selected: string[]) => SaveResult | Promise<SaveResult>;
  };

  const { open, gameId, report, selected, busy, onOpenChange, onSave }: Props = $props();
  let draft = $state<string[]>([]);
  let saving = $state(false);

  const moduleKeys: Partial<
    Record<string, { name: MessageKeyWithoutParams; description: MessageKeyWithoutParams }>
  > = {
    core: {
      name: 'gameDetails.optiscaler.module.core.name',
      description: 'gameDetails.optiscaler.module.core.description',
    },
    ffx_dx12: {
      name: 'gameDetails.optiscaler.module.ffxDx12.name',
      description: 'gameDetails.optiscaler.module.ffxDx12.description',
    },
    ffx_vulkan: {
      name: 'gameDetails.optiscaler.module.ffxVulkan.name',
      description: 'gameDetails.optiscaler.module.ffxVulkan.description',
    },
    agility: {
      name: 'gameDetails.optiscaler.module.agility.name',
      description: 'gameDetails.optiscaler.module.agility.description',
    },
    fakenvapi: {
      name: 'gameDetails.optiscaler.module.fakeNvapi.name',
      description: 'gameDetails.optiscaler.module.fakeNvapi.description',
    },
    nvngx_fsr3_bridge: {
      name: 'gameDetails.optiscaler.module.fsr3Bridge.name',
      description: 'gameDetails.optiscaler.module.fsr3Bridge.description',
    },
    xess: {
      name: 'gameDetails.optiscaler.module.xess.name',
      description: 'gameDetails.optiscaler.module.xess.description',
    },
    optipatcher: {
      name: 'gameDetails.optiscaler.module.optipatcher.name',
      description: 'gameDetails.optiscaler.module.optipatcher.description',
    },
    nvidia_sr: {
      name: 'gameDetails.optiscaler.module.nvidiaSr.name',
      description: 'gameDetails.optiscaler.module.nvidiaSr.description',
    },
  };

  $effect(() => {
    if (open) {
      draft = [...selected];
    }
  });

  function moduleName(id: string): string {
    const keys = moduleKeys[id];
    return keys ? t(keys.name) : id;
  }

  function moduleDescription(id: string, fallback: string): string {
    const keys = moduleKeys[id];
    return keys ? t(keys.description) : fallback;
  }

  async function save(): Promise<void> {
    if (busy || saving) {
      return;
    }
    saving = true;
    try {
      const result = await onSave(draft);
      if (result !== false) {
        onOpenChange(false);
      }
    } finally {
      saving = false;
    }
  }
</script>

<Dialog
  {open}
  onOpenChange={(nextOpen: boolean) => {
    if (!saving) {
      onOpenChange(nextOpen);
    }
  }}
>
  <DialogContent
    closeLabel={t('common.close')}
    class="max-h-[calc(100vh-2rem)] overflow-y-auto sm:max-w-2xl"
  >
    <DialogHeader>
      <DialogTitle>{t('gameDetails.optiscaler.settingsTitle')}</DialogTitle>
      <DialogDescription>{t('gameDetails.optiscaler.settingsDescription')}</DialogDescription>
    </DialogHeader>

    <div class="divide-y rounded-lg border">
      {#each report.modules as module (module.id)}
        <Label
          for={optiscalerControlId(gameId, `module-${module.id}`)}
          class={cn(
            'items-start gap-3 p-3 font-normal',
            !module.available && !draft.includes(module.id) && 'opacity-50',
          )}
        >
          <Checkbox
            id={optiscalerControlId(gameId, `module-${module.id}`)}
            checked={draft.includes(module.id)}
            disabled={busy ||
              saving ||
              !module.optional ||
              (!module.available && !draft.includes(module.id))}
            onCheckedChange={(checked: boolean) => {
              draft = toggleSelectedModule(report.modules, draft, module.id, checked);
            }}
          />
          <span class="min-w-0 flex-1">
            <span class="flex flex-wrap items-center gap-2 font-medium">
              {moduleName(module.id)}
              {#if !module.optional}
                <Badge variant="secondary">{t('gameDetails.optiscaler.moduleRequired')}</Badge>
              {/if}
              {#if !module.available}
                <Badge variant="outline">
                  {t('gameDetails.optiscaler.compatibilityUnavailable')}
                </Badge>
              {/if}
            </span>
            <span class="mt-1 block text-xs/relaxed text-muted-foreground">
              {moduleDescription(module.id, module.description)}
            </span>
          </span>
        </Label>
      {/each}
    </div>

    <DialogFooter>
      <Button
        type="button"
        variant="secondary"
        size="sm"
        disabled={busy || saving}
        onclick={() => {
          onOpenChange(false);
        }}
      >
        {t('common.cancel')}
      </Button>
      <Button type="button" size="sm" disabled={busy || saving} onclick={save}>
        {t('gameDetails.optiscaler.settingsSave')}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
