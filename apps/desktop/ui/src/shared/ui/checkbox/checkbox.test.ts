/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount, tick, unmount } from 'svelte';

import Checkbox from './checkbox.svelte';

describe('Checkbox', () => {
  let target: HTMLDivElement;
  let component: object | undefined;

  beforeEach(() => {
    target = document.createElement('div');
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) {
      await unmount(component);
      component = undefined;
    }
    target.remove();
  });

  it('exposes checkbox semantics and reports checked changes', async () => {
    const onCheckedChange = vi.fn();
    component = mount(Checkbox, {
      target,
      props: { 'aria-label': 'Enable module', onCheckedChange },
    });
    const checkbox = target.querySelector<HTMLButtonElement>('[data-slot="checkbox"]');

    expect(checkbox).not.toBeNull();
    expect(checkbox?.getAttribute('role')).toBe('checkbox');
    expect(checkbox?.getAttribute('aria-checked')).toBe('false');

    checkbox?.click();
    await tick();

    expect(onCheckedChange).toHaveBeenCalledWith(true);
    expect(checkbox?.getAttribute('aria-checked')).toBe('true');
    expect(target.querySelector('[data-slot="checkbox-indicator"] svg')).not.toBeNull();
  });
});
