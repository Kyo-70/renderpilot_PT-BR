import type { MessageKeyWithoutParams } from '@shared/i18n';
import type { ToolI18nPrefix } from '../model/tool-message-key';

/** Visual presentation tone for status and signal badges (confidence, compatibility, etc.). */
export type AddonBadgeTone = 'verified' | 'experimental' | 'untested' | 'unsupported';

/** i18n key prefix for tool-specific freshness / status copy. */
export type AddonToolI18nPrefix = ToolI18nPrefix;

/** Attribution link descriptor for add-on cards. */
export type AddonAttributionProps = {
  textKey: MessageKeyWithoutParams;
  linkKey: MessageKeyWithoutParams;
  href: string;
};
