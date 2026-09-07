//! Proxy-chain topology persistence.

use renderpilot_application::{AppResult, ProxyTopologyRepository};
use renderpilot_domain::{GameId, GameProxyTopology};
use rusqlite::{Connection, OptionalExtension, Row, Transaction};

use crate::error::storage_error;
use crate::repositories::observation::RowObservation;
use crate::{mapping, sqlite_clock};

use super::SqliteStorage;

const TOPOLOGY_SELECT_SQL: &str =
    "SELECT id, topology_json FROM game_proxy_topologies WHERE game_id=?1";

impl ProxyTopologyRepository for SqliteStorage {
    fn get_proxy_topology(&self, game_id: &GameId) -> AppResult<Option<GameProxyTopology>> {
        self.with_connection(|connection| {
            observe_on_connection(connection, game_id)?.into_optional()
        })
    }
}

pub(crate) fn observe_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<RowObservation<GameProxyTopology>> {
    observe_raw(
        transaction
            .query_row(
                TOPOLOGY_SELECT_SQL,
                [game_id.as_str()],
                raw_topology_from_row,
            )
            .optional(),
        game_id,
    )
}

pub(crate) fn get_within_transaction(
    transaction: &Transaction<'_>,
    game_id: &GameId,
) -> AppResult<Option<GameProxyTopology>> {
    observe_within_transaction(transaction, game_id)?.into_optional()
}

fn observe_on_connection(
    connection: &Connection,
    game_id: &GameId,
) -> AppResult<RowObservation<GameProxyTopology>> {
    observe_raw(
        connection
            .query_row(
                TOPOLOGY_SELECT_SQL,
                [game_id.as_str()],
                raw_topology_from_row,
            )
            .optional(),
        game_id,
    )
}

fn observe_raw(
    result: rusqlite::Result<Option<RawProxyTopology>>,
    game_id: &GameId,
) -> AppResult<RowObservation<GameProxyTopology>> {
    match result.map_err(storage_error)? {
        None => Ok(RowObservation::Missing),
        Some(raw) => Ok(match decode_topology(&raw, game_id) {
            Ok(topology) => RowObservation::Present(topology),
            Err(error) => RowObservation::Invalid(error),
        }),
    }
}

#[derive(Debug)]
struct RawProxyTopology {
    row_id: String,
    json: String,
}

fn raw_topology_from_row(row: &Row<'_>) -> rusqlite::Result<RawProxyTopology> {
    Ok(RawProxyTopology {
        row_id: row.get(0)?,
        json: row.get(1)?,
    })
}

fn decode_topology(raw: &RawProxyTopology, game_id: &GameId) -> AppResult<GameProxyTopology> {
    let topology: GameProxyTopology = mapping::deserialize_json(&raw.json)?;
    topology.validate().map_err(crate::error::invalid_row)?;
    if topology.id != raw.row_id {
        return Err(crate::error::invalid_row(
            "proxy topology id does not match its row",
        ));
    }
    if &topology.game_id != game_id {
        return Err(crate::error::invalid_row(
            "proxy topology game_id does not match its row",
        ));
    }
    Ok(topology)
}

pub(crate) fn upsert_within_transaction(
    transaction: &rusqlite::Transaction<'_>,
    topology: &GameProxyTopology,
) -> AppResult<()> {
    topology.validate().map_err(crate::error::invalid_row)?;
    let now_ms = sqlite_clock::now_ms(transaction)?;
    transaction
        .execute(
            "INSERT INTO game_proxy_topologies (id, game_id, topology_json, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(game_id) DO UPDATE SET \
             id=excluded.id, topology_json=excluded.topology_json, updated_at=excluded.updated_at",
            rusqlite::params![
                topology.id,
                topology.game_id.as_str(),
                mapping::serialize_json(topology)?,
                now_ms
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

pub(crate) fn delete_within_transaction(
    transaction: &rusqlite::Transaction<'_>,
    game_id: &GameId,
) -> AppResult<()> {
    transaction
        .execute(
            "DELETE FROM game_proxy_topologies WHERE game_id=?1",
            [game_id.as_str()],
        )
        .map_err(storage_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use renderpilot_domain::{
        FileReceipt, PathRef, ProxyImplementation, ProxyLink, ProxyRootPrestate, Sha256Hash,
    };

    fn topology(game_id: &GameId, id: &str) -> GameProxyTopology {
        let root = PathRef::new("C:/Games/Test/dxgi.dll").expect("root");
        GameProxyTopology {
            id: id.to_owned(),
            game_id: game_id.clone(),
            root_slot: root.clone(),
            outer: ProxyLink {
                implementation: ProxyImplementation::OptiScaler,
                path: root,
                receipt: FileReceipt::owned(
                    "topology-observation",
                    Sha256Hash::new("a".repeat(64)).expect("digest"),
                )
                .expect("receipt"),
            },
            downstream: None,
            downstream_origin: None,
            root_prestate: ProxyRootPrestate::Absent,
        }
    }

    #[test]
    fn topology_observation_distinguishes_invalid_identity_and_sql_outcomes() {
        let game_id = GameId::new("steam:topology-observation").expect("game id");
        let value = topology(&game_id, "topology:observation");
        let json = mapping::serialize_json(&value).expect("topology json");

        assert!(matches!(
            observe_raw(Ok(None), &game_id).expect("missing row"),
            RowObservation::Missing
        ));
        assert!(matches!(
            observe_raw(
                Ok(Some(RawProxyTopology {
                    row_id: value.id.clone(),
                    json: json.clone(),
                })),
                &game_id,
            )
            .expect("valid row"),
            RowObservation::Present(_)
        ));
        assert!(matches!(
            observe_raw(
                Ok(Some(RawProxyTopology {
                    row_id: "topology:wrong-row-id".to_owned(),
                    json,
                })),
                &game_id,
            )
            .expect("identity mismatch"),
            RowObservation::Invalid(_)
        ));
        assert!(matches!(
            observe_raw(
                Ok(Some(RawProxyTopology {
                    row_id: value.id,
                    json: "{".to_owned(),
                })),
                &game_id,
            )
            .expect("malformed json"),
            RowObservation::Invalid(_)
        ));
        assert!(observe_raw(Err(rusqlite::Error::InvalidQuery), &game_id).is_err());
    }
}
