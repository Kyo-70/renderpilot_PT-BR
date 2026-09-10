use renderpilot_domain::{
    NormalizedPathRelation, PathRef, PeerEndpointRole, normalized_path_relation,
};

use super::error::LumaPeerEffectError;
use super::model::{EndpointBundle, LumaPeerEffectGroup, LumaPeerOperationOrder};
use crate::peer_mutation_executor::VerifiedPeerFile;

pub(super) fn live_role(group: LumaPeerEffectGroup) -> PeerEndpointRole {
    match group {
        LumaPeerEffectGroup::Host => PeerEndpointRole::TopologyDownstream,
        LumaPeerEffectGroup::Generic
        | LumaPeerEffectGroup::DgVoodoo
        | LumaPeerEffectGroup::DlssCascade => PeerEndpointRole::Disjoint,
    }
}

pub(super) fn validate_group_roles(bundles: &[EndpointBundle]) -> Result<(), LumaPeerEffectError> {
    let host_live_count = bundles
        .iter()
        .filter(|bundle| bundle.group() == LumaPeerEffectGroup::Host)
        .filter(|bundle| bundle.live_role() == PeerEndpointRole::TopologyDownstream)
        .count();
    if host_live_count > 1 {
        return Err(LumaPeerEffectError::InvalidHostBundle(host_live_count));
    }
    Ok(())
}

pub(super) fn ensure_distinct_pair_paths(
    live_path: &PathRef,
    sidecar_path: &PathRef,
) -> Result<(), LumaPeerEffectError> {
    if matches!(
        normalized_path_relation(live_path.as_str(), sidecar_path.as_str()),
        NormalizedPathRelation::Equal
    ) {
        return Err(LumaPeerEffectError::SamePairPath(live_path.clone()));
    }
    if normalized_path_relation(live_path.as_str(), sidecar_path.as_str()).overlaps() {
        return Err(LumaPeerEffectError::OverlappingPaths(
            live_path.clone(),
            sidecar_path.clone(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_bytes_match_image(
    path: &PathRef,
    bytes: &[u8],
    image: &VerifiedPeerFile,
    baseline: bool,
) -> Result<(), LumaPeerEffectError> {
    if !image.matches_content_bytes(bytes) {
        return Err(if baseline {
            LumaPeerEffectError::BaselineImageMismatch(path.clone())
        } else {
            LumaPeerEffectError::BeforeImageMismatch(path.clone())
        });
    }
    Ok(())
}

pub(super) fn bundle_overlaps(left: &EndpointBundle, right: &EndpointBundle) -> bool {
    let mut overlaps = false;
    left.visit(|left_endpoint| {
        right.visit(|right_endpoint| {
            overlaps |=
                normalized_path_relation(left_endpoint.path.as_str(), right_endpoint.path.as_str())
                    .overlaps();
        });
    });
    overlaps
}

pub(super) fn group_rank(order: LumaPeerOperationOrder, group: LumaPeerEffectGroup) -> u8 {
    match (order, group) {
        (LumaPeerOperationOrder::InstallOrUpdate, LumaPeerEffectGroup::Generic)
        | (LumaPeerOperationOrder::Uninstall, LumaPeerEffectGroup::DlssCascade) => 0,
        (_, LumaPeerEffectGroup::DgVoodoo) => 1,
        (LumaPeerOperationOrder::InstallOrUpdate, LumaPeerEffectGroup::DlssCascade)
        | (LumaPeerOperationOrder::Uninstall, LumaPeerEffectGroup::Generic) => 2,
        (_, LumaPeerEffectGroup::Host) => 3,
    }
}
