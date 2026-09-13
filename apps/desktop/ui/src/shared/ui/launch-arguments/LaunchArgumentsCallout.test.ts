/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import { clearAllNotifications, getActiveNotifications } from '@shared/notifications';
import LaunchArgumentsCalloutTestHost from './LaunchArgumentsCallout.test-host.svelte';

const KNOWN_LAUNCHERS = ['Steam', 'Gog', 'Epic', 'Ea', 'Ubisoft'] as const;

describe('LaunchArgumentsCallout', () => {
  let target: HTMLDivElement;
  let component: object | undefined;
  const writeText = vi.fn<Navigator['clipboard']['writeText']>();

  function render(launcher: string, requirement: 'required' | 'recommended' = 'required'): void {
    component = mount(LaunchArgumentsCalloutTestHost, {
      target,
      props: { launch: { arguments: ['-dx11'], requirement }, launcher },
    });
    flushSync();
  }

  beforeEach(() => {
    clearAllNotifications();
    target = document.createElement('div');
    document.body.append(target);
    writeText.mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
  });

  afterEach(async () => {
    if (component) {
      await unmount(component);
      component = undefined;
    }
    target.remove();
    clearAllNotifications();
    vi.clearAllMocks();
  });

  it.each(KNOWN_LAUNCHERS)('shows the two-step instruction for %s', (launcher) => {
    render(launcher);

    expect(target.textContent).toContain('This add-on requires DirectX 11');
    expect(target.textContent).toContain('Copy the required launch arguments:');
    expect(target.textContent).toContain('-dx11');
    expect(target.textContent).toMatch(/If you start the game through/);
    expect(target.textContent).toContain('Use the launch method that actually starts the game.');
    expect(target.querySelector('button')?.getAttribute('aria-label')).toBe('Copy arguments');
  });

  it('uses neutral instructions for an unknown launcher and distinguishes recommendations', () => {
    render('Manual', 'recommended');

    expect(target.textContent).toContain('Recommended launch arguments');
    expect(target.textContent).toContain('Copy the recommended launch arguments:');
    expect(target.textContent).not.toContain('Copy the required launch arguments:');
    expect(target.textContent).toContain('Use the launch method that actually starts the game.');
    expect(target.textContent).not.toContain('If you start the game through Steam');
  });

  it('keeps copying arguments and reporting feedback independent from their requirement', async () => {
    render('Steam');

    const copyButton = target.querySelector<HTMLButtonElement>('button');
    copyButton?.click();
    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('-dx11');
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({ severity: 'success', title: 'Copied' }),
      ]);
    });

    writeText.mockRejectedValueOnce(new Error('clipboard unavailable'));
    clearAllNotifications();
    copyButton?.click();
    await vi.waitFor(() => {
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({
          severity: 'error',
          title: 'Could not copy the launch arguments',
        }),
      ]);
    });
  });
});
