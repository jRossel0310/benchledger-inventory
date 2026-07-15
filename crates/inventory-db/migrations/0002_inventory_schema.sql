-- Core inventory schema: categories (minimal — attribute system arrives in
-- migration 0003 / Phase 2b), parts, variants, supplier listings, stock
-- aggregates, and the append-only transaction ledger.

CREATE TABLE categories (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    group_name TEXT NOT NULL,
    built_in   INTEGER NOT NULL DEFAULT 0 CHECK (built_in IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

INSERT INTO categories (id, name, group_name, built_in)
VALUES ('00000000000000000000000000', 'Miscellaneous', 'Mechanical and miscellaneous', 1);

-- Stub: Phase 4 extends with status/description/build_quantity/etc. Exists now
-- so ledger rows can carry a real FK from day one.
CREATE TABLE projects (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE TABLE parts (
    id                        TEXT PRIMARY KEY,
    display_name              TEXT NOT NULL,
    category_id               TEXT NOT NULL REFERENCES categories(id),
    description               TEXT NOT NULL DEFAULT '',
    bin_label                 TEXT,
    usage_behavior            TEXT NOT NULL DEFAULT 'usually_consumed'
        CHECK (usage_behavior IN ('usually_consumed', 'usually_checked_out', 'ask')),
    quantity_unit             TEXT NOT NULL DEFAULT 'each'
        CHECK (quantity_unit IN ('each', 'm', 'ft')),
    low_stock_threshold_milli INTEGER CHECK (low_stock_threshold_milli >= 0),
    preferred_reorder_milli   INTEGER CHECK (preferred_reorder_milli >= 0),
    public_notes              TEXT NOT NULL DEFAULT '',
    private_notes             TEXT NOT NULL DEFAULT '',
    metadata_complete         INTEGER NOT NULL DEFAULT 0 CHECK (metadata_complete IN (0, 1)),
    archived                  INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at                TEXT NOT NULL DEFAULT (datetime('now')),
    modified_at               TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_parts_category ON parts(category_id);
CREATE INDEX idx_parts_bin ON parts(bin_label);
CREATE INDEX idx_parts_archived ON parts(archived);

CREATE TABLE part_tags (
    part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (part_id, tag)
) STRICT;

CREATE TABLE manufacturer_variants (
    id            TEXT PRIMARY KEY,
    part_id       TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
    manufacturer  TEXT NOT NULL,
    mpn           TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    package       TEXT,
    datasheet_url TEXT,
    product_url   TEXT,
    lifecycle     TEXT,
    is_preferred  INTEGER NOT NULL DEFAULT 0 CHECK (is_preferred IN (0, 1)),
    notes         TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (part_id, manufacturer, mpn)
) STRICT;
CREATE UNIQUE INDEX idx_variants_one_preferred
    ON manufacturer_variants(part_id) WHERE is_preferred = 1;
CREATE INDEX idx_variants_mpn ON manufacturer_variants(mpn);

CREATE TABLE supplier_listings (
    id                     TEXT PRIMARY KEY,
    variant_id             TEXT NOT NULL REFERENCES manufacturer_variants(id) ON DELETE CASCADE,
    supplier               TEXT NOT NULL,
    supplier_sku           TEXT NOT NULL,
    product_url            TEXT,
    packaging              TEXT,
    typical_order_milli    INTEGER CHECK (typical_order_milli >= 0),
    last_unit_price_micros INTEGER CHECK (last_unit_price_micros >= 0),
    currency               TEXT,
    last_purchase_date     TEXT,
    created_at             TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (variant_id, supplier, supplier_sku)
) STRICT;
CREATE INDEX idx_listings_sku ON supplier_listings(supplier_sku);

CREATE TABLE part_stock (
    part_id                 TEXT PRIMARY KEY REFERENCES parts(id),
    available_milli         INTEGER NOT NULL DEFAULT 0 CHECK (available_milli >= 0),
    reserved_milli          INTEGER NOT NULL DEFAULT 0 CHECK (reserved_milli >= 0),
    checked_out_milli       INTEGER NOT NULL DEFAULT 0 CHECK (checked_out_milli >= 0),
    lifetime_received_milli INTEGER NOT NULL DEFAULT 0 CHECK (lifetime_received_milli >= 0),
    lifetime_consumed_milli INTEGER NOT NULL DEFAULT 0 CHECK (lifetime_consumed_milli >= 0)
) STRICT;

CREATE TABLE transaction_groups (
    id                TEXT PRIMARY KEY,
    kind              TEXT NOT NULL,
    note              TEXT NOT NULL DEFAULT '',
    reversed_group_id TEXT REFERENCES transaction_groups(id),
    created_at        TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE UNIQUE INDEX idx_groups_reversal
    ON transaction_groups(reversed_group_id) WHERE reversed_group_id IS NOT NULL;

CREATE TABLE transactions (
    id              TEXT PRIMARY KEY,
    part_id         TEXT NOT NULL REFERENCES parts(id),
    group_id        TEXT REFERENCES transaction_groups(id),
    txn_type        TEXT NOT NULL CHECK (txn_type IN (
        'receive', 'reserve', 'release_reservation', 'check_out', 'return',
        'consume_available', 'consume_reserved', 'consume_checked_out',
        'adjust_up', 'adjust_down', 'transfer_reservation', 'reverse')),
    quantity_milli  INTEGER NOT NULL CHECK (quantity_milli > 0),
    from_state      TEXT CHECK (from_state IN ('available', 'reserved', 'checked_out')),
    to_state        TEXT CHECK (to_state IN ('available', 'reserved', 'checked_out')),
    project_id      TEXT REFERENCES projects(id),
    to_project_id   TEXT REFERENCES projects(id),
    -- FK for bom_item_id arrives with the Phase 4 BOM tables; for import_id
    -- with the Phase 5 import tables. Domain layer enforces meanwhile.
    bom_item_id     TEXT,
    import_id       TEXT,
    note            TEXT NOT NULL DEFAULT '',
    reversed_txn_id TEXT REFERENCES transactions(id),
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_txn_part ON transactions(part_id, created_at);
CREATE INDEX idx_txn_group ON transactions(group_id);
CREATE UNIQUE INDEX idx_txn_reversal
    ON transactions(reversed_txn_id) WHERE reversed_txn_id IS NOT NULL;
