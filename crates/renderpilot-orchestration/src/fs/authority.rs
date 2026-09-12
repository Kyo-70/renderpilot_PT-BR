//! Native, capability-based authority for OptiScaler's durable participants.
//!
//! A `Path` is accepted only while acquiring a capability. Once acquired, all
//! participant operations are relative to retained native handles and a
//! validated [`LeafName`]. The display path carried by a capability is
//! diagnostic metadata; it is never used to authorize a mutation.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

use sha2::Digest;

use crate::ServiceError;

mod dir;
mod entry;
mod namespaces;
mod platform;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use dir::core::*;
pub(crate) use entry::VerifiedEntry;
pub(crate) use namespaces::{ControlNamespace, PrivateNamespace};
#[cfg(target_os = "linux")]
pub(crate) use platform::linux::*;
#[cfg(windows)]
pub(crate) use platform::windows::*;
pub(crate) use types::*;
