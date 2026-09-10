use renderpilot_domain::{ManagedAddonFile, PathRef, Version};

use crate::peer_mutation_executor::VerifiedPeerFile;

pub(super) enum PersistedDlss {
    None,
    Reused(ManagedAddonFile),
    Owned(ManagedAddonFile),
}

pub(super) enum InputKind {
    Preserve,
    Full,
}

pub(super) struct LiveImage<'a> {
    pub(super) file: &'a VerifiedPeerFile,
    pub(super) bytes: &'a [u8],
}

pub(super) struct LiveDlss<'a> {
    pub(super) image: LiveImage<'a>,
    pub(super) version: Version,
}

pub(super) struct BundledDlss {
    pub(super) bytes: Vec<u8>,
    pub(super) digest: renderpilot_domain::Sha256Hash,
    pub(super) version: Version,
}

pub(super) enum DlssAction {
    Noop {
        binding: Option<ManagedAddonFile>,
    },
    Create {
        binding: ManagedAddonFile,
        bytes: Vec<u8>,
    },
    Acquire {
        binding: ManagedAddonFile,
        live: LiveOwnedImage,
        bytes: Vec<u8>,
    },
    Replace {
        binding: ManagedAddonFile,
        live: LiveOwnedImage,
        bytes: Vec<u8>,
    },
    Release {
        target: PathRef,
        live: LiveOwnedImage,
        sidecar: Option<LiveOwnedImage>,
        baseline: Option<Vec<u8>>,
    },
    CascadeRelease,
}

pub(super) struct LiveOwnedImage {
    pub(super) file: VerifiedPeerFile,
    pub(super) bytes: Vec<u8>,
}
