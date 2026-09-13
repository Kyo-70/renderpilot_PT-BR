import { zhHant } from '../messages/zh-Hant';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages, mergeExternalMessages } from '../messages/external';
import { lumaOverrides } from '../messages/overrides/luma/zh-Hant';
import { LUMA_SOURCE_CATALOG } from '../messages/overrides/luma/contract.generated';
import { NVAPI_SOURCE_CATALOG } from '../messages/overrides/nvapi/contract.generated';
import { nvapiOverrides } from '../messages/overrides/nvapi/zh-Hant';
import { optiscalerOverrides } from '../messages/overrides/optiscaler/zh-Hant';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import type { LocalePack } from './types';

const zhHantPack = {
  locale: 'zh-Hant',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: zhHant,
  externalMessages: mergeExternalMessages(
    bindExternalMessages(LUMA_SOURCE_CATALOG, lumaOverrides),
    bindExternalMessages(NVAPI_SOURCE_CATALOG, nvapiOverrides),
    bindExternalMessages(OPTISCALER_SOURCE_CATALOG, optiscalerOverrides),
  ),
} as const satisfies LocalePack;

export default zhHantPack;
