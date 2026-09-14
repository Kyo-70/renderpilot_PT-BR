use renderpilot_domain::Swappability;

/// Application-level reasons a component cannot be replaced.
///
/// This policy is intentionally independent of operation-plan enums so
/// candidate discovery and plan assessment cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplacementBlockReason {
    ReadOnly,
    IntegratedIntoEngine,
    Unsafe,
}

pub(crate) const fn replacement_block_reason(
    swappability: Swappability,
) -> Option<ReplacementBlockReason> {
    match swappability {
        Swappability::ReadOnly => Some(ReplacementBlockReason::ReadOnly),
        Swappability::IntegratedIntoEngine => Some(ReplacementBlockReason::IntegratedIntoEngine),
        Swappability::Unsafe => Some(ReplacementBlockReason::Unsafe),
        Swappability::Swappable | Swappability::BundleOnly | Swappability::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ReplacementBlockReason, replacement_block_reason};
    use renderpilot_domain::Swappability;

    #[test]
    fn replacement_policy_is_exhaustive_and_neutral() {
        assert_eq!(
            replacement_block_reason(Swappability::ReadOnly),
            Some(ReplacementBlockReason::ReadOnly)
        );
        assert_eq!(
            replacement_block_reason(Swappability::IntegratedIntoEngine),
            Some(ReplacementBlockReason::IntegratedIntoEngine)
        );
        assert_eq!(
            replacement_block_reason(Swappability::Unsafe),
            Some(ReplacementBlockReason::Unsafe)
        );
        for swappability in [
            Swappability::Swappable,
            Swappability::BundleOnly,
            Swappability::Unknown,
        ] {
            assert_eq!(replacement_block_reason(swappability), None);
        }
    }
}
