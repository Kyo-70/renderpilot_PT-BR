use std::path::{Path, PathBuf};

use super::active_dgvoodoo::{
    ActiveDgVoodooError, ActiveDgVoodooPlan, ActiveDgVoodooSnapshot, lower_active_dgvoodoo,
    plan_active_dgvoodoo,
};
use super::effects::{LumaPeerEffectAccumulator, LumaPeerOperationOrder};
use super::root_authority::LumaPeerRootAuthority;
use crate::addons::engine::IniSection;
use crate::addons::luma::dgvoodoo::{
    AdoptedDgVoodoo, DgVoodooInstall, PreparedDgVoodoo, PreparedDgVoodooFile, ReusedDgVoodoo,
};
use crate::peer_mutation_executor::{
    EndpointExpectation, PeerPathSnapshot, observe_peer_path_snapshot,
};
use renderpilot_domain::{
    PathRef, PeerEndpointRole, TrackedSource, TrackedSourceRole, managed_sidecar_path,
};

fn path_ref(path: &Path) -> PathRef {
    PathRef::new(path.to_string_lossy().into_owned()).expect("path")
}

fn authority(root: &Path) -> LumaPeerRootAuthority {
    LumaPeerRootAuthority::resolve(root, &root.join("ReShade64.dll")).expect("authority")
}

fn sections() -> Vec<IniSection> {
    vec![IniSection {
        name: "General".to_owned(),
        keys: vec![("OutputAPI".to_owned(), "d3d11_fl11_0".to_owned())],
    }]
}

fn prepared(files: Vec<PreparedDgVoodooFile>) -> PreparedDgVoodoo {
    PreparedDgVoodoo {
        version: "2.82".to_owned(),
        files,
        config_file: "dgVoodoo.conf".to_owned(),
        config_default: "[General]\r\nOutputAPI = d3d11_fl11_0\r\n".to_owned(),
        config_sections: sections(),
        source_url: "https://example.test/dgvoodoo.zip".to_owned(),
        source_etag: Some("etag".to_owned()),
        source_last_modified: Some("today".to_owned()),
        archive_digest: "archive-digest".to_owned(),
    }
}

fn managed_install() -> DgVoodooInstall {
    DgVoodooInstall::Managed(prepared(vec![
        PreparedDgVoodooFile {
            dest: "D3D9.dll".to_owned(),
            bytes: b"d3d9".to_vec(),
        },
        PreparedDgVoodooFile {
            dest: "D3D8.dll".to_owned(),
            bytes: b"d3d8".to_vec(),
        },
    ]))
}

fn reused_install() -> DgVoodooInstall {
    DgVoodooInstall::Reused(ReusedDgVoodoo {
        config_file: "dgVoodoo.conf".to_owned(),
        config_default: "[General]\r\nOutputAPI = d3d11_fl11_0\r\n".to_owned(),
        config_sections: sections(),
    })
}

fn adopted_install() -> DgVoodooInstall {
    DgVoodooInstall::Adopted(AdoptedDgVoodoo {
        config: match reused_install() {
            DgVoodooInstall::Reused(config) => config,
            DgVoodooInstall::Managed(_) | DgVoodooInstall::Adopted(_) => unreachable!(),
        },
        existing_paths: vec![PathBuf::from("foreign.dll")],
    })
}

fn plan_with_root(root: &Path, install: &DgVoodooInstall) -> ActiveDgVoodooPlan {
    plan_active_dgvoodoo(&authority(root), Some(install.clone()))
        .expect("plan")
        .expect("dependency plan")
}

fn with_plan_observations<R>(
    plan: ActiveDgVoodooPlan,
    root: &Path,
    action: impl for<'a> FnOnce(ActiveDgVoodooPlan, Vec<ActiveDgVoodooSnapshot<'a>>) -> R,
) -> R {
    let images = plan
        .observation_paths()
        .iter()
        .map(|path| observe_peer_path_snapshot(path, &path_ref(root)).expect("snapshot"))
        .collect::<Vec<_>>();
    let entries: Vec<ActiveDgVoodooSnapshot<'_>> = plan
        .observation_paths()
        .iter()
        .zip(&images)
        .map(|(path, image)| ActiveDgVoodooSnapshot::new(path, image))
        .collect();
    action(plan, entries)
}

