import { isMutationSuccess, type AddonMutationResult } from '@entities/addon';

import { selectedModuleIds } from './presentation';
import type { OptiScalerStore } from './create-optiscaler-store.svelte';

export type OptiScalerAction = 'install' | 'update' | 'repair' | 'modules' | 'relocate';

/** Owns transient action and module-selection state for the card. */
export function createOptiScalerCardController(gameId: () => string, store: OptiScalerStore) {
  let selectedModules = $state<string[]>([]);
  let settingsOpen = $state(false);
  let pendingModules = $state<string[] | null>(null);

  const report = $derived(store.report);
  const installed = $derived(report?.install.installed ?? false);

  $effect(() => {
    if (report) {
      selectedModules = selectedModuleIds(report);
    }
  });

  async function execute(action: OptiScalerAction): Promise<AddonMutationResult> {
    const current = report;
    if (!current) {
      return 'skipped';
    }
    switch (action) {
      case 'install':
        return store.install(gameId(), selectedModules);
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

  function requestAction(action: OptiScalerAction): void {
    if (!report) {
      return;
    }
    void execute(action);
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
    requestAction,
    saveModules,
  };
}
