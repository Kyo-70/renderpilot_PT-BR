use renderpilot_application::AppResult;
use renderpilot_domain::{
    GameId, GameProxyTopology, InstalledAddon, OptiScalerInstallState, PlannedGameProxyTopology,
    normalized_path_key,
};

/// Maximum byte length admitted for aggregate operation metadata.
pub const MAX_METADATA_BYTES: usize = 1_048_576;

fn invalid(message: &str) -> renderpilot_application::AppError {
    renderpilot_application::AppError::invalid_input(message)
}

fn validate_operation_id(operation_id: &str) -> AppResult<()> {
    if operation_id.trim().is_empty() || operation_id.contains('\0') {
        return Err(invalid("aggregate operation identity is invalid"));
    }
    if operation_id.len() > MAX_METADATA_BYTES {
        return Err(invalid(
            "aggregate operation identity exceeds the supported limit",
        ));
    }
    Ok(())
}

/// Monotonic generation of the complete game aggregate.
///
/// A single scalar is intentionally used for the whole durable aggregate. It
/// cannot be confused with an individual repository timestamp or participant
/// revision, and its representation is bounded by SQLite's signed integer
/// range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AggregateGeneration(u64);

impl AggregateGeneration {
    /// Returns the initial generation before any aggregate mutation commits.
    #[must_use]
    pub const fn initial() -> Self {
        Self(0)
    }

    /// Validates one SQLite generation value read from durable storage.
    pub fn from_persisted(value: i64) -> AppResult<Self> {
        let generation =
            u64::try_from(value).map_err(|_| invalid("aggregate generation cannot be negative"))?;
        Ok(Self(generation))
    }

    /// Returns this generation in SQLite's signed integer representation.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        i64::try_from(self.0).unwrap_or(i64::MAX)
    }

    /// Advances this generation without exceeding SQLite's integer range.
    pub fn checked_successor(self) -> AppResult<Self> {
        if self.0 >= i64::MAX as u64 {
            return Err(invalid("aggregate generation exceeds SQLite integer range"));
        }
        Ok(Self(self.0 + 1))
    }
}

/// Immutable snapshot of the three game-scoped participants before a peer mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateBefore {
    game_id: GameId,
    state: Option<OptiScalerInstallState>,
    topology: Option<GameProxyTopology>,
    peer: Option<InstalledAddon>,
}

impl AggregateBefore {
    /// Validates and constructs a participant snapshot for one game.
    pub fn new(
        game_id: GameId,
        state: Option<OptiScalerInstallState>,
        topology: Option<GameProxyTopology>,
        peer: Option<InstalledAddon>,
    ) -> AppResult<Self> {
        validate_participants(&game_id, state.as_ref(), topology.as_ref(), peer.as_ref())?;
        Ok(Self {
            game_id,
            state,
            topology,
            peer,
        })
    }

    #[must_use]
    /// Returns the game identity covered by this snapshot.
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }
    #[must_use]
    /// Returns the OptiScaler state captured in the snapshot, if present.
    pub fn state(&self) -> Option<&OptiScalerInstallState> {
        self.state.as_ref()
    }
    #[must_use]
    /// Returns the proxy topology captured in the snapshot, if present.
    pub fn topology(&self) -> Option<&GameProxyTopology> {
        self.topology.as_ref()
    }
    #[must_use]
    /// Returns the installed peer captured in the snapshot, if present.
    pub fn peer(&self) -> Option<&InstalledAddon> {
        self.peer.as_ref()
    }
}

fn same_optional<T>(
    left: Option<&T>,
    right: Option<&T>,
    equivalent: impl FnOnce(&T, &T) -> bool,
) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => equivalent(left, right),
        _ => false,
    }
}

/// Validated participant image planned for the next aggregate generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAggregateAfter {
    game_id: GameId,
    state: Option<OptiScalerInstallState>,
    topology: Option<PlannedGameProxyTopology>,
    peer: Option<InstalledAddon>,
}

