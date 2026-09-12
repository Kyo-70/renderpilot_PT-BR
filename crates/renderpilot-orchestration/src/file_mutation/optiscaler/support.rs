/// Native syscall evidence. It never appears in the persisted journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiskObservation {
    Absent,
    File {
        identity: String,
        digest: Sha256Hash,
    },
    Directory {
        identity: String,
    },
    NonRegular,
    Unreadable,
}

fn durable(value: &DiskObservation) -> DurableObservation {
    match value {
        DiskObservation::Absent => DurableObservation::Absent,
        DiskObservation::File { identity, digest } => DurableObservation::File {
            identity: identity.clone(),
            digest: digest.clone(),
        },
        DiskObservation::Directory { identity } => DurableObservation::Directory {
            identity: identity.clone(),
        },
        DiskObservation::NonRegular => DurableObservation::NonRegular,
        DiskObservation::Unreadable => DurableObservation::Unreadable,
    }
}

fn native(value: &DurableObservation) -> DiskObservation {
    match value {
        DurableObservation::Absent => DiskObservation::Absent,
        DurableObservation::File { identity, digest } => DiskObservation::File {
            identity: identity.clone(),
            digest: digest.clone(),
        },
        DurableObservation::Directory { identity } => DiskObservation::Directory {
            identity: identity.clone(),
        },
        DurableObservation::NonRegular => DiskObservation::NonRegular,
        DurableObservation::Unreadable => DiskObservation::Unreadable,
    }
}

fn disk_observation(
    observation: &crate::fs::EntryObservation,
) -> Result<DiskObservation, ServiceError> {
    match observation.kind {
        crate::fs::EntryKind::File => Ok(DiskObservation::File {
            identity: observation.identity.clone(),
            digest: Sha256Hash::new(
                observation
                    .digest
                    .as_deref()
                    .ok_or_else(|| crate::failed("file observation has no digest"))?,
            )
            .map_err(|error| crate::failed(format!("invalid file observation digest: {error}")))?,
        }),
        crate::fs::EntryKind::Directory => Ok(DiskObservation::Directory {
            identity: observation.identity.clone(),
        }),
    }
}

fn same_file_digest(left: &DiskObservation, right: &DiskObservation) -> bool {
    matches!(
        (left, right),
        (
            DiskObservation::File { digest: left, .. },
            DiskObservation::File { digest: right, .. }
        ) if left == right
    )
}

fn same_file_identity(left: &DiskObservation, right: &DiskObservation) -> bool {
    matches!(
        (left, right),
        (
            DiskObservation::File { identity: left, .. },
            DiskObservation::File { identity: right, .. }
        ) if left == right
    )
}

fn observe(path: &Path) -> DiskObservation {
    let Ok((parent, leaf)) = crate::fs::verified_parent(path) else {
        return DiskObservation::Unreadable;
    };
    match parent.observe_leaf(&leaf) {
        Ok(None) => DiskObservation::Absent,
        Ok(Some(entry)) => match disk_observation(&entry) {
            Ok(observation) => observation,
            Err(_) => DiskObservation::Unreadable,
        },
        Err(_) => DiskObservation::Unreadable,
    }
}

/// Observes a declared managed endpoint below a retained existing root.  It
/// admits only an absent descendant; every root/ancestor authority failure is
/// surfaced instead of being collapsed into a generic unreadable state.
fn observe_managed_descendant(root: &Path, path: &Path) -> Result<DiskObservation, ServiceError> {
    if !crate::paths::is_within(path, root) || crate::paths::same_path(path, root) {
        return Err(crate::failed(format!(
            "managed OptiScaler endpoint escapes its persisted root: {}",
            path.display()
        )));
    }
    let root = crate::fs::VerifiedDir::open(root).map_err(|error| {
        crate::failed(format!(
            "failed to acquire managed OptiScaler root: {error}"
        ))
    })?;
    match root.observe_descendant(path).map_err(|error| {
        crate::failed(format!(
            "failed to observe managed OptiScaler endpoint {}: {error}",
            path.display()
        ))
    })? {
        None => Ok(DiskObservation::Absent),
        Some(observation) => disk_observation(&observation),
    }
}