fn lower(
    plan: ActiveDgVoodooPlan,
    root: &Path,
) -> Result<super::active_dgvoodoo::ActiveDgVoodooRecordProjection, ActiveDgVoodooError> {
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    with_plan_observations(plan, root, |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut accumulator)
    })
}

#[test]
fn managed_runtime_and_absent_config_create_only_owned_files() {
    let root = tempfile::tempdir().expect("root");
    let install = managed_install();
    let plan = plan_with_root(root.path(), &install);

    assert_eq!(
        plan.observation_paths()
            .iter()
            .map(|path| path.as_str().rsplit(['/', '\\']).next().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "D3D8.dll",
            "D3D8.dll.bak",
            "D3D9.dll",
            "D3D9.dll.bak",
            "dgVoodoo.conf",
            "dgVoodoo.conf.bak",
        ]
    );

    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let projection = with_plan_observations(plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut accumulator).expect("lower")
    });
    let effects = accumulator.finalize().expect("finalize").expect("effects");

    assert_eq!(
        effects
            .program()
            .endpoints()
            .iter()
            .map(|endpoint| endpoint.path().as_str().rsplit(['/', '\\']).next().unwrap())
            .collect::<Vec<_>>(),
        vec!["D3D8.dll", "D3D9.dll", "dgVoodoo.conf"]
    );
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| endpoint.role() == PeerEndpointRole::Disjoint)
    );
    assert!(
        effects
            .program()
            .endpoints()
            .iter()
            .all(|endpoint| matches!(endpoint.before(), EndpointExpectation::Absent))
    );
    assert_eq!(projection.created_files().len(), 3);
    assert!(projection.backed_up_files().is_empty());
    assert_eq!(
        projection.tracked_source().map(TrackedSource::role),
        Some(TrackedSourceRole::DgVoodooWrapper)
    );
    assert!(
        projection
            .tracked_source()
            .is_some_and(|source| source.digest() == "archive-digest")
    );
}

#[test]
fn managed_runtime_that_appears_after_phase_one_is_refused() {
    let root = tempfile::tempdir().expect("root");
    let install = managed_install();
    let plan = plan_with_root(root.path(), &install);
    std::fs::write(root.path().join("D3D9.dll"), b"appeared").expect("appeared runtime");

    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = with_plan_observations(plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut accumulator)
    });
    assert!(matches!(
        result,
        Err(ActiveDgVoodooError::Snapshot(
            super::snapshot_input::LumaSnapshotInputError::ExpectedAbsent(path)
        )) if path.as_str().ends_with("D3D9.dll")
    ));
    assert!(accumulator.finalize().expect("finalize").is_none());
}

#[test]
fn reused_dlls_are_not_observed_or_claimed() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("D3D9.dll"), b"foreign d3d9").expect("foreign runtime");
    std::fs::write(root.path().join("D3D8.dll"), b"foreign d3d8").expect("foreign runtime");

    let plan = plan_with_root(root.path(), &reused_install());
    assert_eq!(
        plan.observation_paths()
            .iter()
            .map(|path| path.as_str().rsplit(['/', '\\']).next().unwrap())
            .collect::<Vec<_>>(),
        vec!["dgVoodoo.conf", "dgVoodoo.conf.bak"]
    );
    let projection = lower(plan, root.path()).expect("lower config");
    assert!(
        projection
            .created_files()
            .iter()
            .all(|path| !path.as_str().ends_with("D3D9.dll"))
    );
    assert!(projection.tracked_source().is_none());
}

#[test]
fn adopted_ownership_must_be_normalized_before_active_planning() {
    let root = tempfile::tempdir().expect("root");
    assert!(matches!(
        plan_active_dgvoodoo(&authority(root.path()), Some(adopted_install())),
        Err(ActiveDgVoodooError::AdoptedOwnershipUnsupported)
    ));
}

