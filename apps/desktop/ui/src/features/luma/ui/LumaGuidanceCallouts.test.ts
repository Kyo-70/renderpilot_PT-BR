/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import { setLanguageMode } from '@shared/i18n';
import { clearAllNotifications, getActiveNotifications } from '@shared/notifications';

import LumaGuidanceCalloutsTestHost from './LumaGuidanceCallouts.test-host.svelte';

describe('LumaGuidanceCallouts', () => {
  let target: HTMLDivElement;
  let component: object | undefined;
  const writeText = vi.fn<Navigator['clipboard']['writeText']>();

  beforeEach(() => {
    clearAllNotifications();
    target = document.createElement('div');
    document.body.append(target);
    writeText.mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    component = mount(LumaGuidanceCalloutsTestHost, {
      target,
      props: {
        presentation: 'engine-ini-dialog',
        guidance: [
          {
            id: 'test.ini',
            kind: 'engine_ini',
            fallback_text: 'Set anti-aliasing in Engine.ini.',
            code: '[SystemSettings]\nr.DefaultFeature.AntiAliasing=2',
          },
          {
            id: 'test.warning',
            kind: 'warning',
            fallback_text: 'Do not combine this profile with OptiScaler.',
          },
          {
            id: 'luma.ace-combat-7.engine_ini',
            kind: 'engine_ini',
            fallback_text: 'Apply the following settings manually in Engine.ini.',
            code: '[SystemSettings]\nr.DefaultFeature.AntiAliasing=2',
          },
        ],
      },
    });
    flushSync();
  });

  afterEach(async () => {
    if (component) {
      await unmount(component);
    }
    component = undefined;
    target.remove();
    clearAllNotifications();
    vi.clearAllMocks();
  });

  it('uses reviewed fallback text and a copyable code block', () => {
    expect(target.textContent).toContain('Set anti-aliasing in Engine.ini.');
    expect(target.textContent).toContain('[SystemSettings]');
    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
  });

  it('renders a localized manifest guidance entry without changing its code', async () => {
    await setLanguageMode('ru');
    flushSync();

    expect(target.textContent).toContain('Вручную добавьте в Engine.ini следующие настройки.');
    expect(target.textContent).toContain('[SystemSettings]\nr.DefaultFeature.AntiAliasing=2');
  });

  it('keeps the copy button name stable while notification feedback reports success and failure', async () => {
    const copyButton = target.querySelector<HTMLButtonElement>('button');
    expect(copyButton?.getAttribute('aria-label')).toBe('Copy');
    expect(copyButton?.textContent).toBe('');
    copyButton?.click();

    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('[SystemSettings]\nr.DefaultFeature.AntiAliasing=2');
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({ severity: 'success', title: 'Copied' }),
      ]);
    });
    expect(copyButton?.getAttribute('aria-label')).toBe('Copy');
    expect(target.querySelector('[role="status"]')).toBeNull();

    writeText.mockRejectedValueOnce(new Error('clipboard unavailable'));
    clearAllNotifications();
    copyButton?.click();

    await vi.waitFor(() => {
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({
          severity: 'error',
          title: 'Could not copy',
          important: undefined,
        }),
      ]);
    });
    expect(copyButton?.getAttribute('aria-label')).toBe('Copy');
  });

  it('hides Engine.ini guidance before installation', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(LumaGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'luma.preinstall.engine_ini',
            kind: 'engine_ini',
            fallback_text: 'Do not show this before installation.',
            code: '[SystemSettings]\nr.DefaultFeature.AntiAliasing=2',
          },
          {
            id: 'luma.preinstall.warning',
            kind: 'warning',
            fallback_text: 'Keep this warning visible.',
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('pre')).toBeNull();
    expect(target.textContent).not.toContain('Do not show this before installation.');
    expect(target.textContent).toContain('Keep this warning visible.');
  });

  it('renders plain compatibility advice as neutral information', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(LumaGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'luma.test.compatibility',
            kind: 'compatibility',
            fallback_text: 'Disable other HDR processing to avoid double tone mapping.',
          },
        ],
      },
    });
    flushSync();

    const alert = target.querySelector<HTMLDivElement>('[data-slot="alert"]');
    expect(alert?.getAttribute('role')).toBe('status');
    expect(alert?.className).not.toContain('text-warning');
    expect(target.querySelector('[data-slot="alert-title"]')).toBeNull();
    expect(target.textContent).toContain('Disable other HDR processing');
  });

  it('suppresses structured Engine.ini guidance in the normal callout view', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(LumaGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'luma.automatic.engine_ini',
            kind: 'engine_ini',
            fallback_text: 'Automatic Engine.ini recipe.',
            code: '[SystemSettings]\nr.DefaultFeature.AntiAliasing=2',
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('pre')).toBeNull();
    expect(target.textContent).not.toContain('Automatic Engine.ini recipe.');
  });

  it('renders Engine.ini guidance in the dialog view', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(LumaGuidanceCalloutsTestHost, {
      target,
      props: {
        presentation: 'engine-ini-dialog',
        guidance: [
          {
            id: 'luma.inline.engine_ini',
            kind: 'engine_ini',
            fallback_text: 'Review this Engine.ini value before applying again.',
            code: 'r.DefaultFeature.AntiAliasing=2',
          },
          {
            id: 'luma.inline.warning',
            kind: 'warning',
            fallback_text: 'Keep this warning in the main guidance area.',
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
    expect(target.querySelectorAll('pre code')).toHaveLength(1);
    expect(target.textContent).toContain('Review this Engine.ini value before applying again.');
    expect(target.textContent).not.toContain('Keep this warning in the main guidance area.');
  });
});
