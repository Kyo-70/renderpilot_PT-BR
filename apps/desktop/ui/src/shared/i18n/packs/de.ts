import { de } from '../messages/de';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages, mergeExternalMessages } from '../messages/external';
import { lumaOverrides } from '../messages/overrides/luma/de';
import { LUMA_SOURCE_CATALOG } from '../messages/overrides/luma/contract.generated';
import { NVAPI_SOURCE_CATALOG } from '../messages/overrides/nvapi/contract.generated';
import { nvapiOverrides } from '../messages/overrides/nvapi/de';
import { optiscalerOverrides } from '../messages/overrides/optiscaler/de';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import { renodxOverrides } from '../messages/overrides/renodx/de';
import { RENODX_SOURCE_CATALOG } from '../messages/overrides/renodx/contract.generated';
import type { LocalePack } from './types';

const dePack = {
  locale: 'de',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: de,
  externalMessages: mergeExternalMessages(
    bindExternalMessages(LUMA_SOURCE_CATALOG, lumaOverrides),
    bindExternalMessages(NVAPI_SOURCE_CATALOG, nvapiOverrides),
    bindExternalMessages(OPTISCALER_SOURCE_CATALOG, optiscalerOverrides),
    bindExternalMessages(RENODX_SOURCE_CATALOG, renodxOverrides),
  ),
} as const satisfies LocalePack;

export default dePack;
