//! Pure RenoDX DLSS-Fix companion ownership projection.
//!
//! This projection intentionally models only the companion's physical image
//! and the two persisted claim slots. It does not derive operations, alter
//! storage records, or make decisions about any unrelated add-on fields.

use std::fmt::Write as _;

use crate::{
    InstalledAddon, PathRef, Sha256Hash, TrackedSource, TrackedSourceRole, normalized_path_key,
};
use sha2::{Digest, Sha256};

use super::model::PeerTransitionError;

/// Exact bytes observed before a RenoDX DLSS-Fix transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenoDxDlssBeforeImage {
    /// The companion was absent at the observed path.
    Absent,
    /// The companion was present with its exact native image.
    Present {
        /// Stable native identity observed for the preimage.
        identity: String,
        /// SHA-256 digest of the preimage bytes.
        sha256: Sha256Hash,
        /// Byte length of the preimage.
        length: u64,
        /// Exact preimage bytes retained for replacement CAS.
        bytes: Vec<u8>,
    },
}

impl RenoDxDlssBeforeImage {
    /// Constructs an absent companion image.
    #[must_use]
    pub const fn absent() -> Self {
        Self::Absent
    }

    /// Constructs a present companion image.
    pub fn present(
        identity: impl Into<String>,
        sha256: Sha256Hash,
        length: u64,
        bytes: Vec<u8>,
    ) -> Result<Self, PeerTransitionError> {
        let identity = identity.into();
        validate_present_fields(&identity, &sha256, length, &bytes)?;
        Ok(Self::Present {
            identity: identity.trim().to_owned(),
            sha256,
            length,
            bytes,
        })
    }

    /// Returns whether the observed companion was present.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        matches!(self, Self::Present { .. })
    }

    /// Returns the native identity when the companion was present.
    #[must_use]
    pub fn identity(&self) -> Option<&str> {
        match self {
            Self::Absent => None,
            Self::Present { identity, .. } => Some(identity),
        }
    }

    /// Returns the observed SHA-256 when the companion was present.
    #[must_use]
    pub fn sha256(&self) -> Option<&Sha256Hash> {
        match self {
            Self::Absent => None,
            Self::Present { sha256, .. } => Some(sha256),
        }
    }

    /// Returns the observed byte length when the companion was present.
    #[must_use]
    pub const fn length(&self) -> Option<u64> {
        match self {
            Self::Absent => None,
            Self::Present { length, .. } => Some(*length),
        }
    }

    /// Returns the captured bytes when the companion was present.
    #[must_use]
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Absent => None,
            Self::Present { bytes, .. } => Some(bytes),
        }
    }

    fn validate(&self) -> Result<(), PeerTransitionError> {
        match self {
            Self::Absent => Ok(()),
            Self::Present {
                identity,
                sha256,
                length,
                bytes,
            } => validate_present_fields(identity, sha256, *length, bytes),
        }
    }
}