#[test]
fn absent_identical_and_changed_configs_have_exact_reversible_shapes() {
    let root = tempfile::tempdir().expect("root");
    let install = reused_install();

    let absent_plan = plan_with_root(root.path(), &install);
    let mut absent_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let absent_projection = with_plan_observations(absent_plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut absent_accumulator).expect("absent config")
    });
    assert_eq!(absent_projection.created_files().len(), 1);
    assert!(absent_projection.backed_up_files().is_empty());
    assert_eq!(
        absent_accumulator
            .finalize()
            .expect("finalize")
            .expect("effect")
            .program()
            .endpoints()
            .len(),
        1
    );

    let expected = "[General]\r\nOutputAPI = d3d11_fl11_0\r\n";
    std::fs::write(root.path().join("dgVoodoo.conf"), expected).expect("identical config");
    let identical_plan = plan_with_root(root.path(), &install);
    let mut identical_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let identical_projection =
        with_plan_observations(identical_plan, root.path(), |plan, entries| {
            lower_active_dgvoodoo(plan, &entries, &mut identical_accumulator)
                .expect("identical config")
        });
    assert!(identical_projection.created_files().is_empty());
    assert!(identical_projection.backed_up_files().is_empty());
    assert!(
        identical_accumulator
            .finalize()
            .expect("finalize")
            .is_none()
    );

    let original = "[General]\r\nOutputAPI = d3d9\r\n";
    std::fs::write(root.path().join("dgVoodoo.conf"), original).expect("changed config");
    let changed_plan = plan_with_root(root.path(), &install);
    let mut changed_accumulator =
        LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let changed_projection = with_plan_observations(changed_plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut changed_accumulator).expect("changed config")
    });
    assert_eq!(changed_projection.created_files().len(), 1);
    assert_eq!(changed_projection.backed_up_files().len(), 1);
    assert_eq!(
        changed_projection.created_files(),
        changed_projection.backed_up_files()
    );
    assert!(
        changed_projection.created_files()[0]
            .as_str()
            .ends_with("dgVoodoo.conf")
    );
    let effects = changed_accumulator
        .finalize()
        .expect("finalize")
        .expect("effects");
    assert_eq!(effects.program().endpoints().len(), 2);
    assert_eq!(effects.payloads()[0], Some(original.as_bytes().to_vec()));
    assert_eq!(effects.payloads().len(), 2);
    assert!(matches!(
        effects.program().endpoints()[0].before(),
        EndpointExpectation::Absent
    ));
    assert!(matches!(
        effects.program().endpoints()[1].before(),
        EndpointExpectation::File(_)
    ));
}

#[test]
fn every_accepted_config_shape_rejects_an_occupied_sidecar() {
    let root = tempfile::tempdir().expect("root");
    for install in [managed_install(), reused_install()] {
        let config_sidecar = root.path().join("dgVoodoo.conf.bak");
        std::fs::write(&config_sidecar, b"foreign sidecar").expect("sidecar");
        let plan = plan_with_root(root.path(), &install);
        let mut accumulator =
            LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
        let result = with_plan_observations(plan, root.path(), |plan, entries| {
            lower_active_dgvoodoo(plan, &entries, &mut accumulator)
        });
        assert!(matches!(
            result,
            Err(ActiveDgVoodooError::Snapshot(
                super::snapshot_input::LumaSnapshotInputError::ExpectedAbsent(path)
            )) if path.as_str().ends_with("dgVoodoo.conf.bak")
        ));
        std::fs::remove_file(config_sidecar).expect("remove sidecar");
    }
}

#[test]
fn invalid_utf8_and_nonfile_config_are_fail_closed() {
    let root = tempfile::tempdir().expect("root");
    let install = reused_install();
    std::fs::write(root.path().join("dgVoodoo.conf"), [0xff, 0xfe]).expect("invalid utf8");
    let plan = plan_with_root(root.path(), &install);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = with_plan_observations(plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut accumulator)
    });
    assert!(matches!(
        result,
        Err(ActiveDgVoodooError::InvalidConfigEncoding(path))
            if path.as_str().ends_with("dgVoodoo.conf")
    ));

    std::fs::remove_file(root.path().join("dgVoodoo.conf")).expect("remove config");
    std::fs::create_dir(root.path().join("dgVoodoo.conf")).expect("directory config");
    let directory_plan = plan_with_root(root.path(), &install);
    let config_path = directory_plan
        .observation_paths()
        .iter()
        .find(|path| path.as_str().ends_with("dgVoodoo.conf"))
        .expect("config path");
    assert!(observe_peer_path_snapshot(config_path, &path_ref(root.path())).is_err());
}

