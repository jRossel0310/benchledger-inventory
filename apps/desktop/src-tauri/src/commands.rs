//! Typed Tauri command surface over the Database API.
//!
//! Every command is a thin wrapper: lock `AppState.db`, call the
//! corresponding `Database` method, and map any error into `CommandError`
//! (never `Debug` text or a raw `DbError` crossing the IPC boundary). All
//! business logic lives in `inventory-db`; nothing here does more than
//! lock + call + map.
//!
//! Each command has two parts: a `..._impl(state: &AppState, ...)` function
//! holding the actual logic (directly testable, same pattern as
//! `app::status_of` before it moved here), and a thin `#[tauri::command]`
//! wrapper taking `State<'_, AppState>` that `tauri_specta::Builder`
//! collects for both the runtime `invoke_handler` and the generated
//! TypeScript bindings.

use std::sync::MutexGuard;

use tauri::{AppHandle, State};

use inventory_core::ids::{CategoryId, GroupId, PartId, ProjectId, TransactionId, VariantId};
use inventory_core::ledger::LedgerOp;
use inventory_db::bins::BinSummary;
use inventory_db::categories::{AttributeDefRow, CategoryRecord};
use inventory_db::dashboard::{DashboardSummary, RecentTxn};
use inventory_db::dimensions::{DimensionDraft, DimensionRecord};
use inventory_db::ledger::{GroupRecord, ProjectRef, TransactionRecord};
use inventory_db::matching::{MatchCandidate, MatchResult};
use inventory_db::parts::{
    ListingDraft, ListingRecord, PartDraft, PartRecord, PartStockRow, VariantDraft, VariantRecord,
};
use inventory_db::search::SearchHit;
use inventory_db::validate::ValidationReport;
use inventory_db::{Database, DbError};

use crate::app::{AppState, AppStatus};

/// Error shape crossing the IPC boundary: `code` is the snake_case
/// `DbError` variant name (stable, matchable by the frontend); `message` is
/// the `Display` text (never `Debug` — `Debug` text is for logs, not UI).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl From<DbError> for CommandError {
    fn from(e: DbError) -> Self {
        let code = match &e {
            DbError::NewerSchema { .. } => "newer_schema",
            DbError::Migration { .. } => "migration",
            DbError::Sqlite(_) => "sqlite",
            DbError::Io(_) => "io",
            DbError::PartNotFound => "part_not_found",
            DbError::Corrupt(_) => "corrupt",
            DbError::Domain(_) => "domain",
            DbError::InsufficientStock(_) => "insufficient_stock",
            DbError::PartArchived => "part_archived",
            DbError::Ledger(_) => "ledger",
            DbError::EmptyGroup => "empty_group",
            DbError::TransactionNotFound => "transaction_not_found",
            DbError::AlreadyReversed => "already_reversed",
            DbError::CannotReverseReversal => "cannot_reverse_reversal",
            DbError::GroupNotFound => "group_not_found",
            DbError::TransactionInGroup => "transaction_in_group",
            DbError::AttributeNotFound(_) => "attribute_not_found",
            DbError::InvalidAttributeValue { .. } => "invalid_attribute_value",
            DbError::VariantNotFound => "variant_not_found",
            DbError::ProjectNotFound => "project_not_found",
            DbError::UnitChangeBlocked => "unit_change_blocked",
            DbError::InvalidDimension(_) => "invalid_dimension",
            DbError::InvalidBinLabel(_) => "invalid_bin_label",
            DbError::DimensionNotFound => "dimension_not_found",
            DbError::CategoryNameTaken => "category_name_taken",
            DbError::AttributeKeyTaken => "attribute_key_taken",
            DbError::CategoryNotFound => "category_not_found",
            DbError::UnknownSearchKey(_) => "unknown_search_key",
            DbError::UnsupportedSearchKey(_) => "unsupported_search_key",
            DbError::AliasTaken => "alias_taken",
        };
        CommandError {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}

/// Lock `state.db`, mapping mutex poisoning to a typed `internal` error
/// instead of panicking. The returned guard derefs (mutably, via
/// `DerefMut`) to `Database`, so callers can invoke either `&self` or
/// `&mut self` methods on it.
fn lock(state: &AppState) -> Result<MutexGuard<'_, Database>, CommandError> {
    state.db.lock().map_err(|_| CommandError {
        code: "internal".to_string(),
        message: "database lock poisoned; restart the app".to_string(),
    })
}

// ---------------------------------------------------------------------
// App status
// ---------------------------------------------------------------------

