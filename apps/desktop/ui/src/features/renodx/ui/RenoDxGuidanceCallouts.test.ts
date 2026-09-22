/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import { clearAllNotifications, getActiveNotifications } from '@shared/notifications';
import { t } from '@shared/i18n';

import RenoDxGuidanceCalloutsTestHost from './RenoDxGuidanceCallouts.test-host.svelte';

describe('RenoDxGuidanceCallouts', () => {
  let target: HTMLDivElement;
  let component: object | undefined;
  const writeText = vi.fn<Navigator['clipboard']['writeText']>();

  beforeEach(() => {
    clearAllNotifications();
    target = document.createElement('div');
    document.body.append(target);
    writeText.mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        presentation: 'engine-ini-dialog',
        guidance: [
          {
            id: 'renodx.wukong.engine_ini',
            kind: 'engine_ini',
            message_id: 'renodx.wukong.engine_ini',
            fallback_text: 'Add this setting to Engine.ini for the HDR path.',
            code: 'r.HDR.EnableHDROutput=1',
            settings: [],
            url: null,
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

  it('renders UE Extended INI as a dedicated code block without changing bytes', () => {
    expect(target.querySelector('pre code')?.textContent).toBe('r.HDR.EnableHDROutput=1');
    expect(target.textContent).toContain('Add this setting to Engine.ini for the HDR path.');
    expect(target.querySelectorAll('pre code')).toHaveLength(1);
  });

  it('copies the exact INI payload', async () => {
    target.querySelector<HTMLButtonElement>('.relative button[aria-label]')?.click();

    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('r.HDR.EnableHDROutput=1');
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({
          severity: 'success',
          title: t('gameDetails.renodx.guidance.copied'),
        }),
      ]);
    });
  });

  it('renders structured setting names and values without flattening them into prose', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.test.setting',
            kind: 'addon_setting',
            message_id: 'renodx.test.setting',
            fallback_text: 'Use these RenoDX values.',
            code: null,
            settings: [{ name: 'Upgrade Path', value: 'Off' }],
            url: null,
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('dt')?.textContent).toBe('Upgrade Path');
    expect(target.querySelector('dd code')?.textContent).toBe('Off');
    expect(target.textContent).toContain(t('gameDetails.renodx.guidance.addonSetting'));
    expect(target.textContent).not.toContain('Use these RenoDX values.');
    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
  });

  it('renders in-game settings as a neutral block without an alert', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.test.native-hdr',
            kind: 'game_setting',
            message_id: 'renodx.test.native-hdr',
            fallback_text: 'Turn Native HDR off in the game.',
            code: null,
            settings: [{ name: 'Native HDR', value: 'Off' }],
            url: null,
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('dt')?.textContent).toBe('Native HDR');
    expect(target.querySelector('dd code')?.textContent).toBe('Off');
    expect(target.textContent).toContain(t('gameDetails.renodx.guidance.gameSetting'));
    expect(target.textContent).not.toContain('Turn Native HDR off in the game.');
    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
  });

  it('renders warning guidance with warning variant and suppresses the alert title', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.test.warning',
            kind: 'warning',
            message_id: 'renodx.test.warning',
            fallback_text: 'Disable in-game HDR before activating RenoDX.',
            code: null,
            settings: [],
            url: null,
          },
        ],
      },
    });
    flushSync();

    const alert = target.querySelector<HTMLDivElement>('[data-slot="alert"]');
    expect(alert?.getAttribute('role')).toBe('note');
    expect(alert?.className).toContain('text-warning');
    expect(target.querySelector('[data-slot="alert-title"]')).toBeNull();
    expect(target.textContent).toContain('Disable in-game HDR before activating RenoDX.');
  });

  it('renders plain compatibility advice as neutral information', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.page.hdr.disable-double-tonemapping',
            kind: 'compatibility',
            message_id: 'renodx.page.hdr.disable-double-tonemapping',
            fallback_text:
              'If the image looks washed out, disable Auto HDR and RTX HDR to avoid double tone mapping.',
            code: null,
            settings: [],
            url: null,
          },
        ],
      },
    });
    flushSync();

    const alert = target.querySelector<HTMLDivElement>('[data-slot="alert"]');
    expect(alert?.getAttribute('role')).toBe('status');
    expect(alert?.className).not.toContain('text-warning');
    expect(target.querySelector('[data-slot="alert-title"]')).toBeNull();
    expect(target.textContent).toContain('Auto HDR and RTX HDR');
  });

  it('renders external tool link with target blank and safe rel attributes', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.test.tool',
            kind: 'external_tool',
            message_id: 'renodx.test.tool',
            fallback_text: 'Download the required setup tool.',
            code: null,
            settings: [],
            url: 'https://example.com/tool',
          },
        ],
      },
    });
    flushSync();

    const link = target.querySelector<HTMLAnchorElement>('a');
    expect(link).not.toBeNull();
    expect(link?.href).toBe('https://example.com/tool');
    expect(link?.target).toBe('_blank');
    expect(link?.rel).toBe('noreferrer');
    expect(target.textContent).toContain('Download the required setup tool.');
    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
  });

  it('publishes error notification when clipboard write fails', async () => {
    writeText.mockRejectedValueOnce(new Error('clipboard permission denied'));

    target.querySelector<HTMLButtonElement>('.relative button[aria-label]')?.click();

    await vi.waitFor(() => {
      expect(getActiveNotifications()).toEqual([
        expect.objectContaining({
          severity: 'error',
          title: t('gameDetails.renodx.guidance.copyFailed'),
        }),
      ]);
    });
  });

  it('hides Engine.ini guidance before installation', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.preinstall.engine_ini',
            kind: 'engine_ini',
            message_id: 'renodx.preinstall.engine_ini',
            fallback_text: 'Do not show this before installation.',
            code: '[SystemSettings]\nr.AllowHDR=1',
            settings: [],
            url: null,
          },
          {
            id: 'renodx.preinstall.warning',
            kind: 'warning',
            message_id: 'renodx.preinstall.warning',
            fallback_text: 'Keep this warning visible.',
            code: null,
            settings: [],
            url: null,
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('pre')).toBeNull();
    expect(target.textContent).not.toContain('Do not show this before installation.');
    expect(target.textContent).toContain('Keep this warning visible.');
  });

  it('suppresses structured Engine.ini guidance in the normal callout view', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.automatic.engine_ini',
            kind: 'engine_ini',
            message_id: 'renodx.automatic.engine_ini',
            fallback_text: 'Automatic Engine.ini recipe.',
            code: '[SystemSettings]\nr.AllowHDR=1',
            settings: [],
            url: null,
          },
        ],
      },
    });
    flushSync();

    expect(target.querySelector('pre')).toBeNull();
    expect(target.textContent).not.toContain('Automatic Engine.ini recipe.');
  });

  it('renders external tool guidance in a neutral block with header and link', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        guidance: [
          {
            id: 'renodx.test.tool',
            kind: 'external_tool',
            message_id: 'renodx.test.tool',
            fallback_text: 'Requires Lyall fix.',
            code: null,
            settings: [],
            url: 'https://github.com/Lyall/UltrawideFix',
          },
        ],
      },
    });
    flushSync();

    expect(target.textContent).toContain(t('gameDetails.renodx.guidance.externalTool'));
    expect(target.textContent).toContain('Requires Lyall fix.');
    expect(target.querySelector('a')?.getAttribute('href')).toBe(
      'https://github.com/Lyall/UltrawideFix',
    );
    expect(target.querySelector('[data-slot="alert"]')).toBeNull();
  });

  it('renders Engine.ini guidance in the dialog view', async () => {
    if (component) {
      await unmount(component);
    }
    component = mount(RenoDxGuidanceCalloutsTestHost, {
      target,
      props: {
        presentation: 'engine-ini-dialog',
        guidance: [
          {
            id: 'renodx.inline.engine_ini',
            kind: 'engine_ini',
            message_id: 'renodx.inline.engine_ini',
            fallback_text: 'Review this Engine.ini value before applying again.',
            code: 'r.HDR.EnableHDROutput=1',
            settings: [],
            url: null,
          },
          {
            id: 'renodx.inline.warning',
            kind: 'warning',
            message_id: 'renodx.inline.warning',
            fallback_text: 'Keep this warning in the main guidance area.',
            code: null,
            settings: [],
            url: null,
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