#[test]
fn duplicate_and_unsafe_names_are_rejected_before_observation() {
    let root = tempfile::tempdir().expect("root");
    let duplicate = DgVoodooInstall::Managed(prepared(vec![
        PreparedDgVoodooFile {
            dest: "D3D9.dll".to_owned(),
            bytes: vec![1],
        },
        PreparedDgVoodooFile {
            dest: "d3d9.DLL".to_owned(),
            bytes: vec![2],
        },
    ]));
    assert!(matches!(
        plan_active_dgvoodoo(&authority(root.path()), Some(duplicate)),
        Err(ActiveDgVoodooError::DuplicateTarget { .. })
    ));

    let unsafe_name = DgVoodooInstall::Managed(prepared(vec![PreparedDgVoodooFile {
        dest: "nested\\D3D9.dll".to_owned(),
        bytes: vec![1],
    }]));
    assert!(matches!(
        plan_active_dgvoodoo(&authority(root.path()), Some(unsafe_name)),
        Err(ActiveDgVoodooError::InvalidTargetName { .. })
    ));

    let config_collision = DgVoodooInstall::Managed(PreparedDgVoodoo {
        config_file: "D3D9.dll.bak".to_owned(),
        ..prepared(vec![PreparedDgVoodooFile {
            dest: "D3D9.dll".to_owned(),
            bytes: vec![1],
        }])
    });
    assert!(matches!(
        plan_active_dgvoodoo(&authority(root.path()), Some(config_collision)),
        Err(ActiveDgVoodooError::DuplicateTarget { .. })
    ));
}

#[test]
fn snapshot_set_is_closed_and_aliases_are_not_accepted() {
    let root = tempfile::tempdir().expect("root");
    let install = reused_install();
    let missing_plan = plan_with_root(root.path(), &install);
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let missing_result = with_plan_observations(missing_plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries[..entries.len() - 1], &mut accumulator)
    });
    assert!(matches!(
        missing_result,
        Err(ActiveDgVoodooError::MissingSnapshot(_))
    ));

    let unexpected_path = path_ref(&root.path().join("foreign.dll"));
    let unexpected = PeerPathSnapshot::Absent;
    let unexpected_plan = plan_with_root(root.path(), &install);
    let unexpected_result =
        with_plan_observations(unexpected_plan, root.path(), |plan, entries| {
            let mut entries = entries.clone();
            entries.push(ActiveDgVoodooSnapshot::new(&unexpected_path, &unexpected));
            lower_active_dgvoodoo(plan, &entries, &mut accumulator)
        });
    assert!(matches!(
        unexpected_result,
        Err(ActiveDgVoodooError::UnexpectedSnapshot(path))
            if path.as_str().ends_with("foreign.dll")
    ));
}

#[test]
fn managed_sidecars_are_checked_even_when_runtime_is_absent() {
    let root = tempfile::tempdir().expect("root");
    let install = managed_install();
    let plan = plan_with_root(root.path(), &install);
    let runtime_sidecar = managed_sidecar_path(
        plan.observation_paths()
            .iter()
            .find(|path| path.as_str().ends_with("D3D9.dll"))
            .expect("runtime"),
    )
    .expect("sidecar");
    std::fs::write(runtime_sidecar.as_str(), b"occupied").expect("runtime sidecar");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    let result = with_plan_observations(plan, root.path(), |plan, entries| {
        lower_active_dgvoodoo(plan, &entries, &mut accumulator)
    });
    assert!(matches!(
        result,
        Err(ActiveDgVoodooError::Snapshot(
            super::snapshot_input::LumaSnapshotInputError::ExpectedAbsent(path)
        )) if path.as_str().ends_with("D3D9.dll.bak")
    ));
}

#[test]
fn none_is_a_noop_decision() {
    let root = tempfile::tempdir().expect("root");
    assert!(
        plan_active_dgvoodoo(&authority(root.path()), None)
            .expect("no dependency")
            .is_none()
    );
}
