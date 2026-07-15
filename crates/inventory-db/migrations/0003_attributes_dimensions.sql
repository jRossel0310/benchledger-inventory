-- Typed category attributes and structured dimensions (spec §5, §6).

CREATE TABLE attribute_defs (
    id             TEXT PRIMARY KEY,
    key            TEXT NOT NULL UNIQUE,
    label          TEXT NOT NULL,
    data_type      TEXT NOT NULL CHECK (data_type IN
        ('text', 'number', 'number_unit', 'boolean', 'choice', 'multi_choice', 'range', 'url')),
    unit_kind      TEXT CHECK (unit_kind IN
        ('resistance', 'capacitance', 'inductance', 'voltage', 'current', 'power',
         'frequency', 'length', 'mass', 'time', 'percent', 'charge')),
    canonical_unit TEXT,
    searchable     INTEGER NOT NULL DEFAULT 1 CHECK (searchable IN (0, 1)),
    filterable     INTEGER NOT NULL DEFAULT 1 CHECK (filterable IN (0, 1)),
    identity       INTEGER NOT NULL DEFAULT 0 CHECK (identity IN (0, 1)),
    built_in       INTEGER NOT NULL DEFAULT 0 CHECK (built_in IN (0, 1)),
    created_at     TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE TABLE category_attributes (
    category_id   TEXT NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    attribute_id  TEXT NOT NULL REFERENCES attribute_defs(id) ON DELETE CASCADE,
    display_order INTEGER NOT NULL DEFAULT 0,
    hidden        INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    PRIMARY KEY (category_id, attribute_id)
) STRICT;

CREATE TABLE attribute_choices (
    attribute_id  TEXT NOT NULL REFERENCES attribute_defs(id) ON DELETE CASCADE,
    value         TEXT NOT NULL,
    display_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (attribute_id, value)
) STRICT;

CREATE TABLE part_attribute_values (
    part_id       TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    attribute_id  TEXT NOT NULL REFERENCES attribute_defs(id),
    original_text TEXT NOT NULL,
    -- normalized numeric value (f64 of the exact form) for filtering; exact
    -- identity comparison re-parses original_text at compare time.
    value_num     REAL,
    value_num_hi  REAL,            -- upper bound for 'range' attributes
    value_text    TEXT,            -- text/choice/url; JSON array for multi_choice
    value_bool    INTEGER CHECK (value_bool IN (0, 1)),
    PRIMARY KEY (part_id, attribute_id)
) STRICT;
CREATE INDEX idx_pav_attribute_num ON part_attribute_values(attribute_id, value_num);

CREATE TABLE dimensions (
    id               TEXT PRIMARY KEY,
    part_id          TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    dim_group        TEXT NOT NULL CHECK (dim_group IN ('overall', 'body', 'mounting', 'custom')),
    name             TEXT NOT NULL,
    value_num        REAL NOT NULL,
    display_unit     TEXT NOT NULL,
    -- lengths normalize to millimeters, masses to grams
    normalized_value REAL NOT NULL,
    source           TEXT NOT NULL CHECK (source IN
        ('manufacturer', 'datasheet', 'supplier', 'measured', 'estimated')),
    notes            TEXT NOT NULL DEFAULT '',
    measured_date    TEXT,
    -- FK arrives with Phase 3's attachments table (same pattern as
    -- transactions.bom_item_id).
    attachment_id    TEXT,
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_dimensions_part ON dimensions(part_id);
