//! Durable execution of a fully prepared OptiScaler apply.

mod commit;
mod execution;
mod helpers;
mod plan;
mod retained_fsr;
mod runtime;

pub(in crate::addons::optiscaler) use commit::apply_release;
pub(in crate::addons::optiscaler) use helpers::{
    ini_library_path, module_library_path, release_has_private_runtime,
};
pub(in crate::addons::optiscaler) use plan::plan_apply_peer_transition;
