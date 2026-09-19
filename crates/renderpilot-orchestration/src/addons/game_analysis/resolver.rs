//! Mathematical core reducer for component-wise Unreal Engine version resolution.

use crate::addons::game_analysis::evidence::{Authority, ValidatedEvidence};
use crate::addons::game_analysis::parsers::tokens::VersionClaim;

/// Resolution status of an individual version component (Major, Minor, or Patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedComponent {
    Determined(u32),
    Conflicted,
    Unresolved,
}

/// Component-wise hierarchical reduction of evidence over the 4-tier authority hierarchy.
pub fn resolve_component<F>(
    evidences: &[ValidatedEvidence<'_>],
    extract: F,
) -> (ResolvedComponent, Option<Authority>)
where
    F: Fn(&VersionClaim) -> Option<u32>,
{
    const TIERS: [Authority; 4] = [
        Authority::Authoritative,
        Authority::Strong,
        Authority::Supporting,
        Authority::Weak,
    ];

    for &tier in &TIERS {
        let mut determined_val: Option<u32> = None;
        let mut has_conflict = false;

        for e in evidences.iter().filter(|e| e.authority() == tier) {
            if let Some(val) = extract(&e.claim()) {
                match determined_val {
                    None => determined_val = Some(val),
                    Some(prev) if prev != val => {
                        has_conflict = true;
                        break;
                    }
                    _ => {} // Duplicate identical value, invariant holds
                }
            }
        }

        if has_conflict {
            return (ResolvedComponent::Conflicted, Some(tier));
        } else if let Some(winner) = determined_val {
            return (ResolvedComponent::Determined(winner), Some(tier));
        }
    }

    (ResolvedComponent::Unresolved, None)
}
