use std::path::Path;

use renderpilot_domain::GameProxyTopology;

use crate::{ServiceError, failed};

use super::publication::{exact_receipt_from_live, maybe_exact_receipt_from_live};

/// Revalidates a persisted topology without consulting a manifest or network.
pub(crate) fn revalidate_for_removal(
    topology: &GameProxyTopology,
) -> Result<GameProxyTopology, ServiceError> {
    topology.validate().map_err(|error| {
        failed(format!(
            "cannot uninstall an invalid proxy topology: {error}"
        ))
    })?;
    let root = Path::new(topology.root_slot.as_str());
    if let Some(outer_live) =
        maybe_exact_receipt_from_live(root, topology.outer.receipt.ownership())?
        && (outer_live.identity() != topology.outer.receipt.identity()
            || outer_live.digest() != topology.outer.receipt.digest())
    {
        return Err(failed(
            "cannot uninstall a drifted OptiScaler/ReShade chain; repair it first so the remaining add-on can be restored safely",
        ));
    }
    let Some(downstream) = &topology.downstream else {
        return Ok(topology.clone());
    };
    let downstream_path = Path::new(downstream.path.as_str());
    let downstream_live = exact_receipt_from_live(downstream_path, downstream.receipt.ownership())?;
    if downstream_live.identity() != downstream.receipt.identity()
        || downstream_live.digest() != downstream.receipt.digest()
    {
        return Err(failed(
            "cannot uninstall a drifted OptiScaler/ReShade chain; the downstream host changed from its committed identity",
        ));
    }
    Ok(topology.clone())
}
