CREATE TABLE IF NOT EXISTS game_proxy_topologies (
    game_id         TEXT    NOT NULL,
    id              TEXT    NOT NULL,
    topology_json   TEXT    NOT NULL,
    created_at      INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    updated_at      INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    PRIMARY KEY (game_id, id),
    UNIQUE (game_id),
    FOREIGN KEY (game_id) REFERENCES games(id) ON DELETE CASCADE,
    CHECK (length(trim(game_id)) > 0),
    CHECK (length(trim(id)) > 0),
    CHECK (json_valid(topology_json)),
    CHECK (json_type(topology_json) = 'object'),
    CHECK (instr(topology_json, char(0)) = 0),
    CHECK (created_at >= 0),
    CHECK (updated_at >= created_at)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_game_proxy_topologies_updated_at
    ON game_proxy_topologies(updated_at DESC);

CREATE TRIGGER IF NOT EXISTS trg_game_proxy_topologies_touch_updated_at
AFTER UPDATE ON game_proxy_topologies
FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE game_proxy_topologies
       SET updated_at = max(CAST(unixepoch('subsec') * 1000 AS INTEGER), OLD.updated_at + 1)
     WHERE game_id = NEW.game_id AND id = NEW.id;
END;

CREATE TABLE IF NOT EXISTS optiscaler_install_states (
    game_id                 TEXT    PRIMARY KEY NOT NULL,
    release_id              TEXT    NOT NULL,
    manifest_revision       TEXT    NOT NULL,
    archive_sha256          TEXT,
    source                  TEXT,
    target_exe_path         TEXT    NOT NULL,
    target_dir              TEXT    NOT NULL,
    modules_json            TEXT    NOT NULL,
    release_files_json      TEXT    NOT NULL,
    runtime_bindings_json   TEXT    NOT NULL DEFAULT '[]',
    directory_receipts_json TEXT    NOT NULL DEFAULT '[]',
    proxy_topology_id       TEXT    NOT NULL,
    config_schema            INTEGER NOT NULL,
    config_base_release     TEXT    NOT NULL,
    adoption_state          TEXT    NOT NULL,
    prerequisite_binding    TEXT    NOT NULL,
    created_at              INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    updated_at              INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    configuration_baseline_json TEXT NOT NULL,
    FOREIGN KEY (game_id, proxy_topology_id)
        REFERENCES game_proxy_topologies(game_id, id)
        ON DELETE RESTRICT,
    CHECK (length(trim(game_id)) > 0),
    CHECK (length(trim(release_id)) > 0),
    CHECK (length(trim(manifest_revision)) > 0),
    CHECK (archive_sha256 IS NULL OR (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9A-Fa-f]*')),
    CHECK (source IS NULL OR length(trim(source)) > 0),
    CHECK (length(trim(target_exe_path)) > 0),
    CHECK (length(trim(target_dir)) > 0),
    CHECK (instr(target_exe_path, char(0)) = 0),
    CHECK (instr(target_dir, char(0)) = 0),
    CHECK (json_valid(modules_json)),
    CHECK (json_type(modules_json) = 'array'),
    CHECK (json_array_length(modules_json) > 0),
    CHECK (json_valid(release_files_json)),
    CHECK (json_type(release_files_json) = 'array'),
    CHECK (json_array_length(release_files_json) > 0),
    CHECK (json_valid(runtime_bindings_json)),
    CHECK (json_type(runtime_bindings_json) = 'array'),
    CHECK (json_valid(directory_receipts_json)),
    CHECK (json_type(directory_receipts_json) = 'array'),
    CHECK (length(trim(proxy_topology_id)) > 0),
    CHECK (config_schema > 0),
    CHECK (length(trim(config_base_release)) > 0),
    CHECK (adoption_state IN ('managed', 'adopted_exact', 'taken_over')),
    CHECK (prerequisite_binding IN ('none', 'luma')),
    CHECK (created_at >= 0),
    CHECK (updated_at >= created_at),
    CHECK (
        json_valid(configuration_baseline_json)
        AND json_type(configuration_baseline_json) = 'object'
        AND json_type(configuration_baseline_json, '$.format_tag') = 'text'
        AND json_extract(configuration_baseline_json, '$.format_tag') =
            'renderpilot.optiscaler.configuration-baseline'
        AND json_type(configuration_baseline_json, '$.revision') = 'integer'
        AND json_extract(configuration_baseline_json, '$.revision') = 1
        AND json_type(configuration_baseline_json, '$.kind') = 'text'
        AND json_extract(configuration_baseline_json, '$.kind') IN ('absent', 'present')
    )
) STRICT;

CREATE INDEX IF NOT EXISTS idx_optiscaler_install_states_topology
    ON optiscaler_install_states(game_id, proxy_topology_id);

CREATE TRIGGER IF NOT EXISTS trg_optiscaler_install_states_touch_updated_at
AFTER UPDATE ON optiscaler_install_states
FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE optiscaler_install_states
       SET updated_at = max(CAST(unixepoch('subsec') * 1000 AS INTEGER), OLD.updated_at + 1)
     WHERE game_id = NEW.game_id;
END;

CREATE TRIGGER IF NOT EXISTS trg_optiscaler_install_states_freeze_configuration_baseline
BEFORE UPDATE OF configuration_baseline_json ON optiscaler_install_states
FOR EACH ROW
WHEN NEW.configuration_baseline_json IS NOT OLD.configuration_baseline_json
BEGIN
    SELECT RAISE(ABORT, 'OptiScaler configuration baseline is immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_optiscaler_install_states_freeze_prerequisite_binding
BEFORE UPDATE OF prerequisite_binding ON optiscaler_install_states
FOR EACH ROW
WHEN NEW.prerequisite_binding IS NOT OLD.prerequisite_binding
BEGIN
    SELECT RAISE(ABORT, 'OptiScaler prerequisite binding is immutable');
END;
