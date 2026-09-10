use std::path::{Path, PathBuf};

use super::active_payload::{
    ActivePayloadError, ActivePayloadProjection, ActivePayloadStructuralError, lower_active_payload,
};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use super::root_authority::LumaPeerRootAuthority;
use crate::addons::luma::fetch::types::LumaPayloadFile;
use crate::peer_mutation_executor::EndpointExpectation;
use renderpilot_domain::{PathRef, normalized_path_key};

fn path_ref(path: &Path) -> PathRef {
    PathRef::from_canonical_native_absolute(path).expect("path")
}

fn authority(game: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(game, &game.join("dxgi.dll")).expect("authority")
}

fn canonical_root(game: &Path) -> PathBuf {
    PathBuf::from(authority(game).canonical_game_root_ref().as_str())
}

fn effective_addon_root(game: &Path) -> PathBuf {
    let sealed = authority(game);
    PathBuf::from(sealed.effective_addon_root_ref().as_str())
}

fn file(path: &str, bytes: &[u8]) -> LumaPayloadFile {
    LumaPayloadFile {
        relative_path: path.to_owned(),
        bytes: bytes.to_vec(),
    }
}

fn lower(
    game: &Path,
    payload: &[LumaPayloadFile],
    main: &str,
) -> Result<(ActivePayloadProjection, super::effects::LumaPeerEffects), ActivePayloadError> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection =
        lower_active_payload(&authority(game), payload.to_vec(), main, &mut accumulator)?;
    let effects = accumulator
        .finalize()
        .expect("finalize")
        .expect("payload has a main add-on");
    Ok((projection, effects))
}

#[test]
fn creates_nested_payload_and_projects_the_canonical_main_addon() {
    let root = tempfile::tempdir().expect("root");
    let payload = [
        file("Luma/Global/Copy_PS.hlsl", b"technique {}"),
        file("Luma-Game.addon", b"addon"),
    ];

    let (projection, effects) = lower(root.path(), &payload, "Luma-Game.addon").expect("lower");
    let canonical_root = canonical_root(root.path());

    assert_eq!(
        projection.main_addon(),
        &path_ref(&canonical_root.join("Luma-Game.addon"))
    );
    assert_eq!(projection.created_files().len(), 2);
    assert!(projection.backed_up_files().is_empty());
    assert!(projection.dlss_bytes().is_none());
    assert_eq!(effects.program().endpoints().len(), 2);
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| matches!(endpoint.before(), EndpointExpectation::Absent))
    );
}

#[test]
fn collision_uses_exact_sidecar_then_live_pair_and_projects_live_in_both_lists() {
    let root = tempfile::tempdir().expect("root");
    let live = root.path().join("Luma-Game.addon");
    std::fs::write(&live, b"foreign").expect("foreign");
    let payload = [file("Luma-Game.addon", b"luma")];

    let (projection, effects) = lower(root.path(), &payload, "Luma-Game.addon").expect("lower");
    let canonical_root = canonical_root(root.path());
    let sidecar = path_ref(&canonical_root.join("Luma-Game.addon.bak"));
    let live = path_ref(&canonical_root.join("Luma-Game.addon"));

    assert_eq!(projection.created_files(), std::slice::from_ref(&live));
    assert_eq!(projection.backed_up_files(), std::slice::from_ref(&live));
    assert!(projection.dlss_bytes().is_none());
    assert_eq!(
        effects
            .program()
            .endpoints()
            .iter()
            .map(|endpoint| endpoint.path().clone())
            .collect::<Vec<_>>(),
        vec![sidecar, live]
    );
    assert_eq!(
        effects.payloads(),
        &[Some(b"foreign".to_vec()), Some(b"luma".to_vec())]
    );
}

#[test]
fn identical_collision_is_still_acquired_and_backed() {
    let root = tempfile::tempdir().expect("root");
    let live_path = root.path().join("Luma-Game.addon");
    std::fs::write(&live_path, b"same").expect("foreign");
    let payload = [file("Luma-Game.addon", b"same")];

    let (projection, effects) = lower(root.path(), &payload, "Luma-Game.addon").expect("lower");

    assert_eq!(projection.created_files().len(), 1);
    assert_eq!(projection.backed_up_files(), projection.created_files());
    assert!(projection.dlss_bytes().is_none());
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(
        effects.payloads(),
        &[Some(b"same".to_vec()), Some(b"same".to_vec())]
    );
}

#[test]
fn exact_dlss_payload_is_left_for_the_dedicated_planner() {
    let root = tempfile::tempdir().expect("root");
    let payload = [
        file("nvngx_dlss.dll", b"dedicated"),
        file("Luma-Game.addon", b"addon"),
    ];

    let (projection, effects) = lower(root.path(), &payload, "Luma-Game.addon").expect("lower");

    assert_eq!(projection.created_files().len(), 1);
    assert!(projection.created_files().iter().all(|path| {
        !path
            .as_str()
            .to_ascii_lowercase()
            .ends_with("nvngx_dlss.dll")
    }));
    assert_eq!(projection.dlss_bytes(), Some(b"dedicated".as_slice()));
    assert_eq!(effects.program().endpoints().len(), 1);
}

