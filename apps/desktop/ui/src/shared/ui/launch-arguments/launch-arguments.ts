import type { MessageKeyWithoutParams } from '@shared/i18n';

const INSTRUCTION_KEY_BY_LAUNCHER: Record<string, MessageKeyWithoutParams | undefined> = {
  Steam: 'gameDetails.addon.launchArguments.instructions.steam',
  Gog: 'gameDetails.addon.launchArguments.instructions.gog',
  Epic: 'gameDetails.addon.launchArguments.instructions.epic',
  Ea: 'gameDetails.addon.launchArguments.instructions.ea',
  Ubisoft: 'gameDetails.addon.launchArguments.instructions.ubisoft',
};

export type LaunchArgumentsRequirement = 'required' | 'recommended';

export type LaunchArguments = {
  arguments: string[];
  requirement: LaunchArgumentsRequirement;
};

export function launchArgumentsInstructionKey(launcher: string): MessageKeyWithoutParams {
  return (
    INSTRUCTION_KEY_BY_LAUNCHER[launcher] ?? 'gameDetails.addon.launchArguments.instructions.other'
  );
}

export function hasKnownLaunchArgumentsInstructions(launcher: string): boolean {
  return INSTRUCTION_KEY_BY_LAUNCHER[launcher] !== undefined;
}

export function includesDx11LaunchArgument(launchArgs: readonly string[]): boolean {
  return launchArgs.some((argument) =>
    argument
      .trim()
      .split(/\s+/u)
      .some((token) => token.toLowerCase() === '-dx11'),
  );
}