fn hash_bytes(bytes: &[u8]) -> Result<Sha256Hash, ServiceError> {
    Sha256Hash::new(hex::encode(Sha256::digest(bytes)))
        .map_err(|error| crate::failed(format!("failed to hash OptiScaler bytes: {error}")))
}

fn token_drift(path: &Path, expected: &DiskObservation, actual: &DiskObservation) -> ServiceError {
    crate::failed(format!(
        "OptiScaler endpoint changed before apply {}: expected {expected:?}, found {actual:?}",
        path.display()
    ))
}

fn roots(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[cfg_attr(windows, expect(unsafe_code, reason = "Windows platform CSPRNG"))]
fn private_workspace_capability() -> Result<String, ServiceError> {
    let mut bytes = [0_u8; 32];
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")
            .map_err(|error| crate::failed(format!("failed to open platform CSPRNG: {error}")))?
            .read_exact(&mut bytes)
            .map_err(|error| {
                crate::failed(format!(
                    "failed to read private namespace capability: {error}"
                ))
            })?;
    }
    #[cfg(windows)]
    {
        #[expect(unsafe_code, reason = "SystemFunction036 is the Windows CSPRNG")]
        #[link(name = "advapi32")]
        unsafe extern "system" {
            fn SystemFunction036(random: *mut u8, length: u32) -> u8;
        }
        let length =
            u32::try_from(bytes.len()).map_err(|_| crate::failed("capability length overflow"))?;
        if unsafe { SystemFunction036(bytes.as_mut_ptr(), length) } == 0 {
            return Err(crate::failed(format!(
                "failed to read private namespace capability: {}",
                std::io::Error::last_os_error()
            )));
        }
    }
    #[cfg(not(any(unix, windows)))]
    return Err(crate::failed(
        "private namespace capability has no native CSPRNG",
    ));
    Ok(hex::encode(bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptiScalerAction {
    Write,
    Delete,
    Relocate,
    CreateDirectory,
    Verify,
    PostCommitRemoveDirectory,
}

/// Stable handle returned by an action after its ordinal is durably applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppliedOperation {
    ordinal: u32,
}

impl AppliedOperation {
    pub(crate) const fn ordinal(self) -> u32 {
        self.ordinal
    }
}

fn action(effect: &DomainOperationEffect) -> OptiScalerAction {
    match effect {
        DomainOperationEffect::Write(_) => OptiScalerAction::Write,
        DomainOperationEffect::Delete(_) => OptiScalerAction::Delete,
        DomainOperationEffect::Relocate(_) => OptiScalerAction::Relocate,
        DomainOperationEffect::CreateDirectory(_) => OptiScalerAction::CreateDirectory,
        DomainOperationEffect::Verify(_) => OptiScalerAction::Verify,
        DomainOperationEffect::PostCommitRemoveDirectory(_) => {
            OptiScalerAction::PostCommitRemoveDirectory
        }
    }
}

fn endpoint_for<'a>(
    effect: &'a DomainOperationEffect,
    path: &Path,
) -> Option<&'a DomainOperationEndpoint> {
    effect
        .endpoints()
        .into_iter()
        .find(|endpoint| crate::paths::same_path(Path::new(endpoint.path()), path))
}

fn endpoint_by_role(
    effect: &DomainOperationEffect,
    role: DomainEndpoint,
) -> Option<&DomainOperationEndpoint> {
    effect
        .endpoints()
        .into_iter()
        .find(|endpoint| endpoint.endpoint() == role)
}

fn expected_native(endpoint: &DomainOperationEndpoint) -> DiskObservation {
    match endpoint.expected_after() {
        JournalAfter::Known(value) => native(value),
        JournalAfter::Pending => DiskObservation::Absent,
    }
}

fn operation_is_terminal(record: &DomainOperationRecord) -> bool {
    record.effect().is_prepared_terminal()
}

fn set_endpoint_expected(endpoint: &mut DomainOperationEndpoint, value: &DiskObservation) {
    endpoint.set_expected_after(JournalAfter::Known(durable(value)));
}

