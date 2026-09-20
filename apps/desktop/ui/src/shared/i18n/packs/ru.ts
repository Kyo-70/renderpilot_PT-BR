import { ru } from '../messages/ru';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages, mergeExternalMessages } from '../messages/external';
import { lumaOverrides } from '../messages/overrides/luma/ru';
import { LUMA_SOURCE_CATALOG } from '../messages/overrides/luma/contract.generated';
import { NVAPI_SOURCE_CATALOG } from '../messages/overrides/nvapi/contract.generated';
import { nvapiOverrides } from '../messages/overrides/nvapi/ru';
import { optiscalerOverrides } from '../messages/overrides/optiscaler/ru';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import { renodxOverrides } from '../messages/overrides/renodx/ru';
import { RENODX_SOURCE_CATALOG } from '../messages/overrides/renodx/contract.generated';
import type { LocalePack } from './types';

const ruPack = {
  locale: 'ru',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: ru,
  externalMessages: mergeExternalMessages(
    bindExternalMessages(LUMA_SOURCE_CATALOG, lumaOverrides),
    bindExternalMessages(NVAPI_SOURCE_CATALOG, nvapiOverrides),
    bindExternalMessages(OPTISCALER_SOURCE_CATALOG, optiscalerOverrides),
    bindExternalMessages(RENODX_SOURCE_CATALOG, renodxOverrides),
  ),
} as const satisfies LocalePack;

export default ruPack;
