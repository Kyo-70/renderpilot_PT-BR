import { isMutationSuccess, type AddonMutationResult } from '@entities/addon';
import { t } from '@shared/i18n';

import { selectedModuleIds } from './presentation';
import type { OptiScalerStore } from './create-optiscaler-store.svelte';

export type OptiScalerConfirmAction = 'install' | 'update' | 'repair' | 'modules' | 'relocate';

/** Owns transient action, module-selection and confirmation state for the card. */
export function createOptiScalerCardController(gameId: () => string, store: OptiScalerStore) {
  let selectedModules = $state<string[]>([]);
  let settingsOpen = $state(false);
  let confirmOpen = $state(false);
  let confirmAction = $state<OptiScalerConfirmAction>('install');
  let pendingModules = $state<string[] | null>(null);

  const report = $derived(store.report);
  const installed = $derived(report?.install.installed ?? false);
  const confirmTitle = $derived(t('gameDetails.optiscaler.confirmCompatibilityTitle'));
  const confirmDescription = $derived(t('gameDetails.optiscaler.confirmCompatibilityBody'));
  const confirmWarning = $derived(t('gameDetails.optiscaler.confirmCompatibilityWarning'));
  const confirmLabel = $derived(t('gameDetails.optiscaler.confirmInstallAnyway'));

  $effect(() => {
    if (report) {
      selectedModules = selectedModuleIds(report);
    }
  });

  async function execute(
    action: OptiScalerConfirmAction,
    confirmed = false,
  ): Promise<AddonMutationResult> {
    const current = report;
    if (!current) {
      return 'skipped';
    }
    switch (action) {
      case 'install':
        return store.install(
          gameId(),
          selectedModules,
          confirmed && current.eligibility.manual_override,
        );
      case 'update':
        return store.update(gameId());
      case 'repair':
        return store.repair(gameId());
      case 'modules':
        return store.setModules(gameId(), pendingModules ?? selectedModules);
      case 'relocate':
        return current.relocation
          ? store.relocate(gameId(), current.relocation.target_exe)
          : 'skipped';
    }
  }

  function requestAction(action: OptiScalerConfirmAction): void {
    const current = report;
    if (!current) {
      return;
    }
    const needsConfirmation = action === 'install' && current.eligibility.manual_override;
    if (needsConfirmation) {
      confirmAction = action;
      confirmOpen = true;
      return;
    }
    void execute(action);
  }

  async function confirmPendingAction(): Promise<void> {
    const action = confirmAction;
    const modules = pendingModules;
    confirmOpen = false;
    pendingModules = null;
    const result = await execute(action, true);
    if (isMutationSuccess(result)) {
      if (action === 'modules' && modules) {
        selectedModules = modules;
      }
    }
  }

  async function saveModules(next: string[]): Promise<boolean> {
    if (!installed) {
      selectedModules = next;
      return true;
    }
    pendingModules = next;
    const result = await execute('modules');
    if (isMutationSuccess(result)) {
      selectedModules = next;
      pendingModules = null;
      return true;
    }
    return false;
  }

  return {
    get selectedModules() {
      return selectedModules;
    },
    get settingsOpen() {
      return settingsOpen;
    },
    set settingsOpen(value: boolean) {
      settingsOpen = value;
    },
    get confirmOpen() {
      return confirmOpen;
    },
    set confirmOpen(value: boolean) {
      confirmOpen = value;
      if (!value) {
        pendingModules = null;
      }
    },
    get confirmTitle() {
      return confirmTitle;
    },
    get confirmDescription() {
      return confirmDescription;
    },
    get confirmWarning() {
      return confirmWarning;
    },
    get confirmLabel() {
      return confirmLabel;
    },
    requestAction,
    confirmPendingAction,
    saveModules,
  };
}
