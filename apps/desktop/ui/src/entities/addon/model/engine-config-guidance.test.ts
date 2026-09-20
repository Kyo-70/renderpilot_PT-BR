import { describe, expect, it } from 'vitest';

import { shouldShowEngineConfigRow } from './engine-config-guidance';

describe('shouldShowEngineConfigRow', () => {
  it('places installed managed and manual states in the component group', () => {
    expect(shouldShowEngineConfigRow('ready')).toBe(true);
    expect(shouldShowEngineConfigRow('configured')).toBe(true);
    expect(shouldShowEngineConfigRow('needs_repair')).toBe(true);
    expect(shouldShowEngineConfigRow('manual_only')).toBe(true);
    expect(shouldShowEngineConfigRow('pending_first_launch')).toBe(true);

    for (const status of [undefined, 'not_applicable'] as const) {
      expect(shouldShowEngineConfigRow(status)).toBe(false);
    }

    expect(shouldShowEngineConfigRow('conflict')).toBe(true);
    expect(shouldShowEngineConfigRow('recovery_required')).toBe(true);
  });
});
