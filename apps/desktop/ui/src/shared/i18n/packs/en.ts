import { en } from '../messages/en';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages } from '../messages/external';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import type { LocalePack } from './types';

export const enPack = {
  locale: 'en',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: en,
  externalMessages: bindExternalMessages(OPTISCALER_SOURCE_CATALOG, OPTISCALER_SOURCE_CATALOG),
} as const satisfies LocalePack;

export default enPack;