fn validate_present_fields(
    identity: &str,
    sha256: &Sha256Hash,
    length: u64,
    bytes: &[u8],
) -> Result<(), PeerTransitionError> {
    if identity.trim().is_empty() {
        return Err(PeerTransitionError::EmptyImageIdentity);
    }
    if bytes.len() as u64 != length {
        return Err(PeerTransitionError::InvalidRenoDxDlssClaim(
            "before image length does not match retained bytes",
        ));
    }
    let actual_sha256 = sha256_hex(bytes);
    if actual_sha256 != sha256.as_str() {
        return Err(PeerTransitionError::InvalidRenoDxDlssClaim(
            "before image SHA-256 does not match retained bytes",
        ));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(Sha256Hash::HEX_LENGTH);
    for byte in Sha256::digest(bytes) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Persisted ownership and provenance claim for the companion path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenoDxDlssClaim {
    created: bool,
    source: Option<TrackedSource>,
}

impl RenoDxDlssClaim {
    /// Constructs a claim. Any source, when present, must be the DLSS-Fix
    /// source slot; other tracked-source roles remain outside this projection.
    pub fn new(created: bool, source: Option<TrackedSource>) -> Result<Self, PeerTransitionError> {
        if let Some(source) = &source
            && source.role() != TrackedSourceRole::DlssFix
        {
            return Err(PeerTransitionError::InvalidRenoDxDlssSourceRole(
                source.role(),
            ));
        }
        Ok(Self { created, source })
    }

    /// Alias emphasizing reconstruction from the two persisted claim slots.
    pub fn from_parts(
        created: bool,
        source: Option<TrackedSource>,
    ) -> Result<Self, PeerTransitionError> {
        Self::new(created, source)
    }

    /// Constructs an absent, source-free claim.
    #[must_use]
    pub const fn absent() -> Self {
        Self {
            created: false,
            source: None,
        }
    }

    /// Returns whether the companion path is claimed as created by the peer.
    #[must_use]
    pub const fn created(&self) -> bool {
        self.created
    }

    /// Returns the optional DLSS-Fix provenance source slot.
    #[must_use]
    pub fn source(&self) -> Option<&TrackedSource> {
        self.source.as_ref()
    }
}

/// Before/after DLSS-Fix companion projection bound to one normalized path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenoDxDlssProjection {
    companion_path: PathRef,
    before_image: RenoDxDlssBeforeImage,
    before_claim: RenoDxDlssClaim,
    after_claim: RenoDxDlssClaim,
}

impl RenoDxDlssProjection {
    /// Constructs a validated DLSS-Fix projection.
    #[must_use]
    pub fn new(
        companion_path: PathRef,
        before_image: RenoDxDlssBeforeImage,
        before_claim: RenoDxDlssClaim,
        after_claim: RenoDxDlssClaim,
    ) -> Self {
        Self {
            companion_path,
            before_image,
            before_claim,
            after_claim,
        }
    }

    /// Alias emphasizing reconstruction from persisted and observed parts.
    #[must_use]
    pub fn from_parts(
        companion_path: PathRef,
        before_image: RenoDxDlssBeforeImage,
        before_claim: RenoDxDlssClaim,
        after_claim: RenoDxDlssClaim,
    ) -> Self {
        Self::new(companion_path, before_image, before_claim, after_claim)
    }

    /// Returns the normalized companion path.
    #[must_use]
    pub fn companion_path(&self) -> &PathRef {
        &self.companion_path
    }

    /// Returns the exact pre-transition companion image.
    #[must_use]
    pub fn before_image(&self) -> &RenoDxDlssBeforeImage {
        &self.before_image
    }

    /// Returns the persisted before claim.
    #[must_use]
    pub fn before_claim(&self) -> &RenoDxDlssClaim {
        &self.before_claim
    }

    /// Returns the persisted after claim.
    #[must_use]
    pub fn after_claim(&self) -> &RenoDxDlssClaim {
        &self.after_claim
    }

    /// Validates only this projection's claims against the peer records.
    ///
    /// The companion path is compared lexically through the shared normalized
    /// key. Every peer has at most one exact `DlssFix` source slot and its value
    /// must equal the projection claim. Every unrelated record field and entry
    /// is compared exactly before and after; this helper never modifies records.
    pub fn validate_against_peers(
        &self,
        before_peer: Option<&InstalledAddon>,
        after_peer: Option<&InstalledAddon>,
    ) -> Result<(), PeerTransitionError> {
        self.before_image.validate()?;
        validate_claim_against_peer(&self.companion_path, self.before_claim(), before_peer)?;
        validate_claim_against_peer(&self.companion_path, self.after_claim(), after_peer)?;
        match (before_peer, after_peer) {
            (None, None) => Ok(()),
            (Some(before), Some(after)) => {
                validate_unrelated_peer_fields(&self.companion_path, before, after)
            }
            (None, Some(_)) | (Some(_), None) => Err(PeerTransitionError::RenoDxDlssPeerMismatch(
                self.companion_path.clone(),
            )),
        }
    }
}

fn validate_claim_against_peer(
    companion_path: &PathRef,
    claim: &RenoDxDlssClaim,
    peer: Option<&InstalledAddon>,
) -> Result<(), PeerTransitionError> {
    let Some(peer) = peer else {
        if !claim.created() && claim.source().is_none() {
            return Ok(());
        }
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    };

    let companion_key = normalized_path_key(companion_path.as_str());
    if normalized_path_key(peer.addon_file().as_str()) == companion_key
        || peer
            .backed_up_files()
            .iter()
            .any(|path| normalized_path_key(path.as_str()) == companion_key)
        || peer
            .managed_files()
            .iter()
            .any(|file| normalized_path_key(file.path().as_str()) == companion_key)
    {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }
    let created_count = peer
        .created_files()
        .iter()
        .filter(|path| normalized_path_key(path.as_str()) == companion_key)
        .count();
    if (claim.created() && created_count != 1) || (!claim.created() && created_count != 0) {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }

    let mut fix_sources = peer
        .tracked_sources()
        .iter()
        .filter(|source| source.role() == TrackedSourceRole::DlssFix);
    let first_source = fix_sources.next();
    if fix_sources.next().is_some() {
        return Err(PeerTransitionError::InvalidRenoDxDlssClaim(
            "peer has duplicate DlssFix source slots",
        ));
    }
    if claim.source() != first_source {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }
    Ok(())
}

fn validate_unrelated_peer_fields(
    companion_path: &PathRef,
    before: &InstalledAddon,
    after: &InstalledAddon,
) -> Result<(), PeerTransitionError> {
    if before.game_id() != after.game_id()
        || before.kind() != after.kind()
        || before.addon_file() != after.addon_file()
        || before.addon_version() != after.addon_version()
        || before.backed_up_files() != after.backed_up_files()
        || before.managed_files() != after.managed_files()
        || before.installed_at() != after.installed_at()
        || before.updated_at() != after.updated_at()
        || before.host_kind() != after.host_kind()
        || before.reshade_channel() != after.reshade_channel()
        || before.registered_exe_path() != after.registered_exe_path()
    {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }

    let companion_key = normalized_path_key(companion_path.as_str());
    let created_equal = before
        .created_files()
        .iter()
        .filter(|path| normalized_path_key(path.as_str()) != companion_key)
        .eq(after
            .created_files()
            .iter()
            .filter(|path| normalized_path_key(path.as_str()) != companion_key));
    if !created_equal {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }

    let sources_equal = before
        .tracked_sources()
        .iter()
        .filter(|source| source.role() != TrackedSourceRole::DlssFix)
        .eq(after
            .tracked_sources()
            .iter()
            .filter(|source| source.role() != TrackedSourceRole::DlssFix));
    if !sources_equal {
        return Err(PeerTransitionError::RenoDxDlssPeerMismatch(
            companion_path.clone(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AddonKind, GameId, InstalledAddonHostKind, ManagedAddonFile, ManagedFileBaseline};

    fn path(value: &str) -> PathRef {
        PathRef::new(format!("C:/Games/Test/{value}")).expect("path")
    }

    fn hash() -> Sha256Hash {
        Sha256Hash::new("a".repeat(64)).expect("hash")
    }

    fn hash_bytes(bytes: &[u8]) -> Sha256Hash {
        Sha256Hash::new(sha256_hex(bytes)).expect("hash")
    }

    fn source(role: TrackedSourceRole) -> TrackedSource {
        TrackedSource::new(role, "https://example.test/dlss-fix", None, "digest")
    }

    fn peer(created: &[&str], sources: Vec<TrackedSource>) -> InstalledAddon {
        let mut peer = InstalledAddon::new(
            GameId::new("manual:renodx-dlss").expect("game"),
            AddonKind::RenoDx,
            path("renodx-test.addon64"),
        );
        for file in created {
            peer = peer.with_created_file(path(file));
        }
        peer.with_tracked_sources(sources)
    }

    #[test]
    fn present_image_trims_and_rejects_empty_identity() {
        let image = RenoDxDlssBeforeImage::present(
            "  native-dlss  ",
            hash_bytes(b"abc"),
            3,
            b"abc".to_vec(),
        )
        .expect("image");
        assert_eq!(image.identity(), Some("native-dlss"));
        assert_eq!(image.bytes(), Some(b"abc".as_slice()));
        assert!(RenoDxDlssBeforeImage::present(" ", hash(), 0, Vec::new()).is_err());
    }

    #[test]
    fn present_image_rejects_retained_length_or_digest_drift() {
        assert!(
            RenoDxDlssBeforeImage::present("native-dlss", hash_bytes(b"abc"), 2, b"abc".to_vec())
                .is_err()
        );
        assert!(RenoDxDlssBeforeImage::present("native-dlss", hash(), 3, b"abc".to_vec()).is_err());

        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::Present {
                identity: "native-dlss".to_owned(),
                sha256: hash(),
                length: 3,
                bytes: b"abc".to_vec(),
            },
            RenoDxDlssClaim::absent(),
            RenoDxDlssClaim::absent(),
        );
        assert!(projection.validate_against_peers(None, None).is_err());
    }

    #[test]
    fn claim_rejects_non_dlss_source_roles() {
        let error = RenoDxDlssClaim::new(false, Some(source(TrackedSourceRole::AddonPayload)))
            .expect_err("only DlssFix is admitted");
        assert!(matches!(
            error,
            PeerTransitionError::InvalidRenoDxDlssSourceRole(TrackedSourceRole::AddonPayload)
        ));
    }

    #[test]
    fn projection_accepts_normalized_created_path_and_exact_source_slot() {
        let source = source(TrackedSourceRole::DlssFix);
        let before_claim = RenoDxDlssClaim::new(true, Some(source.clone())).expect("claim");
        let after_claim = RenoDxDlssClaim::new(false, None).expect("claim");
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::absent(),
            before_claim,
            after_claim,
        );
        let before = peer(&[r"NVNGX_DLSS.DLL"], vec![source]);
        let after = peer(&[], Vec::new());
        projection
            .validate_against_peers(Some(&before), Some(&after))
            .expect("claims match");
    }

    #[test]
    fn projection_rejects_created_path_or_source_drift() {
        let claim =
            RenoDxDlssClaim::new(true, Some(source(TrackedSourceRole::DlssFix))).expect("claim");
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::absent(),
            claim.clone(),
            claim,
        );
        let wrong_path = peer(&["other.dll"], vec![source(TrackedSourceRole::DlssFix)]);
        assert!(
            projection
                .validate_against_peers(Some(&wrong_path), Some(&wrong_path))
                .is_err()
        );
        let wrong_source = peer(
            &["nvngx_dlss.dll"],
            vec![source(TrackedSourceRole::DlssFix).with_channel("nightly")],
        );
        assert!(
            projection
                .validate_against_peers(Some(&wrong_source), Some(&wrong_source))
                .is_err()
        );
    }

    #[test]
    fn projection_accepts_claim_only_companion_transition() {
        let source = source(TrackedSourceRole::DlssFix);
        let before_claim = RenoDxDlssClaim::absent();
        let after_claim = RenoDxDlssClaim::new(true, Some(source.clone())).expect("claim");
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::absent(),
            before_claim,
            after_claim,
        );
        let before = peer(&[], Vec::new());
        let after = peer(&["NVNGX_DLSS.DLL"], vec![source]);
        projection
            .validate_against_peers(Some(&before), Some(&after))
            .expect("only companion claims changed");
    }

    #[test]
    fn claim_only_contract_remains_valid_at_consumer_boundaries() {
        let source = source(TrackedSourceRole::DlssFix);
        let before = peer(&["nvngx_dlss.dll"], Vec::new());
        let after = peer(&["NVNGX_DLSS.DLL"], vec![source.clone()]);
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::present(
                "native-dlss",
                hash_bytes(b"existing"),
                b"existing".len() as u64,
                b"existing".to_vec(),
            )
            .expect("before image"),
            RenoDxDlssClaim::new(true, None).expect("claim"),
            RenoDxDlssClaim::new(true, Some(source)).expect("claim"),
        );
        let contract =
            crate::PeerTransitionContract::derive_physical_with_renodx_reshade_ini_and_dlss(
                crate::PeerTransitionContext::new(
                    Some(&before),
                    Some(&after),
                    None,
                    None,
                    crate::ProxyPeerRoute::DurableDisjoint,
                ),
                None,
                Vec::new(),
                projection,
            )
            .expect("claim-only contract");

        contract.validate_intents().expect("consumer intents");
        contract.validate_evidence(&[]).expect("empty evidence");
        contract.validate_preimages(&[]).expect("empty preimages");
    }

    #[test]
    fn projection_rejects_unrelated_peer_field_changes() {
        let claim = RenoDxDlssClaim::new(true, None).expect("claim");
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::absent(),
            claim.clone(),
            claim,
        );
        let before = peer(&["nvngx_dlss.dll"], Vec::new());

        let after = peer(&["nvngx_dlss.dll"], Vec::new()).with_addon_version("new-version");
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after = peer(&["nvngx_dlss.dll"], Vec::new())
            .with_timestamps(Some(1), Some(2))
            .with_host_kind(InstalledAddonHostKind::Proxy)
            .with_reshade_channel("stable")
            .with_registered_exe_path(path("game.exe"));
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after = peer(&["nvngx_dlss.dll", "other.dll"], Vec::new());
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after =
            peer(&["nvngx_dlss.dll"], Vec::new()).with_backed_up_file(path("other.dll.bak"));
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let managed =
            ManagedAddonFile::owned(path("managed.dll"), ManagedFileBaseline::Absent, hash());
        let after = peer(&["nvngx_dlss.dll"], Vec::new())
            .try_with_managed_files(vec![managed])
            .expect("managed peer");
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after = peer(
            &["nvngx_dlss.dll"],
            vec![source(TrackedSourceRole::HostBinary)],
        );
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after = peer(&["nvngx_dlss.dll", "other.dll", "other.dll"], Vec::new());
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );
    }

    #[test]
    fn projection_rejects_missing_peer_or_companion_in_non_created_slots() {
        let claim = RenoDxDlssClaim::new(true, None).expect("claim");
        let projection = RenoDxDlssProjection::new(
            path("nvngx_dlss.dll"),
            RenoDxDlssBeforeImage::absent(),
            claim.clone(),
            claim,
        );
        let before = peer(&["nvngx_dlss.dll"], Vec::new());
        assert!(
            projection
                .validate_against_peers(Some(&before), None)
                .is_err()
        );

        let after =
            peer(&["nvngx_dlss.dll"], Vec::new()).with_backed_up_file(path("nvngx_dlss.dll"));
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );

        let after = InstalledAddon::new(
            GameId::new("manual:renodx-dlss").expect("game"),
            AddonKind::RenoDx,
            path("nvngx_dlss.dll"),
        );
        assert!(
            projection
                .validate_against_peers(Some(&before), Some(&after))
                .is_err()
        );
    }
}
