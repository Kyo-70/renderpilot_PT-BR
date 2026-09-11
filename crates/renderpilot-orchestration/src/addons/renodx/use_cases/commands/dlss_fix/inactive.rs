//! DLSS-Fix route for games without persisted topology.

mod lifecycle;

pub(crate) use lifecycle::{
    install_dlss_fix, retry_dlss_fix_recovery, uninstall_dlss_fix, update_dlss_fix,
};
