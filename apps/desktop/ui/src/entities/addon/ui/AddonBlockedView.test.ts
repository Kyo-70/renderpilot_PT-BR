/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import AddonBlockedView from './AddonBlockedView.svelte';

describe('AddonBlockedView', () => {
  let target: HTMLDivElement;
  let component: object | undefined;

  const attribution = {
    textKey: 'gameDetails.renodx.attribution' as const,
    linkKey: 'gameDetails.renodx.attributionLink' as const,
    href: 'https://github.com/clshortfuse/renodx',
  };

  function render(
    props: {
      blockedAddon?: 'renodx' | 'luma';
      installedAddon?: 'renodx' | 'luma' | null;
      fallbackInstalledAddon?: 'renodx' | 'luma';
      unmanaged?: boolean;
      selfUnmanagedMessage?: string | null;
      installLabel?: string;
    } = {},
  ): void {
    component = mount(AddonBlockedView, {
      target,
      props: {
        blockedAddon: props.blockedAddon ?? 'renodx',
        installedAddon: props.installedAddon !== undefined ? props.installedAddon : 'luma',
        fallbackInstalledAddon: props.fallbackInstalledAddon ?? 'luma',
        unmanaged: props.unmanaged ?? false,
        selfUnmanagedMessage: props.selfUnmanagedMessage ?? null,
        attribution,
        installLabel: props.installLabel ?? 'Install',
      },
    });
    flushSync();
  }

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

  it('renders blocked message, attribution, and disabled install button with icon', () => {
    render();

    expect(target.textContent).toContain(
      'Luma is installed for this game — uninstall it before installing RenoDX.',
    );
    expect(target.textContent).toContain('RenoDX by clshortfuse.');

    const button = target.querySelector<HTMLButtonElement>('button');
    expect(button).not.toBeNull();
    expect(button?.disabled).toBe(true);
    expect(button?.textContent.trim()).toBe('Install');
    expect(button?.querySelector('svg')).not.toBeNull();
  });

  it('renders pinned footer', () => {
    render();

    const footer = target.querySelector('[data-slot="addon-card-footer"]');
    expect(footer).not.toBeNull();
    expect(footer?.className).toContain('mt-auto');
    expect(footer?.className).not.toContain('border-t');
  });

  it('prefers selfUnmanagedMessage when provided', () => {
    render({
      selfUnmanagedMessage: 'Luma unmanaged files found on disk.',
    });

    expect(target.textContent).toContain('Luma unmanaged files found on disk.');
  });
});
