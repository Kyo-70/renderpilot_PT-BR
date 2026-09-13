import { describe, expect, it } from 'vitest';

import {
  hasKnownLaunchArgumentsInstructions,
  includesDx11LaunchArgument,
  launchArgumentsInstructionKey,
} from './launch-arguments';

describe('launch argument presentation', () => {
  it.each([
    ['Steam', 'gameDetails.addon.launchArguments.instructions.steam'],
    ['Gog', 'gameDetails.addon.launchArguments.instructions.gog'],
    ['Epic', 'gameDetails.addon.launchArguments.instructions.epic'],
    ['Ea', 'gameDetails.addon.launchArguments.instructions.ea'],
    ['Ubisoft', 'gameDetails.addon.launchArguments.instructions.ubisoft'],
  ])('uses the %s instruction', (launcher, expected) => {
    expect(launchArgumentsInstructionKey(launcher)).toBe(expected);
  });

  it('uses the neutral instruction for an unknown launcher', () => {
    expect(launchArgumentsInstructionKey('Manual')).toBe(
      'gameDetails.addon.launchArguments.instructions.other',
    );
    expect(hasKnownLaunchArgumentsInstructions('Steam')).toBe(true);
    expect(hasKnownLaunchArgumentsInstructions('Manual')).toBe(false);
  });

  it('recognises a standalone DirectX 11 token without changing its configuration', () => {
    expect(includesDx11LaunchArgument(['-NoD3D9Ex', ' -DX11 '])).toBe(true);
    expect(includesDx11LaunchArgument(['-oss=Steam -dx11'])).toBe(true);
    expect(includesDx11LaunchArgument(['-NoD3D9Ex'])).toBe(false);
    expect(includesDx11LaunchArgument(['--foo=-dx11'])).toBe(false);
  });
});
