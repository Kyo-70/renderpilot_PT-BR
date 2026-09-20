import { es } from '../messages/es';
import { MESSAGE_CONTRACT_VERSION } from '../messages/generated/contract-version';
import { bindExternalMessages, mergeExternalMessages } from '../messages/external';
import { lumaOverrides } from '../messages/overrides/luma/es';
import { LUMA_SOURCE_CATALOG } from '../messages/overrides/luma/contract.generated';
import { NVAPI_SOURCE_CATALOG } from '../messages/overrides/nvapi/contract.generated';
import { nvapiOverrides } from '../messages/overrides/nvapi/es';
import { optiscalerOverrides } from '../messages/overrides/optiscaler/es';
import { OPTISCALER_SOURCE_CATALOG } from '../messages/overrides/optiscaler/contract.generated';
import { renodxOverrides } from '../messages/overrides/renodx/es';
import { RENODX_SOURCE_CATALOG } from '../messages/overrides/renodx/contract.generated';
import type { LocalePack } from './types';

const esPack = {
  locale: 'es',
  contractVersion: MESSAGE_CONTRACT_VERSION,
  messages: es,
  externalMessages: mergeExternalMessages(
    bindExternalMessages(LUMA_SOURCE_CATALOG, lumaOverrides),
    bindExternalMessages(NVAPI_SOURCE_CATALOG, nvapiOverrides),
    bindExternalMessages(OPTISCALER_SOURCE_CATALOG, optiscalerOverrides),
    bindExternalMessages(RENODX_SOURCE_CATALOG, renodxOverrides),
  ),
} as const satisfies LocalePack;

export default esPack;
