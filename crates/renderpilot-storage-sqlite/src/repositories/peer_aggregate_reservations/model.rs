use std::fmt;

use renderpilot_application::{AppError, AppResult};
use renderpilot_domain::GameId;

use crate::peer_runtime::aggregate::AggregateGeneration;

/// Closed set of game aggregate participants that may own a reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerAggregateKind {
    OptiScalerJournal,
    SharedPeer,
    Metadata,
}

impl PeerAggregateKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OptiScalerJournal => "optiscaler_journal",
            Self::SharedPeer => "shared_peer",
            Self::Metadata => "metadata",
        }
    }

    pub(crate) const fn binding(self) -> PeerAggregateBinding {
        match self {
            Self::OptiScalerJournal => PeerAggregateBinding::File,
            Self::SharedPeer => PeerAggregateBinding::Shared,
            Self::Metadata => PeerAggregateBinding::Metadata,
        }
    }

    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "optiscaler_journal" => Ok(Self::OptiScalerJournal),
            "shared_peer" => Ok(Self::SharedPeer),
            "metadata" => Ok(Self::Metadata),
            _ => Err(invalid_value("aggregate kind", value)),
        }
    }
}

impl fmt::Display for PeerAggregateKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Physical pending-table binding derived from [`PeerAggregateKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerAggregateBinding {
    File,
    Shared,
    Metadata,
}

impl PeerAggregateBinding {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Shared => "shared",
            Self::Metadata => "metadata",
        }
    }

    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "file" => Ok(Self::File),
            "shared" => Ok(Self::Shared),
            "metadata" => Ok(Self::Metadata),
            _ => Err(invalid_value("pending binding", value)),
        }
    }
}

impl fmt::Display for PeerAggregateBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Closed durable reservation state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PeerAggregateReservationState {
    Preparing,
    Prepared,
    Committed,
}

impl PeerAggregateReservationState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Prepared => "prepared",
            Self::Committed => "committed",
        }
    }

    fn parse(value: &str) -> AppResult<Self> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "prepared" => Ok(Self::Prepared),
            "committed" => Ok(Self::Committed),
            _ => Err(invalid_value("reservation state", value)),
        }
    }
}

impl fmt::Display for PeerAggregateReservationState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Fully parsed reservation identity and lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerAggregateReservation {
    game_id: GameId,
    operation_id: String,
    kind: PeerAggregateKind,
    binding: PeerAggregateBinding,
    state: PeerAggregateReservationState,
    expected_revision: AggregateGeneration,
    created_at: i64,
    updated_at: i64,
}

/// Untyped SQLite projection, kept separate from the validated reservation.
pub(crate) struct RawPeerAggregateReservation {
    pub(crate) game_id: String,
    pub(crate) operation_id: String,
    pub(crate) kind: String,
    pub(crate) binding: String,
    pub(crate) state: String,
    pub(crate) expected_revision: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl PeerAggregateReservation {
    pub(crate) fn from_row(raw: RawPeerAggregateReservation) -> AppResult<Self> {
        let RawPeerAggregateReservation {
            game_id,
            operation_id,
            kind,
            binding,
            state,
            expected_revision,
            created_at,
            updated_at,
        } = raw;
        let game_id = GameId::new(game_id).map_err(|error| {
            AppError::storage_failed(format!("invalid reservation game id: {error}"))
        })?;
        if operation_id.trim().is_empty()
            || operation_id.contains('\0')
            || operation_id.len() > crate::peer_runtime::aggregate::MAX_METADATA_BYTES
        {
            return Err(AppError::storage_failed(
                "invalid reservation operation identity",
            ));
        }
        let kind = PeerAggregateKind::parse(&kind)?;
        let binding = PeerAggregateBinding::parse(&binding)?;
        if binding != kind.binding() {
            return Err(AppError::storage_failed(format!(
                "reservation binding `{binding}` does not match aggregate kind `{kind}`"
            )));
        }
        let state = PeerAggregateReservationState::parse(&state)?;
        let expected_revision = AggregateGeneration::from_persisted(expected_revision)?;
        if created_at < 0 || updated_at < created_at {
            return Err(AppError::storage_failed(
                "reservation timestamps are outside their valid range",
            ));
        }
        Ok(Self {
            game_id,
            operation_id,
            kind,
            binding,
            state,
            expected_revision,
            created_at,
            updated_at,
        })
    }

    pub(crate) fn game_id(&self) -> &GameId {
        &self.game_id
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub(crate) const fn kind(&self) -> PeerAggregateKind {
        self.kind
    }

    pub(crate) const fn binding(&self) -> PeerAggregateBinding {
        self.binding
    }

    pub(crate) const fn state(&self) -> PeerAggregateReservationState {
        self.state
    }

    pub(crate) const fn expected_revision(&self) -> AggregateGeneration {
        self.expected_revision
    }
}

fn invalid_value(field: &str, value: &str) -> AppError {
    AppError::storage_failed(format!("invalid reservation {field} `{value}`"))
}
