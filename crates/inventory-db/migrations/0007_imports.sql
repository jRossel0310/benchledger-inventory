-- Phase 5a Task 2: import schema + matching-memory + checkout tables
-- (spec §10 import pipeline, §4.2 matching memory).
--
-- `imports` is the persisted record of one parsed supplier order file
-- (PDF/CSV/XLSX); `import_files` points at the original bytes preserved
-- verbatim in the existing content-addressed `attachments` store (Phase 3),
-- so re-parsing is always possible. `import_lines` keeps one row per parsed
-- line item, including its full `raw_json` so review/debugging can see
-- exactly what the parser saw even for fields not (yet) modeled here.
-- Nothing in this migration touches `part_stock`/`transactions` — 5a only
-- captures what was parsed; matching/enrichment/commit is 5b/5c.
--
-- Money is stored as *_micros (i64, x1_000_000) rather than a float, since
-- DigiKey unit prices carry up to 5 decimals (e.g. 1.82000) that a float
-- would round; quantities are *_milli (x1000), matching part_stock/
-- transactions elsewhere in the schema.
--
-- `price_history` and the `equivalence_families` / `equivalence_family_members`
-- pair complete the §4.2 matching-memory trio started in Phase 2c
-- (`part_aliases`, `equivalence_decisions` — NOT recreated here). They are
-- populated at commit time (5b) but the schema is added now so it's
-- complete. `project_checkouts` is the §4.2 stub table for project/part
-- association bookkeeping; wiring it into build/checkout flows is later.

CREATE TABLE imports (
    id              TEXT PRIMARY KEY,
    supplier        TEXT NOT NULL,
    order_number    TEXT,
    invoice_number  TEXT,
    shipment_number TEXT,
    order_date      TEXT,
    currency        TEXT NOT NULL DEFAULT 'USD',
    subtotal_micros INTEGER,
    shipping_micros INTEGER,
    tax_micros      INTEGER,
    tariff_micros   INTEGER,
    total_micros    INTEGER,
    source_format   TEXT NOT NULL CHECK (source_format IN ('pdf', 'csv', 'xlsx')),
    status          TEXT NOT NULL DEFAULT 'parsed'
        CHECK (status IN ('parsed', 'committed', 'reversed')),
    web_order_id    TEXT,
    notes           TEXT DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_imports_order_number ON imports(order_number);
CREATE INDEX idx_imports_invoice_number ON imports(invoice_number);

-- Points at the attachments store (migration 0005): the original file bytes
-- for an import are content-hashed and deduplicated there, same as any
-- other attachment. One import may (in principle) have more than one source
-- file, hence a separate table rather than a column on `imports`.
CREATE TABLE import_files (
    id                TEXT PRIMARY KEY,
    import_id         TEXT NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
    attachment_hash   TEXT NOT NULL,
    original_filename TEXT NOT NULL,
    byte_size         INTEGER NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_import_files_import ON import_files(import_id);
CREATE INDEX idx_import_files_hash ON import_files(attachment_hash);

CREATE TABLE import_lines (
    id                    TEXT PRIMARY KEY,
    import_id             TEXT NOT NULL REFERENCES imports(id) ON DELETE CASCADE,
    line_number           INTEGER,
    supplier_sku          TEXT,
    mpn                   TEXT,
    manufacturer          TEXT,
    description           TEXT,
    ordered_milli         INTEGER,
    shipped_milli         INTEGER,
    backordered_milli     INTEGER,
    unit_price_micros     INTEGER,
    extended_price_micros INTEGER,
    packaging             TEXT,
    customer_reference    TEXT,
    raw_json              TEXT NOT NULL,
    line_kind             TEXT NOT NULL DEFAULT 'part'
        CHECK (line_kind IN ('part', 'fee', 'tariff', 'no_charge', 'unknown')),
    parse_confidence      REAL NOT NULL DEFAULT 1.0,
    created_at            TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_import_lines_import ON import_lines(import_id);

-- One row per observed purchase price point. Populated at commit time (5b);
-- `import_id` uses ON DELETE SET NULL (rather than CASCADE) because a price
-- observation remains historically true even if the import that produced it
-- is later reversed/deleted.
CREATE TABLE price_history (
    id                 TEXT PRIMARY KEY,
    part_id            TEXT REFERENCES parts(id),
    supplier           TEXT,
    supplier_sku       TEXT,
    unit_price_micros  INTEGER NOT NULL,
    currency           TEXT NOT NULL,
    quantity_milli     INTEGER,
    import_id          TEXT REFERENCES imports(id) ON DELETE SET NULL,
    purchased_at       TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;
CREATE INDEX idx_price_history_part ON price_history(part_id);

-- Groups of parts a human has judged interchangeable-enough to remember
-- (distinct from the pairwise `equivalence_decisions` memory of Phase 2c).
CREATE TABLE equivalence_families (
    id         TEXT PRIMARY KEY,
    name       TEXT,
    note       TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
) STRICT;

CREATE TABLE equivalence_family_members (
    family_id TEXT NOT NULL REFERENCES equivalence_families(id) ON DELETE CASCADE,
    part_id   TEXT NOT NULL REFERENCES parts(id),
    PRIMARY KEY (family_id, part_id)
) STRICT;

-- §4.2 stub: association bookkeeping for parts checked out against a
-- project outside the normal build-consumption flow. Wiring this into
-- build/checkout commands is deferred past 5a.
CREATE TABLE project_checkouts (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    part_id        TEXT NOT NULL REFERENCES parts(id),
    quantity_milli INTEGER NOT NULL CHECK (quantity_milli > 0),
    checked_out_at TEXT NOT NULL DEFAULT (datetime('now')),
    note           TEXT DEFAULT ''
) STRICT;
