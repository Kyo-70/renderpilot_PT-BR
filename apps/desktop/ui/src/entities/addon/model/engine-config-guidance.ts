import type { EngineConfigStatus } from './types';

const ENGINE_CONFIG_COMPONENT_STATUSES: ReadonlySet<EngineConfigStatus> = new Set([
  'manual_only',
  'pending_first_launch',
  'ready',
  'configured',
  'needs_repair',
  'conflict',
  'recovery_required',
]);

/** Installed lifecycle states that belong in the component group. */
export function shouldShowEngineConfigRow(status?: EngineConfigStatus): boolean {
  return status !== undefined && ENGINE_CONFIG_COMPONENT_STATUSES.has(status);
}
