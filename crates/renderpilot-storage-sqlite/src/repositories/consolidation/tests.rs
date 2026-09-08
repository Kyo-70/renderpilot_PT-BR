//! Consolidation test module plumbing.

use renderpilot_application::{AppError, ComponentRepository, GameRepository};
use renderpilot_domain::{
    ComponentFile, ComponentId, ComponentKind, GameId, GameIdentity, GameInstallation, GameRuntime,
    Launcher, LibraryComponent, LibraryTechnology, PathRef, Platform, RootAuthority, Swappability,
};

use super::execution::move_optiscaler_aggregate;
use super::{ComponentRekey, ConsolidationPlan, ConsolidationSource};
use crate::{ScanWriteUnit, SqliteStorage};
use rusqlite::named_params;

mod aggregate;
mod components;
mod fixtures;
mod optiscaler;

use fixtures::{
    assert_foreign_keys_are_valid, assert_game_and_optiscaler_state_remain,
    assert_optiscaler_blocked, component, consolidation_plan, game, game_exists,
    optiscaler_row_count, optiscaler_state_topology_id, optiscaler_topology_id,
    seed_all_scoped_state, seed_optiscaler_complete,
    seed_optiscaler_complete_with_identity_mismatch, seed_optiscaler_rows, seed_optiscaler_state,
    seed_optiscaler_topology, target_dir,
};
