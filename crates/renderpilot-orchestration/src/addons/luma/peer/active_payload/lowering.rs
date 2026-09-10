use renderpilot_domain::normalized_path_key;

use crate::{
    addons::luma::fetch::types::LumaPayloadFile, peer_mutation_executor::PeerPathSnapshot,
};

use super::super::{
    effects::LumaPeerEffectAccumulator,
    generic::{LumaGenericDecision, LumaGenericLoweringError, lower_generic_decision},
    root_authority::LumaPeerRootAuthority,
    snapshot_input::require_absent,
};
use super::{
    model::{ActivePayloadError, ActivePayloadProjection, GenericTarget, ObservedGenericTarget},
    observation::observe,
    validation::{effective_root, validate_active_payload},
};

/// Validates, observes, and lowers all non-DLSS files in one active Luma
/// payload. The caller must have resolved `authority` before entering this
/// function; the ReShade configuration is never consulted here.
pub(crate) fn lower_active_payload(
    authority: &LumaPeerRootAuthority,
    payload: Vec<LumaPayloadFile>,
    main_addon_rel: &str,
    accumulator: &mut LumaPeerEffectAccumulator,
) -> Result<ActivePayloadProjection, ActivePayloadError> {
    let root = effective_root(authority);
    let validated = validate_active_payload(authority, payload, main_addon_rel)?;
    let (main_addon, targets, dlss_bytes) = validated.into_parts();
    let mut targets = targets
        .into_iter()
        .map(|target| {
            let (live, sidecar, bytes) = target.into_parts();
            GenericTarget {
                live,
                sidecar,
                bytes,
            }
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| normalized_path_key(target.live.as_str()));

    // Retain every observation before appending anything to the accumulator.
    // This keeps non-file, link, unreadable, and occupied-sidecar failures
    // atomic from the effect builder's perspective.
    let mut observed = Vec::with_capacity(targets.len());
    for target in targets {
        let live_snapshot = observe(&target.live, root)?;
        let sidecar_snapshot = observe(&target.sidecar, root)?;
        observed.push(ObservedGenericTarget {
            target,
            live_snapshot,
            sidecar_snapshot,
        });
    }

    let mut created_files = Vec::with_capacity(observed.len());
    let mut backed_up_files = Vec::new();
    for entry in observed {
        let ObservedGenericTarget {
            target:
                super::model::GenericTarget {
                    live,
                    sidecar,
                    bytes,
                },
            live_snapshot,
            sidecar_snapshot,
        } = entry;
        let decision = match &live_snapshot {
            PeerPathSnapshot::Absent => {
                // CreateNested has no sidecar endpoint of its own, but an
                // occupied managed sidecar is still an unsafe precondition:
                // a later uninstall must never mistake that file for a
                // sidecar created by this install.
                require_absent(&sidecar, &sidecar_snapshot)?;
                LumaGenericDecision::Create {
                    live_path: &live,
                    live_snapshot: &live_snapshot,
                    prepared_bytes: bytes,
                }
            }
            PeerPathSnapshot::File(_) => {
                backed_up_files.push(live.clone());
                LumaGenericDecision::AcquireForeign {
                    live_path: &live,
                    sidecar_path: &sidecar,
                    live_snapshot: &live_snapshot,
                    sidecar_snapshot: &sidecar_snapshot,
                    prepared_bytes: bytes,
                }
            }
        };
        lower_generic_decision(decision, accumulator).map_err(map_lowering_error)?;
        created_files.push(live);
    }

    Ok(ActivePayloadProjection {
        main_addon,
        created_files,
        backed_up_files,
        dlss_bytes,
    })
}

fn map_lowering_error(error: LumaGenericLoweringError) -> ActivePayloadError {
    match error {
        LumaGenericLoweringError::Snapshot(error) => ActivePayloadError::Snapshot(error),
        LumaGenericLoweringError::Effects(error) => ActivePayloadError::Effects(error),
    }
}
