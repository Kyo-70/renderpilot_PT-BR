<script lang="ts">
  import { onDestroy, untrack } from 'svelte';

  import type { GameFileSafetyAssessment } from '@entities/game';

  import {
    createFileSafetyContext,
    type FileSafetyScope,
  } from './create-file-safety-context.svelte';

  let { initialGameId }: { initialGameId: string } = $props();
  let gameId = $state<string | null>(untrack(() => initialGameId));
  const context = createFileSafetyContext({ getGameId: () => gameId });

  export function replaceGameId(nextGameId: string): void {
    gameId = nextGameId;
  }

  export function requireTokens(scope: FileSafetyScope) {
    return context.requireTokens(scope);
  }

  export function requireInstallTokens(scope: FileSafetyScope) {
    return context.requireInstallTokens(scope);
  }

  export function resolveInstallConfirmation(accepted: boolean): void {
    context.resolveInstallConfirmation(accepted);
  }

  export function getInstallConfirmation() {
    return context.installConfirmation;
  }

  export function getAssessment(): GameFileSafetyAssessment | null {
    return context.assessment;
  }

  onDestroy(context.destroy);
</script>
