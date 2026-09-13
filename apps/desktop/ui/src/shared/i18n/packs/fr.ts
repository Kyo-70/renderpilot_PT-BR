import { fr } from '../messages/fr';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages, mergeExternalMessages } from '../messages/external';
import { lumaOverrides } from '../messages/overrides/luma/fr';
import { LUMA_SOURCE_CATALOG } from '../messages/overrides/luma/contract.generated';
import { NVAPI_SOURCE_CATALOG } from '../messages/overrides/nvapi/contract.generated';
import { nvapiOverrides } from '../messages/overrides/nvapi/fr';
import { optiscalerOverrides } from '../messages/overrides/optiscaler/fr';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import type { LocalePack } from './types';

const frPack = {
  locale: 'fr',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: fr,
  externalMessages: mergeExternalMessages(
    bindExternalMessages(LUMA_SOURCE_CATALOG, lumaOverrides),
    bindExternalMessages(NVAPI_SOURCE_CATALOG, nvapiOverrides),
    bindExternalMessages(OPTISCALER_SOURCE_CATALOG, optiscalerOverrides),
  ),
} as const satisfies LocalePack;

export default frPack;