fn private_artifact_leaf(operation_id: u32, artifact: ArtifactSlot) -> String {
    let slot = match artifact {
        ArtifactSlot::Custody => "custody",
        ArtifactSlot::Stage => "stage",
        ArtifactSlot::Discard => "discard",
    };
    format!("{operation_id}-{slot}")
}

fn workspace_for_operation(
    journal: &OptiScalerJournal,
    index: usize,
) -> Result<&renderpilot_domain::PrivateWorkspaceBinding, ServiceError> {
    let record = journal
        .operations()
        .get(index)
        .ok_or_else(|| crate::failed("operation ordinal is outside the journal"))?;
    let workspace_id = record
        .workspace_id()
        .ok_or_else(|| crate::failed("artifact-free operation has no private workspace"))?;
    journal
        .private_workspaces()
        .get(usize::try_from(workspace_id).map_err(|_| crate::failed("workspace id overflow"))?)
        .ok_or_else(|| crate::failed("operation workspace is outside the journal"))
}

fn derived_private_workspace_path(
    transaction_id: &str,
    journal: &OptiScalerJournal,
    workspace_id: u32,
) -> Result<PathBuf, ServiceError> {
    namespace::derived_workspace_path(transaction_id, journal, workspace_id)
}

/// One exact private artifact leaf, resolved through the identity-bound
/// workspace rather than reopened by its diagnostic path.
struct PrivateArtifact {
    namespace: crate::fs::PrivateNamespace,
    leaf: crate::fs::LeafName,
    path: PathBuf,
}

fn private_artifact(
    prepared: &PreparedFileMutation<'_>,
    index: usize,
    artifact: ArtifactSlot,
) -> Result<PrivateArtifact, ServiceError> {
    let operation_id = prepared
        .journal
        .operations()
        .get(index)
        .ok_or_else(|| crate::failed("operation ordinal is outside the journal"))?
        .operation_id();
    let workspace = workspace_for_operation(&prepared.journal, index)?;
    let workspace_path =
        derived_private_workspace_path(&prepared.id, &prepared.journal, workspace.workspace_id())?;
    let expected_identity = workspace
        .identity()
        .ok_or_else(|| crate::failed("private artifact workspace has no durable identity"))?;
    let (parent, leaf) = crate::fs::verified_parent(&workspace_path)?;
    let namespace = crate::fs::PrivateNamespace::reopen(&parent, &leaf, expected_identity)?;
    let artifact_leaf = private_artifact_leaf(operation_id, artifact);
    let leaf = crate::fs::LeafName::parse(std::ffi::OsStr::new(&artifact_leaf))?;
    let path = workspace_path.join(leaf.as_os_str());
    Ok(PrivateArtifact {
        namespace,
        leaf,
        path,
    })
}

fn observe_private_artifact(
    prepared: &PreparedFileMutation<'_>,
    index: usize,
    artifact: ArtifactSlot,
) -> Result<DiskObservation, ServiceError> {
    let artifact = private_artifact(prepared, index, artifact)?;
    Ok(
        match artifact
            .namespace
            .directory()
            .observe_leaf(&artifact.leaf)?
        {
            None => DiskObservation::Absent,
            Some(entry) => disk_observation(&entry)?,
        },
    )
}

fn set_artifact(
    record: &mut DomainOperationRecord,
    artifact: ArtifactSlot,
    value: &DiskObservation,
) {
    let slot = match artifact {
        ArtifactSlot::Custody => record.slots_mut().custody_mut(),
        ArtifactSlot::Stage => record.slots_mut().stage_mut(),
        ArtifactSlot::Discard => record.slots_mut().discard_mut(),
    };
    *slot = durable(value);
}

fn artifact(record: &DomainOperationRecord, artifact: ArtifactSlot) -> DiskObservation {
    native(match artifact {
        ArtifactSlot::Custody => record.slots().custody(),
        ArtifactSlot::Stage => record.slots().stage(),
        ArtifactSlot::Discard => record.slots().discard(),
    })
}
