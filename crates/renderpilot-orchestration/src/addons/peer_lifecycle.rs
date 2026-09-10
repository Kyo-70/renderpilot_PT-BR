//! Shared active-topology peer planning boundary.
//!
//! This module freezes the route, roots, O1 observations, aggregate snapshots,
//! payload mapping, and ancestor plan before a durable reservation is made.
//! Lifecycle adapters consume the package; they do not repeat route or root
//! inference independently.

mod aggregate_membership;
pub(crate) mod package;
mod roots;
mod route;

pub(crate) use aggregate_membership::{
    PeerAggregateMembershipCommitInputs, PeerAggregateMembershipPackage,
    PeerAggregateMembershipRequest,
};
pub(crate) use package::PeerMutationPackage;
pub(crate) use roots::PeerRoots;
