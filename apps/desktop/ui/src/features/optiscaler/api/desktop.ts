import { invokeDesktop } from '@shared/api';
import { requireNonBlankString } from '@shared/validation';

import type {
  OptiScalerAvailability,
  OptiScalerOperationResult,
  OptiScalerUpdateCheck,
} from '../model/types';

const id = (gameId: string) => requireNonBlankString(gameId, 'gameId');

export const optiscalerApi = {
  availability: (gameId: string) =>
    invokeDesktop<OptiScalerAvailability>('get_optiscaler_availability', {
      gameId: id(gameId),
    }),
  install: (gameId: string, modules: string[], gameContextToken?: string) =>
    invokeDesktop<OptiScalerOperationResult>('install_optiscaler', {
      gameId: id(gameId),
      modules,
      ...(gameContextToken === undefined ? {} : { gameContextToken }),
    }),
  checkUpdate: (gameId: string) =>
    invokeDesktop<OptiScalerUpdateCheck>('check_optiscaler_update', {
      gameId: id(gameId),
    }),
  update: (gameId: string, gameContextToken?: string) =>
    invokeDesktop<OptiScalerOperationResult>('update_optiscaler', {
      gameId: id(gameId),
      ...(gameContextToken === undefined ? {} : { gameContextToken }),
    }),
  repair: (gameId: string, gameContextToken?: string) =>
    invokeDesktop<OptiScalerOperationResult>('repair_optiscaler', {
      gameId: id(gameId),
      ...(gameContextToken === undefined ? {} : { gameContextToken }),
    }),
  setModules: (gameId: string, modules: string[], gameContextToken?: string) =>
    invokeDesktop<OptiScalerOperationResult>('set_optiscaler_modules', {
      gameId: id(gameId),
      modules,
      ...(gameContextToken === undefined ? {} : { gameContextToken }),
    }),
  relocate: (gameId: string, targetExe: string, gameContextToken?: string) =>
    invokeDesktop<OptiScalerOperationResult>('relocate_optiscaler', {
      gameId: id(gameId),
      targetExe: requireNonBlankString(targetExe, 'targetExe'),
      ...(gameContextToken === undefined ? {} : { gameContextToken }),
    }),
  uninstall: (gameId: string) =>
    invokeDesktop<OptiScalerOperationResult>('uninstall_optiscaler', { gameId: id(gameId) }),
};

export type OptiScalerApi = typeof optiscalerApi;