impl PlannedAggregateAfter {
    /// Validates and constructs the planned participant image.
    pub fn new(
        game_id: GameId,
        state: Option<OptiScalerInstallState>,
        topology: Option<PlannedGameProxyTopology>,
        peer: Option<InstalledAddon>,
    ) -> AppResult<Self> {
        validate_participants(&game_id, state.as_ref(), None, peer.as_ref())?;
        if let Some(topology) = topology.as_ref() {
            validate_planned_topology(&game_id, topology)?;
        }
        Ok(Self {
            game_id,
            state,
            topology,
            peer,
        })
    }

    #[must_use]
    /// Returns the game identity covered by this image.
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }
    #[must_use]
    /// Returns the planned OptiScaler state, if present.
    pub fn state(&self) -> Option<&OptiScalerInstallState> {
        self.state.as_ref()
    }
    #[must_use]
    /// Returns the planned proxy topology, if present.
    pub fn topology(&self) -> Option<&PlannedGameProxyTopology> {
        self.topology.as_ref()
    }
    #[must_use]
    /// Returns the planned installed peer, if present.
    pub fn peer(&self) -> Option<&InstalledAddon> {
        self.peer.as_ref()
    }
}

/// Fully materialized participant image after a successful aggregate commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateAfter {
    game_id: GameId,
    state: Option<OptiScalerInstallState>,
    topology: Option<GameProxyTopology>,
    peer: Option<InstalledAddon>,
}

impl AggregateAfter {
    /// Validates and constructs one fully materialized aggregate image.
    pub fn new(
        game_id: GameId,
        state: Option<OptiScalerInstallState>,
        topology: Option<GameProxyTopology>,
        peer: Option<InstalledAddon>,
    ) -> AppResult<Self> {
        validate_participants(&game_id, state.as_ref(), topology.as_ref(), peer.as_ref())?;
        Ok(Self {
            game_id,
            state,
            topology,
            peer,
        })
    }

    /// Returns the game identity covered by this image.
    #[must_use]
    pub fn game_id(&self) -> &GameId {
        &self.game_id
    }
    /// Returns the materialized OptiScaler state, if present.
    #[must_use]
    pub fn state(&self) -> Option<&OptiScalerInstallState> {
        self.state.as_ref()
    }
    /// Returns the materialized proxy topology, if present.
    #[must_use]
    pub fn topology(&self) -> Option<&GameProxyTopology> {
        self.topology.as_ref()
    }
    /// Returns the materialized installed peer, if present.
    #[must_use]
    pub fn peer(&self) -> Option<&InstalledAddon> {
        self.peer.as_ref()
    }

    /// Compares the exact persisted participant image while ignoring only the
    /// database-managed timestamps of participant rows.
    #[must_use]
    pub(crate) fn persistence_equivalent(&self, other: &Self) -> bool {
        self.game_id == other.game_id
            && same_optional(self.state.as_ref(), other.state.as_ref(), |left, right| {
                left.eq_ignoring_persistence_timestamps(right)
            })
            && self.topology == other.topology
            && same_optional(self.peer.as_ref(), other.peer.as_ref(), |left, right| {
                left.eq_ignoring_persistence_timestamps(right)
            })
    }
}

/// Operation identity and before/after images for one aggregate CAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameAggregateMutation {
    operation_id: String,
    before: AggregateBefore,
    planned_after: PlannedAggregateAfter,
}

impl GameAggregateMutation {
    /// Validates and constructs a game aggregate mutation.
    pub fn new(
        operation_id: impl Into<String>,
        before: AggregateBefore,
        planned_after: PlannedAggregateAfter,
    ) -> AppResult<Self> {
        let operation_id = operation_id.into();
        validate_operation_id(&operation_id)?;
        if before.game_id() != planned_after.game_id() {
            return Err(invalid("aggregate game identity changed during mutation"));
        }
        Ok(Self {
            operation_id,
            before,
            planned_after,
        })
    }

