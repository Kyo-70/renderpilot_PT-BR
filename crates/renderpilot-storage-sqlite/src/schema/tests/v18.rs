use rusqlite::{Connection, params};

use super::*;

fn seed_game(connection: &Connection, game_id: &str) {
    let install_key = format!("c:/{game_id}");
    connection
        .execute(
            "INSERT INTO games
                (id, title, launcher, platform, runtime, install_path,
                 install_key, root_authority, executable_candidates_json)
             VALUES (?1, ?2, 'Manual', 'Windows', 'NativeWindows', ?3,
                     ?4, 'legacy', '[]')",
            params![game_id, game_id, install_key, install_key],
        )
        .expect("game fixture");
}

#[test]
fn released_v18_pending_tables_upgrade_to_the_v19_contract() {
    let connection = Connection::open_in_memory().expect("released v18 catalog");
    connection
        .execute_batch(
            "CREATE TABLE games (id TEXT PRIMARY KEY NOT NULL) STRICT;
             CREATE TABLE pending_file_mutations (
                 id TEXT PRIMARY KEY NOT NULL,
                 game_id TEXT NOT NULL,
                 feature TEXT NOT NULL,
                 subject_id TEXT,
                 state TEXT NOT NULL,
                 manifest_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             ) STRICT;
             CREATE INDEX idx_pending_file_mutations_game_id
                 ON pending_file_mutations(game_id);
             CREATE TABLE pending_shared_vulkan_mutations (
                 resource_key TEXT PRIMARY KEY NOT NULL,
                 id TEXT UNIQUE NOT NULL,
                 scope TEXT NOT NULL,
                 game_id TEXT,
                 feature TEXT NOT NULL,
                 state TEXT NOT NULL,
                 manifest_json TEXT NOT NULL,
                 root_capabilities_json TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             ) STRICT;
             INSERT INTO games (id) VALUES ('game:v18');
             INSERT INTO pending_file_mutations
                 (id, game_id, feature, state, manifest_json, created_at, updated_at)
             VALUES ('mutation:v18', 'game:v18', 'legacy', 'prepared', '{}', 10, 11);
             INSERT INTO pending_shared_vulkan_mutations
                 (resource_key, id, scope, feature, state, manifest_json,
                  root_capabilities_json, created_at, updated_at)
             VALUES ('renodx_vulkan_layer', 'shared:v18', 'shared_only', 'legacy',
                     'prepared', '{}', '{}', 12, 13);
             PRAGMA user_version = 18;",
        )
        .expect("released v18 shape");

    super::super::steps::run_to_for_test(&connection, 18, 19).expect("v18 to v19");

    let pending_file: (Option<String>, Option<i64>) = connection
        .query_row(
            "SELECT aggregate_kind, aggregate_revision
               FROM pending_file_mutations
              WHERE id = 'mutation:v18'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("migrated pending file row");
    assert_eq!(pending_file, (None, None));
    let retained_ordinary: (String, String, String, String, i64, i64) = connection
        .query_row(
            "SELECT game_id, feature, state, manifest_json, created_at, updated_at
               FROM pending_file_mutations
              WHERE id = 'mutation:v18'",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("retained ordinary pending file row");
    assert_eq!(
        retained_ordinary,
        (
            "game:v18".to_owned(),
            "legacy".to_owned(),
            "prepared".to_owned(),
            "{}".to_owned(),
            10,
            11,
        )
    );
    let pending_file_index_table: String = connection
        .query_row(
            "SELECT tbl_name
               FROM sqlite_master
              WHERE type = 'index' AND name = 'idx_pending_file_mutations_game_id'",
            [],
            |row| row.get(0),
        )
        .expect("required v19 pending-file index");
    assert_eq!(pending_file_index_table, "pending_file_mutations");
    let pending_shared: (Option<String>, Option<i64>, Option<Vec<u8>>) = connection
        .query_row(
            "SELECT aggregate_kind, aggregate_revision, aggregate_program
               FROM pending_shared_vulkan_mutations
              WHERE id = 'shared:v18'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("migrated shared row");
    assert_eq!(pending_shared, (None, None, None));

    let mut fresh = open_test_connection();
    apply(&mut fresh).expect("fresh v19 baseline");
    for table in [
        "optiscaler_install_states",
        "pending_file_mutations",
        "pending_shared_vulkan_mutations",
        "peer_aggregate_reservations",
    ] {
        assert_eq!(
            normalized_schema_sql(&connection, table),
            normalized_schema_sql(&fresh, table),
            "fresh and v18-upgraded v19 must share exact {table} DDL"
        );
    }
    for trigger in [
        "trg_optiscaler_install_states_freeze_prerequisite_binding",
        "trg_pending_file_mutations_freeze_aggregate_binding",
        "trg_pending_file_mutations_restrict_state_transition",
        "trg_peer_aggregate_reservations_restrict_state_transition",
    ] {
        assert_eq!(
            normalized_trigger_sql(&connection, trigger),
            normalized_trigger_sql(&fresh, trigger),
            "fresh and v18-upgraded v19 must share exact {trigger} DDL"
        );
    }
}

#[test]
fn final_v19_enforces_optiscaler_and_shared_fences() {
    let mut connection = open_test_connection();
    apply(&mut connection).expect("v19 baseline");
    seed_game(&connection, "manual:aggregate");

    connection
        .execute(
            "INSERT INTO pending_file_mutations
                (id, game_id, feature, state, manifest_json)
             VALUES ('mutation:ordinary', 'manual:aggregate', 'ordinary', 'preparing', '{}')",
            [],
        )
        .expect("ordinary pending row");
    assert!(
        connection
            .execute(
                "INSERT INTO pending_file_mutations
                    (id, game_id, feature, state, manifest_json, aggregate_kind,
                     aggregate_revision)
                 VALUES ('mutation:file-peer', 'manual:aggregate', 'file', 'preparing', '{}',
                         'file_peer', 4)",
                [],
            )
            .is_err(),
        "FilePeer aggregate is not part of the v19 contract"
    );
    assert!(
        connection
            .execute(
                "INSERT INTO pending_file_mutations
                    (id, game_id, feature, state, manifest_json)
                 VALUES ('mutation:materializing', 'manual:aggregate', 'ordinary',
                         'materializing', '{}')",
                [],
            )
            .is_err(),
        "Materializing state is not part of the v19 contract"
    );
    connection
        .execute(
            "INSERT INTO pending_file_mutations
                (id, game_id, feature, state, manifest_json, aggregate_kind,
                 aggregate_revision)
             VALUES ('mutation:journal', 'manual:aggregate', 'journal', 'preparing', '{}',
                     'optiscaler_journal', 4)",
            [],
        )
        .expect("OptiScaler aggregate fence");
    connection
        .execute(
            "UPDATE pending_file_mutations SET state = 'prepared'
              WHERE id = 'mutation:journal'",
            [],
        )
        .expect("Preparing to Prepared");
    assert!(
        connection
            .execute(
                "UPDATE pending_file_mutations
                    SET aggregate_revision = 5 WHERE id = 'mutation:journal'",
                [],
            )
            .is_err(),
        "aggregate binding remains immutable"
    );

    connection
        .execute(
            "INSERT INTO pending_shared_vulkan_mutations
                (resource_key, id, scope, game_id, feature, state, manifest_json,
                 root_capabilities_json, aggregate_kind, aggregate_revision, aggregate_program)
             VALUES ('renodx_vulkan_layer', 'mutation:shared', 'game_shared',
                     'manual:aggregate', 'shared', 'prepared', '{}', '{}',
                     'shared_peer', 6, ?1)",
            [vec![4_u8, 5]],
        )
        .expect("shared Vulkan aggregate program remains supported");
    assert!(
        connection
            .execute(
                "INSERT INTO peer_aggregate_reservations
                    (game_id, operation_id, aggregate_kind, pending_binding, state,
                     expected_revision)
                 VALUES ('manual:aggregate', 'operation:file-peer', 'file_peer',
                         'file', 'preparing', 4)",
                [],
            )
            .is_err(),
        "reservation cannot name FilePeer"
    );
    connection
        .execute(
            "INSERT INTO peer_aggregate_reservations
                (game_id, operation_id, aggregate_kind, pending_binding, state,
                 expected_revision)
             VALUES ('manual:aggregate', 'operation:journal', 'optiscaler_journal',
                     'file', 'preparing', 4)",
            [],
        )
        .expect("OptiScaler reservation");
    assert!(
        connection
            .execute(
                "UPDATE peer_aggregate_reservations SET state = 'committed'
                  WHERE operation_id = 'operation:journal'",
                [],
            )
            .is_err(),
        "reservation state transitions remain linear"
    );
}

fn normalized_schema_sql(connection: &Connection, table: &str) -> String {
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )
        .expect("table DDL");
    sql.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .replacen("createtableifnotexists", "createtable", 1)
}

fn normalized_trigger_sql(connection: &Connection, trigger: &str) -> String {
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
            [trigger],
            |row| row.get(0),
        )
        .expect("trigger DDL");
    sql.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .replacen("createtriggerifnotexists", "createtrigger", 1)
}
