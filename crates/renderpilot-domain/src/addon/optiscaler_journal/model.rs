// Contract-owned, domain-neutral wire for an OptiScaler file mutation.
//
// This module intentionally knows nothing about a filesystem, a native handle,
// or an orchestration implementation.  It records the observations and the
// ordered program that the native adapter is required to execute.  The
// journal has no format/protocol version: schema revision belongs to the
// persistence envelope, while these types describe the current contract.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    FileOwnership, FileReceipt, NormalizedPathRelation, Sha256Hash, normalized_path_key,
    normalized_path_relation,
};

include!("model/observation.rs");
include!("model/operation.rs");
include!("model/slots.rs");
include!("model/namespace_bindings.rs");
include!("model/states.rs");