    #[must_use]
    /// Returns the caller-supplied operation identity.
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    #[must_use]
    /// Returns the immutable aggregate image observed before mutation.
    pub fn before(&self) -> &AggregateBefore {
        &self.before
    }
    #[must_use]
    /// Returns the validated participant image planned after mutation.
    pub fn planned_after(&self) -> &PlannedAggregateAfter {
        &self.planned_after
    }
}

/// Durable result of one committed aggregate mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedAggregate {
    operation_id: String,
    generation: AggregateGeneration,
    after: AggregateAfter,
}

impl CommittedAggregate {
    /// Validates and constructs a committed aggregate result.
    pub fn new(
        operation_id: impl Into<String>,
        generation: AggregateGeneration,
        after: AggregateAfter,
    ) -> AppResult<Self> {
        let operation_id = operation_id.into();
        validate_operation_id(&operation_id)?;
        Ok(Self {
            operation_id,
            generation,
            after,
        })
    }

    /// Returns the operation identity that committed this image.
    #[must_use]
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    /// Returns the durable generation assigned by the aggregate commit.
    #[must_use]
    pub const fn generation(&self) -> AggregateGeneration {
        self.generation
    }
    /// Returns the committed participant image.
    #[must_use]
    pub fn after(&self) -> &AggregateAfter {
        &self.after
    }
}

fn validate_participants(
    game_id: &GameId,
    state: Option<&OptiScalerInstallState>,
    topology: Option<&GameProxyTopology>,
    peer: Option<&InstalledAddon>,
) -> AppResult<()> {
    if let Some(state) = state {
        if &state.game_id != game_id {
            return Err(invalid("OptiScaler state belongs to another game"));
        }
        state
            .validate()
            .map_err(|error| invalid(&format!("invalid OptiScaler state: {error}")))?;
    }
    if let Some(topology) = topology {
        if &topology.game_id != game_id {
            return Err(invalid("proxy topology belongs to another game"));
        }
        topology
            .validate()
            .map_err(|error| invalid(&format!("invalid proxy topology: {error}")))?;
    }
    if let Some(peer) = peer
        && peer.game_id() != game_id
    {
        return Err(invalid("installed peer belongs to another game"));
    }
    Ok(())
}

fn validate_planned_topology(
    game_id: &GameId,
    topology: &PlannedGameProxyTopology,
) -> AppResult<()> {
    match topology {
        PlannedGameProxyTopology::Exact(topology) => {
            if &topology.game_id != game_id {
                return Err(invalid("planned proxy topology belongs to another game"));
            }
            topology
                .validate()
                .map_err(|error| invalid(&format!("invalid planned proxy topology: {error}")))?;
        }
        PlannedGameProxyTopology::ObservedOwnedDownstream {
            id,
            game_id: planned_game_id,
            root_slot,
            outer,
            downstream_path,
            downstream_origin,
            planned_length,
            ..
        } => {
            if planned_game_id != game_id {
                return Err(invalid("planned proxy topology belongs to another game"));
            }
            if id.trim().is_empty() {
                return Err(invalid("planned proxy topology id must not be empty"));
            }
            if normalized_path_key(root_slot.as_str()) != normalized_path_key(outer.path.as_str()) {
                return Err(invalid(
                    "planned proxy topology root differs from outer path",
                ));
            }
            if normalized_path_key(downstream_path.as_str())
                == normalized_path_key(downstream_origin.as_str())
            {
                return Err(invalid(
                    "planned proxy topology downstream origin equals active path",
                ));
            }
            let root_key = normalized_path_key(root_slot.as_str());
            if normalized_path_key(downstream_path.as_str()) == root_key
                || normalized_path_key(downstream_origin.as_str()) != root_key
            {
                return Err(invalid(
                    "planned proxy topology paths are not bound to the root slot",
                ));
            }
            if *planned_length > i64::MAX as u64 {
                return Err(invalid(
                    "planned proxy topology length exceeds storage range",
                ));
            }
            outer
                .receipt
                .validate()
                .map_err(|_| invalid("planned proxy topology outer receipt is invalid"))?;
        }
    }
    Ok(())
}