pub fn status_of(state: &AppState, app_version: &str) -> Result<AppStatus, CommandError> {
    let db = lock(state)?;
    Ok(AppStatus {
        app_version: app_version.to_string(),
        schema_version: db.schema_version()?,
        data_dir: state.layout.root.display().to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn app_status(state: State<'_, AppState>, app: AppHandle) -> Result<AppStatus, CommandError> {
    let version = app.package_info().version.to_string();
    status_of(&state, &version)
}

// ---------------------------------------------------------------------
// Parts, variants, listings, stock
// ---------------------------------------------------------------------

pub fn list_parts_impl(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<PartRecord>, CommandError> {
    Ok(lock(state)?.list_parts(include_archived)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_parts(
    state: State<'_, AppState>,
    include_archived: bool,
) -> Result<Vec<PartRecord>, CommandError> {
    list_parts_impl(&state, include_archived)
}

pub fn get_part_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Option<PartRecord>, CommandError> {
    Ok(lock(state)?.get_part(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_part(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Option<PartRecord>, CommandError> {
    get_part_impl(&state, part_id)
}

pub fn create_part_impl(state: &AppState, draft: PartDraft) -> Result<PartRecord, CommandError> {
    Ok(lock(state)?.create_part(&draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn create_part(
    state: State<'_, AppState>,
    draft: PartDraft,
) -> Result<PartRecord, CommandError> {
    create_part_impl(&state, draft)
}

pub fn update_part_impl(state: &AppState, record: PartRecord) -> Result<(), CommandError> {
    Ok(lock(state)?.update_part(&record)?)
}

#[tauri::command]
#[specta::specta]
pub fn update_part(state: State<'_, AppState>, record: PartRecord) -> Result<(), CommandError> {
    update_part_impl(&state, record)
}

pub fn set_part_archived_impl(
    state: &AppState,
    part_id: PartId,
    archived: bool,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_part_archived(&part_id, archived)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_part_archived(
    state: State<'_, AppState>,
    part_id: PartId,
    archived: bool,
) -> Result<(), CommandError> {
    set_part_archived_impl(&state, part_id, archived)
}

pub fn get_stock_impl(state: &AppState, part_id: PartId) -> Result<PartStockRow, CommandError> {
    Ok(lock(state)?.get_stock(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_stock(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<PartStockRow, CommandError> {
    get_stock_impl(&state, part_id)
}

pub fn add_variant_impl(
    state: &AppState,
    part_id: PartId,
    draft: VariantDraft,
) -> Result<VariantRecord, CommandError> {
    Ok(lock(state)?.add_variant(&part_id, &draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn add_variant(
    state: State<'_, AppState>,
    part_id: PartId,
    draft: VariantDraft,
) -> Result<VariantRecord, CommandError> {
    add_variant_impl(&state, part_id, draft)
}

pub fn set_preferred_variant_impl(
    state: &AppState,
    part_id: PartId,
    variant_id: VariantId,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_preferred_variant(&part_id, &variant_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_preferred_variant(
    state: State<'_, AppState>,
    part_id: PartId,
    variant_id: VariantId,
) -> Result<(), CommandError> {
    set_preferred_variant_impl(&state, part_id, variant_id)
}

pub fn add_supplier_listing_impl(
    state: &AppState,
    variant_id: VariantId,
    draft: ListingDraft,
) -> Result<ListingRecord, CommandError> {
    Ok(lock(state)?.add_supplier_listing(&variant_id, &draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn add_supplier_listing(
    state: State<'_, AppState>,
    variant_id: VariantId,
    draft: ListingDraft,
) -> Result<ListingRecord, CommandError> {
    add_supplier_listing_impl(&state, variant_id, draft)
}

pub fn list_variants_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<VariantRecord>, CommandError> {
    Ok(lock(state)?.list_variants(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_variants(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<VariantRecord>, CommandError> {
    list_variants_impl(&state, part_id)
}

pub fn list_supplier_listings_impl(
    state: &AppState,
    variant_id: VariantId,
) -> Result<Vec<ListingRecord>, CommandError> {
    Ok(lock(state)?.list_supplier_listings(&variant_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_supplier_listings(
    state: State<'_, AppState>,
    variant_id: VariantId,
) -> Result<Vec<ListingRecord>, CommandError> {
    list_supplier_listings_impl(&state, variant_id)
}

pub fn get_tags_impl(state: &AppState, part_id: PartId) -> Result<Vec<String>, CommandError> {
    Ok(lock(state)?.get_tags(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_tags(state: State<'_, AppState>, part_id: PartId) -> Result<Vec<String>, CommandError> {
    get_tags_impl(&state, part_id)
}

// ---------------------------------------------------------------------
// Ledger: single ops, groups, reversals, history
// ---------------------------------------------------------------------

pub fn apply_ledger_op_impl(
    state: &AppState,
    op: LedgerOp,
) -> Result<TransactionRecord, CommandError> {
    Ok(lock(state)?.apply(&op)?)
}

#[tauri::command]
#[specta::specta]
pub fn apply_ledger_op(
    state: State<'_, AppState>,
    op: LedgerOp,
) -> Result<TransactionRecord, CommandError> {
    apply_ledger_op_impl(&state, op)
}

pub fn apply_group_impl(
    state: &AppState,
    kind: String,
    note: String,
    ops: Vec<LedgerOp>,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.apply_group(&kind, &note, &ops)?)
}

#[tauri::command]
#[specta::specta]
pub fn apply_group(
    state: State<'_, AppState>,
    kind: String,
    note: String,
    ops: Vec<LedgerOp>,
) -> Result<GroupRecord, CommandError> {
    apply_group_impl(&state, kind, note, ops)
}

pub fn reverse_transaction_impl(
    state: &AppState,
    txn_id: TransactionId,
    note: String,
) -> Result<TransactionRecord, CommandError> {
    Ok(lock(state)?.reverse_transaction(&txn_id, &note)?)
}

#[tauri::command]
#[specta::specta]
pub fn reverse_transaction(
    state: State<'_, AppState>,
    txn_id: TransactionId,
    note: String,
) -> Result<TransactionRecord, CommandError> {
    reverse_transaction_impl(&state, txn_id, note)
}

pub fn reverse_group_impl(
    state: &AppState,
    group_id: GroupId,
    note: String,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.reverse_group(&group_id, &note)?)
}

#[tauri::command]
#[specta::specta]
pub fn reverse_group(
    state: State<'_, AppState>,
    group_id: GroupId,
    note: String,
) -> Result<GroupRecord, CommandError> {
    reverse_group_impl(&state, group_id, note)
}

pub fn list_transactions_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<TransactionRecord>, CommandError> {
    Ok(lock(state)?.list_transactions(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_transactions(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<TransactionRecord>, CommandError> {
    list_transactions_impl(&state, part_id)
}

pub fn get_group_impl(
    state: &AppState,
    group_id: GroupId,
) -> Result<Option<GroupRecord>, CommandError> {
    Ok(lock(state)?.get_group(&group_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_group(
    state: State<'_, AppState>,
    group_id: GroupId,
) -> Result<Option<GroupRecord>, CommandError> {
    get_group_impl(&state, group_id)
}

// ---------------------------------------------------------------------
// Projects (Phase 4 stub — id+name only; see `inventory_db::ledger::ProjectRef`)
// ---------------------------------------------------------------------

pub fn list_projects_impl(state: &AppState) -> Result<Vec<ProjectRef>, CommandError> {
    Ok(lock(state)?.list_projects()?)
}

#[tauri::command]
#[specta::specta]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectRef>, CommandError> {
    list_projects_impl(&state)
}

pub fn create_project_impl(state: &AppState, name: String) -> Result<ProjectId, CommandError> {
    Ok(lock(state)?.create_project(&name)?)
}

#[tauri::command]
#[specta::specta]
pub fn create_project(state: State<'_, AppState>, name: String) -> Result<ProjectId, CommandError> {
    create_project_impl(&state, name)
}

// ---------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------

pub fn set_attribute_impl(
    state: &AppState,
    part_id: PartId,
    key: String,
    raw: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_attribute(&part_id, &key, &raw)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_attribute(
    state: State<'_, AppState>,
    part_id: PartId,
    key: String,
    raw: String,
) -> Result<(), CommandError> {
    set_attribute_impl(&state, part_id, key, raw)
}

pub fn get_attributes_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<(String, String, Option<f64>)>, CommandError> {
    Ok(lock(state)?.get_attributes(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_attributes(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<(String, String, Option<f64>)>, CommandError> {
    get_attributes_impl(&state, part_id)
}

pub fn clear_attribute_impl(
    state: &AppState,
    part_id: PartId,
    key: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.clear_attribute(&part_id, &key)?)
}

#[tauri::command]
#[specta::specta]
pub fn clear_attribute(
    state: State<'_, AppState>,
    part_id: PartId,
    key: String,
) -> Result<(), CommandError> {
    clear_attribute_impl(&state, part_id, key)
}

// ---------------------------------------------------------------------
// Dimensions
// ---------------------------------------------------------------------

pub fn add_dimension_impl(
    state: &AppState,
    part_id: PartId,
    draft: DimensionDraft,
) -> Result<DimensionRecord, CommandError> {
    Ok(lock(state)?.add_dimension(&part_id, &draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn add_dimension(
    state: State<'_, AppState>,
    part_id: PartId,
    draft: DimensionDraft,
) -> Result<DimensionRecord, CommandError> {
    add_dimension_impl(&state, part_id, draft)
}

pub fn list_dimensions_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<DimensionRecord>, CommandError> {
    Ok(lock(state)?.list_dimensions(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_dimensions(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<DimensionRecord>, CommandError> {
    list_dimensions_impl(&state, part_id)
}

pub fn remove_dimension_impl(state: &AppState, id: String) -> Result<(), CommandError> {
    Ok(lock(state)?.remove_dimension(&id)?)
}

#[tauri::command]
#[specta::specta]
pub fn remove_dimension(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    remove_dimension_impl(&state, id)
}

// ---------------------------------------------------------------------
// Categories and custom attributes
// ---------------------------------------------------------------------

pub fn list_categories_impl(state: &AppState) -> Result<Vec<CategoryRecord>, CommandError> {
    Ok(lock(state)?.list_categories()?)
}

#[tauri::command]
#[specta::specta]
pub fn list_categories(state: State<'_, AppState>) -> Result<Vec<CategoryRecord>, CommandError> {
    list_categories_impl(&state)
}

pub fn category_attributes_impl(
    state: &AppState,
    category_id: CategoryId,
) -> Result<Vec<(String, String, i64, bool)>, CommandError> {
    Ok(lock(state)?.category_attributes(&category_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn category_attributes(
    state: State<'_, AppState>,
    category_id: CategoryId,
) -> Result<Vec<(String, String, i64, bool)>, CommandError> {
    category_attributes_impl(&state, category_id)
}

pub fn category_attribute_defs_impl(
    state: &AppState,
    category_id: CategoryId,
) -> Result<Vec<AttributeDefRow>, CommandError> {
    Ok(lock(state)?.category_attribute_defs(&category_id)?)
}

/// The richer per-attribute shape (`AttributeDefRow`: data_type, unit_kind,
/// identity, choices, alongside `category_attributes`' key/label/
/// display_order/hidden) the part create/edit form (Phase 3 Task 6) needs to
/// render one typed field widget per attribute.
#[tauri::command]
#[specta::specta]
pub fn category_attribute_defs(
    state: State<'_, AppState>,
    category_id: CategoryId,
) -> Result<Vec<AttributeDefRow>, CommandError> {
    category_attribute_defs_impl(&state, category_id)
}

pub fn preview_unit_value_impl(unit_kind: String, raw: String) -> Result<String, CommandError> {
    Ok(inventory_db::attributes::preview_unit_value(
        &unit_kind, &raw,
    )?)
}

/// Stateless: formats `raw` under `unit_kind`'s parsing rules into its
/// canonical display form (`"10k"` -> `"10 kΩ"`) without touching the
/// database or a part — the part form's live `number_unit`/`range` preview
/// as the user types, using the exact same parser `set_attribute` normalizes
/// through on save.
#[tauri::command]
#[specta::specta]
pub fn preview_unit_value(unit_kind: String, raw: String) -> Result<String, CommandError> {
    preview_unit_value_impl(unit_kind, raw)
}

pub fn create_category_impl(
    state: &AppState,
    name: String,
    group: String,
) -> Result<CategoryRecord, CommandError> {
    Ok(lock(state)?.create_category(&name, &group)?)
}

#[tauri::command]
#[specta::specta]
pub fn create_category(
    state: State<'_, AppState>,
    name: String,
    group: String,
) -> Result<CategoryRecord, CommandError> {
    create_category_impl(&state, name, group)
}

pub fn duplicate_category_impl(
    state: &AppState,
    source: CategoryId,
    new_name: String,
) -> Result<CategoryRecord, CommandError> {
    Ok(lock(state)?.duplicate_category(&source, &new_name)?)
}

#[tauri::command]
#[specta::specta]
pub fn duplicate_category(
    state: State<'_, AppState>,
    source: CategoryId,
    new_name: String,
) -> Result<CategoryRecord, CommandError> {
    duplicate_category_impl(&state, source, new_name)
}

#[allow(clippy::too_many_arguments)]
pub fn create_custom_attribute_impl(
    state: &AppState,
    key: String,
    label: String,
    data_type: String,
    unit_kind: Option<String>,
    identity: bool,
) -> Result<String, CommandError> {
    Ok(lock(state)?.create_custom_attribute(
        &key,
        &label,
        &data_type,
        unit_kind.as_deref(),
        identity,
    )?)
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub fn create_custom_attribute(
    state: State<'_, AppState>,
    key: String,
    label: String,
    data_type: String,
    unit_kind: Option<String>,
    identity: bool,
) -> Result<String, CommandError> {
    create_custom_attribute_impl(&state, key, label, data_type, unit_kind, identity)
}

pub fn attach_attribute_impl(
    state: &AppState,
    category: CategoryId,
    attribute_key: String,
    display_order: i64,
) -> Result<(), CommandError> {
    Ok(lock(state)?.attach_attribute(&category, &attribute_key, display_order)?)
}

#[tauri::command]
#[specta::specta]
pub fn attach_attribute(
    state: State<'_, AppState>,
    category: CategoryId,
    attribute_key: String,
    display_order: i64,
) -> Result<(), CommandError> {
    attach_attribute_impl(&state, category, attribute_key, display_order)
}

pub fn set_attribute_hidden_impl(
    state: &AppState,
    category: CategoryId,
    attribute_key: String,
    hidden: bool,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_attribute_hidden(&category, &attribute_key, hidden)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_attribute_hidden(
    state: State<'_, AppState>,
    category: CategoryId,
    attribute_key: String,
    hidden: bool,
) -> Result<(), CommandError> {
    set_attribute_hidden_impl(&state, category, attribute_key, hidden)
}

pub fn reorder_attribute_impl(
    state: &AppState,
    category: CategoryId,
    attribute_key: String,
    display_order: i64,
) -> Result<(), CommandError> {
    Ok(lock(state)?.reorder_attribute(&category, &attribute_key, display_order)?)
}

#[tauri::command]
#[specta::specta]
pub fn reorder_attribute(
    state: State<'_, AppState>,
    category: CategoryId,
    attribute_key: String,
    display_order: i64,
) -> Result<(), CommandError> {
    reorder_attribute_impl(&state, category, attribute_key, display_order)
}

// ---------------------------------------------------------------------
// Search and duplicate matching
// ---------------------------------------------------------------------

pub fn search_impl(state: &AppState, query: String) -> Result<Vec<SearchHit>, CommandError> {
    Ok(lock(state)?.search(&query)?)
}

#[tauri::command]
#[specta::specta]
pub fn search(state: State<'_, AppState>, query: String) -> Result<Vec<SearchHit>, CommandError> {
    search_impl(&state, query)
}

pub fn find_matches_impl(
    state: &AppState,
    candidate: MatchCandidate,
) -> Result<Vec<MatchResult>, CommandError> {
    Ok(lock(state)?.find_matches(&candidate)?)
}

#[tauri::command]
#[specta::specta]
pub fn find_matches(
    state: State<'_, AppState>,
    candidate: MatchCandidate,
) -> Result<Vec<MatchResult>, CommandError> {
    find_matches_impl(&state, candidate)
}

pub fn suggest_duplicates_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<MatchResult>, CommandError> {
    Ok(lock(state)?.suggest_duplicates(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn suggest_duplicates(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<MatchResult>, CommandError> {
    suggest_duplicates_impl(&state, part_id)
}

pub fn record_equivalence_impl(
    state: &AppState,
    a: PartId,
    b: PartId,
    decision: String,
    note: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.record_equivalence(&a, &b, &decision, &note)?)
}

#[tauri::command]
#[specta::specta]
pub fn record_equivalence(
    state: State<'_, AppState>,
    a: PartId,
    b: PartId,
    decision: String,
    note: String,
) -> Result<(), CommandError> {
    record_equivalence_impl(&state, a, b, decision, note)
}

pub fn add_alias_impl(
    state: &AppState,
    kind: String,
    value: String,
    part_id: PartId,
    source: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.add_alias(&kind, &value, &part_id, &source)?)
}

#[tauri::command]
#[specta::specta]
pub fn add_alias(
    state: State<'_, AppState>,
    kind: String,
    value: String,
    part_id: PartId,
    source: String,
) -> Result<(), CommandError> {
    add_alias_impl(&state, kind, value, part_id, source)
}

pub fn set_tags_impl(
    state: &AppState,
    part_id: PartId,
    tags: Vec<String>,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_tags(&part_id, &tags)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_tags(
    state: State<'_, AppState>,
    part_id: PartId,
    tags: Vec<String>,
) -> Result<(), CommandError> {
    set_tags_impl(&state, part_id, tags)
}

// ---------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------

pub fn get_setting_impl(state: &AppState, key: String) -> Result<Option<String>, CommandError> {
    Ok(lock(state)?.get_setting(&key)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_setting(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<String>, CommandError> {
    get_setting_impl(&state, key)
}

pub fn set_setting_impl(state: &AppState, key: String, value: String) -> Result<(), CommandError> {
    Ok(lock(state)?.set_setting(&key, &value)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<(), CommandError> {
    set_setting_impl(&state, key, value)
}

// ---------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------

pub fn dashboard_summary_impl(state: &AppState) -> Result<DashboardSummary, CommandError> {
    Ok(lock(state)?.dashboard_summary()?)
}

#[tauri::command]
#[specta::specta]
pub fn dashboard_summary(state: State<'_, AppState>) -> Result<DashboardSummary, CommandError> {
    dashboard_summary_impl(&state)
}

pub fn recent_transactions_impl(
    state: &AppState,
    limit: i64,
) -> Result<Vec<RecentTxn>, CommandError> {
    Ok(lock(state)?.recent_transactions(limit)?)
}

#[tauri::command]
#[specta::specta]
pub fn recent_transactions(
    state: State<'_, AppState>,
    limit: i64,
) -> Result<Vec<RecentTxn>, CommandError> {
    recent_transactions_impl(&state, limit)
}

// ---------------------------------------------------------------------
// Bins
// ---------------------------------------------------------------------

pub fn list_bins_impl(state: &AppState) -> Result<Vec<BinSummary>, CommandError> {
    Ok(lock(state)?.list_bins()?)
}

#[tauri::command]
#[specta::specta]
pub fn list_bins(state: State<'_, AppState>) -> Result<Vec<BinSummary>, CommandError> {
    list_bins_impl(&state)
}

pub fn rename_bin_impl(
    state: &AppState,
    old_label: String,
    new_label: String,
) -> Result<u32, CommandError> {
    Ok(lock(state)?.rename_bin(&old_label, &new_label)?)
}

#[tauri::command]
#[specta::specta]
pub fn rename_bin(
    state: State<'_, AppState>,
    old_label: String,
    new_label: String,
) -> Result<u32, CommandError> {
    rename_bin_impl(&state, old_label, new_label)
}

// ---------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------

pub fn validate_invariants_impl(state: &AppState) -> Result<ValidationReport, CommandError> {
    Ok(lock(state)?.validate_invariants()?)
}

#[tauri::command]
#[specta::specta]
pub fn validate_invariants(state: State<'_, AppState>) -> Result<ValidationReport, CommandError> {
    validate_invariants_impl(&state)
}

// ---------------------------------------------------------------------
// Development-only seed data
// ---------------------------------------------------------------------

/// Populate a representative dataset for UI development (parts across
/// several categories, stock via the ledger, a couple of projects, some
/// manufacturer variants/listings/dimensions) — see
/// `inventory_db::dev_seed` for the actual dataset. Idempotent: no-ops
/// (returns `Ok(0)`) if the database already has any part. Debug-only: the
/// command itself is registered in every build (see `builder` below), but
/// only this debug body runs the real dataset against the database; the
/// `not(debug_assertions)` variant below is what actually ships in a
/// release binary.
#[cfg(debug_assertions)]
pub fn dev_seed_impl(state: &AppState) -> Result<u32, CommandError> {
    let mut db = lock(state)?;
    Ok(inventory_db::dev_seed::run(&mut db)?)
}

/// Release stub: never touches a user's production database. Kept as a
/// real (if inert) command — rather than omitted from release builds —
/// so `dev_seed` appears in exactly one `collect_commands!` list that's
/// identical across build profiles; see `builder` below.
#[cfg(not(debug_assertions))]
pub fn dev_seed_impl(_state: &AppState) -> Result<u32, CommandError> {
    Err(CommandError {
        code: "internal".to_string(),
        message: "dev seed is only available in debug builds".to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn dev_seed(state: State<'_, AppState>) -> Result<u32, CommandError> {
    dev_seed_impl(&state)
}

// ---------------------------------------------------------------------
// Bindings builder
// ---------------------------------------------------------------------

/// Every command registered with `tauri_specta`, shared by `main`'s
/// `invoke_handler` wiring and the `export_bindings` test below so the two
/// can never drift apart.
///
/// This is the single command list for every build profile. `dev_seed` is
/// always included — only its body is `#[cfg(debug_assertions)]`-gated
/// (see above) — so there is nothing profile-specific to keep in sync
/// here: a future command addition either lands in this one list and the
/// drift-checked bindings, or doesn't compile.
pub fn builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        // Milli-unit quantities, currency micros, and display orders are all
        // i64 but stay far inside Number.MAX_SAFE_INTEGER for any realistic
        // inventory; opt out of specta-typescript's BigInt-precision guard
        // rather than exporting them as `bigint` (which the webview's JSON
        // transport can't represent anyway) or as a lossy separate type.
        .dangerously_cast_bigints_to_number()
        .commands(tauri_specta::collect_commands![
            app_status,
            list_parts,
            get_part,
            create_part,
            update_part,
            set_part_archived,
            get_stock,
            apply_ledger_op,
            apply_group,
            reverse_transaction,
            reverse_group,
            list_transactions,
            get_group,
            list_projects,
            create_project,
            set_attribute,
            get_attributes,
            clear_attribute,
            add_dimension,
            list_dimensions,
            remove_dimension,
            add_variant,
            set_preferred_variant,
            add_supplier_listing,
            list_variants,
            list_supplier_listings,
            get_tags,
            list_categories,
            category_attributes,
            category_attribute_defs,
            preview_unit_value,
            create_category,
            duplicate_category,
            create_custom_attribute,
            attach_attribute,
            set_attribute_hidden,
            reorder_attribute,
            search,
            find_matches,
            suggest_duplicates,
            record_equivalence,
            add_alias,
            set_tags,
            validate_invariants,
            dev_seed,
            dashboard_summary,
            recent_transactions,
            list_bins,
            rename_bin,
            get_setting,
            set_setting,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use inventory_core::quantity::{Quantity, QuantityUnit};
    use std::sync::Mutex;

    fn misc_category() -> CategoryId {
        CategoryId::from_string(inventory_db::MISC_CATEGORY_ID.to_string()).unwrap()
    }

    fn part_draft(name: &str) -> PartDraft {
        PartDraft {
            display_name: name.to_string(),
            category_id: misc_category(),
            description: String::new(),
            bin_label: None,
            usage_behavior: "usually_consumed".to_string(),
            quantity_unit: QuantityUnit::Each,
            low_stock_threshold: None,
            public_notes: String::new(),
            private_notes: String::new(),
        }
    }

    #[test]
    fn commands_map_typed_errors() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("Consume test part")).unwrap();

        // Freshly created parts have zero stock; consuming any amount from
        // "available" must fail with a typed InsufficientStock error.
        let op = LedgerOp::ConsumeAvailable {
            part_id: part.id,
            quantity: Quantity::from_whole(1).unwrap(),
            project_id: None,
            note: "test consume beyond stock".to_string(),
        };
        let err = apply_ledger_op_impl(&state, op).unwrap_err();

        assert_eq!(err.code, "insufficient_stock");
        // Display text ("insufficient stock: ..."), never the Debug
        // representation (which would look like `InsufficientStock("...")`).
        assert!(
            err.message.starts_with("insufficient stock:"),
            "expected Display-formatted message, got: {}",
            err.message
        );
        assert!(!err.message.contains("InsufficientStock("));
    }

    #[test]
    fn dashboard_summary_command_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        create_part_impl(&state, part_draft("Dashboard test part")).unwrap();

        let summary = dashboard_summary_impl(&state).unwrap();
        assert_eq!(summary.part_count, 1);
        assert_eq!(summary.metadata_incomplete_count, 1);
    }

    #[test]
    fn recent_transactions_command_round_trips_and_flags_reversibility() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("Recent activity part")).unwrap();
        let op = LedgerOp::Receive {
            part_id: part.id.clone(),
            quantity: Quantity::from_whole(5).unwrap(),
            note: "initial".to_string(),
        };
        apply_ledger_op_impl(&state, op).unwrap();

        let recent = recent_transactions_impl(&state, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].part_id, part.id);
        assert_eq!(recent[0].display_name, "Recent activity part");
        assert!(recent[0].reversible);
    }

    #[test]
    fn list_bins_command_groups_by_label_including_a_distinct_unassigned_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let mut binned_a = part_draft("binned a");
        binned_a.bin_label = Some("A1".to_string());
        create_part_impl(&state, binned_a).unwrap();
        let mut binned_b = part_draft("binned b, same bin");
        binned_b.bin_label = Some("A1".to_string());
        create_part_impl(&state, binned_b).unwrap();
        create_part_impl(&state, part_draft("unbinned")).unwrap();

        let bins = list_bins_impl(&state).unwrap();
        assert_eq!(bins.len(), 2);
        let a1 = bins
            .iter()
            .find(|b| b.bin_label.as_deref() == Some("A1"))
            .unwrap();
        assert_eq!(a1.part_count, 2);
        let unassigned = bins.iter().find(|b| b.bin_label.is_none()).unwrap();
        assert_eq!(unassigned.part_count, 1);
    }

    #[test]
    fn rename_bin_command_moves_every_part_and_rejects_an_empty_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let mut old_bin_part = part_draft("old bin part");
        old_bin_part.bin_label = Some("OLD".to_string());
        create_part_impl(&state, old_bin_part).unwrap();

        let moved = rename_bin_impl(&state, "OLD".to_string(), "NEW".to_string()).unwrap();
        assert_eq!(moved, 1);

        let bins = list_bins_impl(&state).unwrap();
        assert!(bins.iter().any(|b| b.bin_label.as_deref() == Some("NEW")));
        assert!(!bins.iter().any(|b| b.bin_label.as_deref() == Some("OLD")));

        let err = rename_bin_impl(&state, "NEW".to_string(), "   ".to_string()).unwrap_err();
        assert_eq!(err.code, "invalid_bin_label");
    }

    #[test]
    fn search_command_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("Searchable Widget")).unwrap();

        let hits = search_impl(&state, "Searchable".to_string()).unwrap();
        assert!(hits.iter().any(|h| h.part_id == part.id));
    }

    #[test]
    fn category_attribute_defs_command_carries_data_type_and_choices() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let resistor = list_categories_impl(&state)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "Resistor")
            .unwrap()
            .id;

        let defs = category_attribute_defs_impl(&state, resistor).unwrap();
        let resistance = defs.iter().find(|d| d.key == "resistance").unwrap();
        assert_eq!(resistance.data_type, "number_unit");
        assert_eq!(resistance.unit_kind.as_deref(), Some("resistance"));
        assert!(resistance.identity);
        let mounting = defs.iter().find(|d| d.key == "mounting_style").unwrap();
        assert_eq!(mounting.data_type, "choice");
        assert!(!mounting.choices.is_empty());
    }

    #[test]
    fn preview_unit_value_command_formats_and_rejects() {
        assert_eq!(
            preview_unit_value_impl("resistance".to_string(), "10k".to_string()).unwrap(),
            "10 kΩ"
        );
        let err =
            preview_unit_value_impl("resistance".to_string(), "10 V".to_string()).unwrap_err();
        assert_eq!(err.code, "invalid_attribute_value");
    }

    #[test]
    fn create_project_command_is_listed_alphabetically() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        assert_eq!(list_projects_impl(&state).unwrap(), Vec::new());

        let blinky = create_project_impl(&state, "Blinky Board".to_string()).unwrap();

        let projects = list_projects_impl(&state).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, blinky);
        assert_eq!(projects[0].name, "Blinky Board");
    }

    #[test]
    fn setting_round_trips_and_is_none_until_set() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        assert_eq!(
            get_setting_impl(&state, "saved_views".to_string()).unwrap(),
            None
        );

        set_setting_impl(&state, "saved_views".to_string(), "[]".to_string()).unwrap();

        assert_eq!(
            get_setting_impl(&state, "saved_views".to_string()).unwrap(),
            Some("[]".to_string())
        );
    }

    #[test]
    fn status_reports_version_and_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let status = status_of(&state, "0.1.0").unwrap();

        assert_eq!(status.app_version, "0.1.0");
        assert_eq!(
            status.schema_version,
            inventory_db::SUPPORTED_SCHEMA_VERSION
        );
        assert!(
            status.data_dir.ends_with("data"),
            "expected data_dir to end with the temp data dir, got: {}",
            status.data_dir
        );
    }

    #[test]
    fn poisoned_mutex_maps_to_internal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.db.lock().unwrap();
            panic!("poisoning the mutex for the test");
        }));

        let err = status_of(&state, "0.1.0").unwrap_err();
        assert_eq!(err.code, "internal");
        assert_eq!(err.message, "database lock poisoned; restart the app");
    }

    /// Guards against a committed `apps/desktop/src/bindings.gen.ts` drifting
    /// from the Rust command surface: generates fresh bindings (via the same
    /// `builder()` + exporter every command wrapper feeds) to a temp file,
    /// then asserts the result matches the committed file (modulo line
    /// endings). A stale committed file — e.g. after adding/renaming a
    /// `#[tauri::command]` without re-running the export — fails `cargo
    /// test` instead of passing silently.
    ///
    /// Run with `EXPORT_BINDINGS=1 cargo test -p electronics-inventory
    /// export_bindings` to regenerate `bindings.gen.ts` in place after a
    /// legitimate command-surface change, then commit the result.
    #[test]
    fn export_bindings() {
        const COMMITTED_PATH: &str = "../src/bindings.gen.ts";

        if std::env::var_os("EXPORT_BINDINGS").is_some() {
            builder()
                .export(specta_typescript::Typescript::default(), COMMITTED_PATH)
                .expect("failed to export typescript bindings");
            return;
        }

        // Export to a temp file rather than building a string directly: the
        // `tauri_specta::Builder::export` -> `LanguageExt::export` path only
        // writes to a filesystem path, so generating through the identical
        // mechanism (rather than reimplementing it) guarantees the
        // comparison isn't fooled by some formatting difference between two
        // different generation routes.
        let dir = tempfile::tempdir().unwrap();
        let fresh_path = dir.path().join("bindings.gen.ts");
        builder()
            .export(specta_typescript::Typescript::default(), &fresh_path)
            .expect("failed to export typescript bindings");

        let fresh =
            std::fs::read_to_string(&fresh_path).expect("failed to read freshly exported bindings");
        let committed = std::fs::read_to_string(COMMITTED_PATH)
            .expect("failed to read committed apps/desktop/src/bindings.gen.ts");

        // Normalize CRLF -> LF before comparing. The exporter always writes
        // LF (Rust's `std::fs::write` performs no newline translation), but
        // a working tree checked out with `core.autocrlf=true` (the default
        // on this repo, on Windows) rewrites those LFs to CRLF on disk. That
        // is a per-checkout artifact, not real drift, so line endings must
        // not be able to fail this assertion on their own.
        let normalize = |s: &str| s.replace("\r\n", "\n");
        assert_eq!(
            normalize(&fresh),
            normalize(&committed),
            "bindings.gen.ts is out of date; re-run the binding export and commit the result"
        );
    }
}
