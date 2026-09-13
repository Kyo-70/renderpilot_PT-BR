/**
 * Which add-on tool a backend capability or `blocked_by_other_addon` outcome names.
 * RenoDX and Luma are mutually exclusive peers. OptiScaler is chain-aware and
 * may coexist with either one, so exclusivity policy must not be inferred from
 * this taxonomy alone.
 *
 * Pure cross-cutting taxonomy. Moved out of entities/addon to avoid
 * entity-to-entity coupling for a simple vocabulary type.
 */
export type AddonKind = 'renodx' | 'luma' | 'optiscaler';

/** Mutually exclusive add-on pair sharing exclusive host slots (RenoDX ↔ Luma). */
export type ExclusiveAddonKind = 'renodx' | 'luma';

export const ALL_ADDON_KINDS: readonly AddonKind[] = ['renodx', 'luma', 'optiscaler'];

/** Short display name for an add-on tool, shared by filter chips and any UI
 * copy that names one tool from another's context (e.g. a blocked-by message). */
export const ADDON_DISPLAY_NAME: Record<AddonKind, string> = {
  renodx: 'RenoDX',
  luma: 'Luma',
  optiscaler: 'OptiScaler',
};
