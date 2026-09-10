use renderpilot_domain::PathRef;

use crate::addons::luma::peer::active_update::{
    error::LumaActiveUpdateError, model::LumaActiveUpdateDlssInput,
};
use crate::addons::luma::peer::root_authority::LumaPeerRootAuthority;
use crate::catalog::cascade::CascadeResult;
use crate::coordinated_files::CatalogPathClaim;

use super::{
    evidence,
    model::{DlssAction, PersistedDlss},
    routes,
};

/// Performs the one retained live observation and dispatches to the
/// ownership-specific route classifier. No effect is appended in this phase.
pub(super) fn classify(
    authority: &LumaPeerRootAuthority,
    target: PathRef,
    persisted: PersistedDlss,
    input: LumaActiveUpdateDlssInput,
    catalog_claim: &CatalogPathClaim,
    cascade: &CascadeResult,
) -> Result<DlssAction, LumaActiveUpdateError> {
    let (input_kind, bundled) = evidence::prepare_bundled(input)?;
    if let Some(bundled) = bundled {
        let live_snapshot = evidence::observe_path(authority, &target)?;
        let live = evidence::inspect_live(&target, &live_snapshot)?;
        return routes::classify_with_bundled(
            authority,
            target,
            persisted,
            live,
            bundled,
            catalog_claim,
        );
    }

    let live_snapshot = if matches!(&persisted, PersistedDlss::None) {
        None
    } else {
        Some(evidence::observe_path(authority, &target)?)
    };
    let live = live_snapshot
        .as_ref()
        .map(|snapshot| evidence::inspect_live_image(&target, snapshot))
        .transpose()?
        .flatten();
    routes::classify_without_bundled(
        authority,
        target,
        persisted,
        input_kind,
        live.as_ref(),
        catalog_claim,
        cascade,
    )
}
