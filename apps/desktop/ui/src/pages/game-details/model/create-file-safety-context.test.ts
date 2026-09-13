/**
 * @vitest-environment jsdom
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { flushSync, mount, unmount } from 'svelte';

import { registerPreviewInvoker, type DesktopInvoker } from '@shared/api-preview';

import FileSafetyContextTestHost from './create-file-safety-context.test-host.svelte';

describe('createFileSafetyContext', () => {
  let target: HTMLDivElement;
  let component: object | undefined;
  let disposeInvoker: (() => void) | undefined;

  beforeEach(() => {
    target = document.createElement('div');
    document.body.append(target);
  });

  afterEach(async () => {
    if (component) {
      await unmount(component);
    }
    disposeInvoker?.();
    document.body.replaceChildren();
  });

  it('returns fresh install authority immediately when no anti-cheat is detected', async () => {
    let calls = 0;
    const invoker = ((command: string) => {
      if (command === 'get_game_file_safety_assessment') {
        calls += 1;
        return Promise.resolve({
          game_id: 'game-a',
          context_token: `game-token-${calls}`,
          detected_engines: [],
          scan_completeness: 'complete',
        });
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    }) as DesktopInvoker;
    disposeInvoker = registerPreviewInvoker(invoker);

    component = mount(FileSafetyContextTestHost, {
      target,
      props: { initialGameId: 'game-a' },
    });
    const host = component as {
      requireInstallTokens(scope: 'game'): Promise<{ gameContextToken: string } | null>;
      getInstallConfirmation(): unknown;
    };
    await vi.waitFor(() => {
      expect(calls).toBe(1);
    });

    await expect(host.requireInstallTokens('game')).resolves.toEqual({
      gameContextToken: 'game-token-2',
    });
    expect(host.getInstallConfirmation()).toBeNull();
  });

  it('requires one explicit confirmation for a detected anti-cheat and preserves the tokens', async () => {
    let calls = 0;
    const invoker = ((command: string) => {
      if (command === 'get_game_file_safety_assessment') {
        calls += 1;
        return Promise.resolve({
          game_id: 'game-a',
          context_token: `game-token-${calls}`,
          detected_engines: ['EasyAntiCheat'],
          scan_completeness: 'complete',
        });
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    }) as DesktopInvoker;
    disposeInvoker = registerPreviewInvoker(invoker);

    component = mount(FileSafetyContextTestHost, {
      target,
      props: { initialGameId: 'game-a' },
    });
    const host = component as {
      requireInstallTokens(scope: 'game'): Promise<{ gameContextToken: string } | null>;
      resolveInstallConfirmation(accepted: boolean): void;
      getInstallConfirmation(): { detectedEngines: string[] } | null;
    };
    await vi.waitFor(() => {
      expect(calls).toBe(1);
    });

    const tokens = host.requireInstallTokens('game');
    await vi.waitFor(() => {
      expect(host.getInstallConfirmation()?.detectedEngines).toEqual(['EasyAntiCheat']);
    });
    host.resolveInstallConfirmation(true);

    await expect(tokens).resolves.toEqual({ gameContextToken: 'game-token-2' });
    expect(host.getInstallConfirmation()).toBeNull();
  });

  it('treats a rejected anti-cheat confirmation as a skipped install', async () => {
    const invoker = ((command: string) => {
      if (command === 'get_game_file_safety_assessment') {
        return Promise.resolve({
          game_id: 'game-a',
          context_token: 'game-token',
          detected_engines: ['BattlEye'],
          scan_completeness: 'complete',
        });
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    }) as DesktopInvoker;
    disposeInvoker = registerPreviewInvoker(invoker);

    component = mount(FileSafetyContextTestHost, {
      target,
      props: { initialGameId: 'game-a' },
    });
    const host = component as {
      requireInstallTokens(scope: 'game'): Promise<{ gameContextToken: string } | null>;
      resolveInstallConfirmation(accepted: boolean): void;
      getInstallConfirmation(): unknown;
    };

    const tokens = host.requireInstallTokens('game');
    await vi.waitFor(() => {
      expect(host.getInstallConfirmation()).not.toBeNull();
    });
    host.resolveInstallConfirmation(false);

    await expect(tokens).resolves.toBeNull();
  });

  it("loads a newly selected game without waiting for the previous game's stalled request", async () => {
    let gameACalls = 0;
    let gameBCalls = 0;
    let resolveGameB!: (assessment: {
      game_id: string;
      context_token: string;
      detected_engines: string[];
      scan_completeness: 'complete';
    }) => void;
    const gameBAssessment = new Promise<{
      game_id: string;
      context_token: string;
      detected_engines: string[];
      scan_completeness: 'complete';
    }>((resolve) => {
      resolveGameB = resolve;
    });
    const stalledGameA = new Promise<never>(() => undefined);
    const invoker = ((command: string, payload?: Record<string, unknown>) => {
      if (command === 'get_game_file_safety_assessment') {
        const gameId = (payload as { gameId?: unknown } | undefined)?.gameId;
        if (gameId === 'game-a') {
          gameACalls += 1;
          return stalledGameA;
        }
        if (gameId === 'game-b') {
          gameBCalls += 1;
          return gameBAssessment;
        }
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    }) as DesktopInvoker;
    disposeInvoker = registerPreviewInvoker(invoker);

    component = mount(FileSafetyContextTestHost, {
      target,
      props: { initialGameId: 'game-a' },
    });
    const host = component as {
      replaceGameId(gameId: string): void;
      requireTokens(scope: 'game'): Promise<{ gameContextToken: string }>;
      getAssessment(): { game_id: string } | null;
    };
    await vi.waitFor(() => {
      expect(gameACalls).toBe(1);
    });

    host.replaceGameId('game-b');
    flushSync();
    await vi.waitFor(() => {
      expect(gameBCalls).toBe(1);
    });
    const tokens = host.requireTokens('game');

    resolveGameB({
      game_id: 'game-b',
      context_token: 'game-b-token',
      detected_engines: [],
      scan_completeness: 'complete',
    });

    await expect(tokens).resolves.toEqual({ gameContextToken: 'game-b-token' });
    expect(host.getAssessment()?.game_id).toBe('game-b');
  });

  it('does not publish the previous game assessment after a delayed shared request', async () => {
    let resolveShared!: (assessment: { context_token: string }) => void;
    const sharedAssessment = new Promise<{ context_token: string }>((resolve) => {
      resolveShared = resolve;
    });
    let resolveGameB!: (assessment: {
      game_id: string;
      context_token: string;
      detected_engines: string[];
      scan_completeness: 'complete';
    }) => void;
    const gameBAssessment = new Promise<{
      game_id: string;
      context_token: string;
      detected_engines: string[];
      scan_completeness: 'complete';
    }>((resolve) => {
      resolveGameB = resolve;
    });
    let gameACalls = 0;
    const invoker = ((command: string, payload?: Record<string, unknown>) => {
      if (command === 'get_game_file_safety_assessment') {
        const gameId = (payload as { gameId?: unknown } | undefined)?.gameId;
        if (gameId === 'game-a') {
          gameACalls += 1;
          return Promise.resolve({
            game_id: 'game-a',
            context_token: `game-a-token-${gameACalls}`,
            detected_engines: ['EasyAntiCheat'],
            scan_completeness: 'complete',
          });
        }
        if (gameId === 'game-b') {
          return gameBAssessment;
        }
      }
      if (command === 'get_shared_vulkan_safety_assessment') {
        return sharedAssessment;
      }
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    }) as DesktopInvoker;
    disposeInvoker = registerPreviewInvoker(invoker);

    component = mount(FileSafetyContextTestHost, {
      target,
      props: { initialGameId: 'game-a' },
    });
    const host = component as {
      replaceGameId(gameId: string): void;
      requireTokens(scope: 'game_and_shared'): Promise<unknown>;
      getAssessment(): { game_id: string } | null;
    };
    await vi.waitFor(() => {
      expect(host.getAssessment()?.game_id).toBe('game-a');
    });

    const tokens = host.requireTokens('game_and_shared');
    await vi.waitFor(() => {
      expect(gameACalls).toBe(2);
    });
    host.replaceGameId('game-b');
    flushSync();
    resolveShared({ context_token: 'shared-token' });

    await expect(tokens).rejects.toMatchObject({ code: 'safety_context_scope_mismatch' });
    expect(host.getAssessment()).toBeNull();

    resolveGameB({
      game_id: 'game-b',
      context_token: 'game-b-token',
      detected_engines: [],
      scan_completeness: 'complete',
    });
    await vi.waitFor(() => {
      expect(host.getAssessment()?.game_id).toBe('game-b');
    });
  });
});
