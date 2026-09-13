export type { Locale, LanguageMode } from './locale';
export type {
  MessageKey,
  MessageKeyForParams,
  MessageKeyWithoutParams,
  MessageParams,
  MessageRef,
  ParameterizedMessageKey,
} from './messages/en';

export {
  LocaleLoadError,
  createMessageRef,
  getI18nState,
  getLocale,
  initializeI18n,
  setLanguageMode,
  t,
  translateExternalMessage,
  translateMessageRef,
} from './runtime.svelte';

export {
  OPTISCALER_MESSAGE_CONTEXTS,
  OPTISCALER_SOURCE_CATALOG,
} from './messages/overrides/optiscaler/contract.generated';

export type {
  ExternalMessageInput,
  I18nInitializationResult,
  I18nRuntimeState,
  I18nSwitchResult,
} from './runtime.svelte';