#[test]
fn main_addon_must_be_a_present_non_dlss_addon_payload() {
    let root = tempfile::tempdir().expect("root");
    let missing = [file("Luma/Global/Copy.hlsl", b"shader")];
    assert!(matches!(
        lower(root.path(), &missing, "Luma-Game.addon"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::MissingMainAddon { .. }
        ))
    ));

    let dlss_main = [file("nvngx_dlss.dll", b"dlss")];
    assert!(matches!(
        lower(root.path(), &dlss_main, "nvngx_dlss.dll"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::InvalidMainAddon { .. }
        ))
    ));

    let nested_main = [file("Luma/Game.addon", b"addon")];
    assert!(matches!(
        lower(root.path(), &nested_main, "Luma/Game.addon"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::InvalidMainAddon { .. }
        ))
    ));

    let duplicate_main = [file("Game.addon", b"one"), file("Other.addon", b"two")];
    assert!(matches!(
        lower(root.path(), &duplicate_main, "Game.addon"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::MultipleMainAddons { .. }
        ))
    ));
}

#[test]
fn malformed_paths_are_rejected_before_any_effect_is_added() {
    let root = tempfile::tempdir().expect("root");
    let malformed = [
        vec![file("../escape.hlsl", b"escape"), file("Game.addon", b"a")],
        vec![file("Luma//Copy.hlsl", b"alias"), file("Game.addon", b"a")],
        vec![file("CON.txt", b"reserved"), file("Game.addon", b"a")],
        vec![
            file("Luma", b"file"),
            file("Luma/child.hlsl", b"nested"),
            file("Game.addon", b"a"),
        ],
        vec![
            file("Luma/Global/Copy.hlsl", b"a"),
            file("luma\u{005c}global\u{005c}copy.hlsl", b"duplicate"),
            file("Game.addon", b"main"),
        ],
    ];

    for payload in malformed {
        let mut accumulator =
            LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
        let result = lower_active_payload(
            &authority(root.path()),
            payload,
            "Game.addon",
            &mut accumulator,
        );
        assert!(matches!(result, Err(ActivePayloadError::Structural(_))));
        assert!(accumulator.finalize().expect("finalize").is_none());
    }
}

#[test]
fn duplicate_payload_targets_are_rejected() {
    let root = tempfile::tempdir().expect("root");
    let duplicate = [file("Game.addon", b"a"), file("game.addon", b"b")];
    assert!(matches!(
        lower(root.path(), &duplicate, "Game.addon"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::DuplicateTarget(_)
        ))
    ));
}

#[test]
fn duplicate_exact_dlss_payload_targets_are_rejected() {
    let root = tempfile::tempdir().expect("root");
    let duplicate = [
        file("nvngx_dlss.dll", b"first"),
        file("NVNGX_DLSS.DLL", b"second"),
        file("Game.addon", b"addon"),
    ];

    assert!(matches!(
        lower(root.path(), &duplicate, "Game.addon"),
        Err(ActivePayloadError::Structural(
            ActivePayloadStructuralError::DuplicateDlss(_)
        ))
    ));
}

#[test]
fn occupied_sidecar_and_nonfile_target_fail_without_finalizing_effects() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("Game.addon.bak"), b"occupied").expect("sidecar");
    let payload = [file("Game.addon", b"addon")];
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = lower_active_payload(
        &authority(root.path()),
        payload.to_vec(),
        "Game.addon",
        &mut accumulator,
    );
    assert!(matches!(result, Err(ActivePayloadError::Snapshot(_))));
    assert!(accumulator.finalize().expect("finalize").is_none());

    let root = tempfile::tempdir().expect("root");
    std::fs::create_dir(root.path().join("Luma")).expect("directory");
    let payload = [file("Game.addon", b"addon"), file("Luma", b"not a file")];
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = lower_active_payload(
        &authority(root.path()),
        payload.to_vec(),
        "Game.addon",
        &mut accumulator,
    );
    assert!(matches!(result, Err(ActivePayloadError::Observation(_))));
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn split_external_addon_path_is_the_only_payload_authority() {
    let game = tempfile::tempdir().expect("game");
    let payload_root = tempfile::tempdir().expect("payload root");
    std::fs::write(
        game.path().join("ReShade.ini"),
        format!(
            "[ADDON]\r\nAddonPath={}\r\n",
            payload_root.path().to_string_lossy()
        ),
    )
    .expect("config");

    let payload = [file("Game.addon", b"addon")];
    let (projection, _) = lower(game.path(), &payload, "Game.addon").expect("lower");
    let sealed_addon_root = effective_addon_root(game.path());
    assert!(projection.created_files().iter().all(|path| {
        normalized_path_key(path.as_str()).starts_with(&format!(
            "{}/",
            normalized_path_key(path_ref(&sealed_addon_root).as_str())
        ))
    }));
    assert!(!game.path().join("Game.addon").exists());
}

#[test]
fn target_order_is_deterministic_and_independent_of_payload_order() {
    let first = tempfile::tempdir().expect("first");
    let second = tempfile::tempdir().expect("second");
    let forward = [
        file("Luma/Z.hlsl", b"z"),
        file("Game.addon", b"a"),
        file("Luma/A.hlsl", b"a"),
    ];
    let reverse = [forward[2].clone(), forward[1].clone(), forward[0].clone()];
    let (_, first_effects) = lower(first.path(), &forward, "Game.addon").expect("first");
    let (_, second_effects) = lower(second.path(), &reverse, "Game.addon").expect("second");
    let first_root = canonical_root(first.path());
    let second_root = canonical_root(second.path());

    let relative_names = |path: &PathRef, root: &Path| {
        let path_key = normalized_path_key(path.as_str());
        let root_key = normalized_path_key(path_ref(root).as_str());
        path_key
            .strip_prefix(&format!("{root_key}/"))
            .expect("sealed root prefix")
            .to_owned()
    };
    let first_names: Vec<_> = first_effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| relative_names(endpoint.path(), &first_root))
        .collect();
    let second_names: Vec<_> = second_effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| relative_names(endpoint.path(), &second_root))
        .collect();
    assert_eq!(first_names, second_names);
}
