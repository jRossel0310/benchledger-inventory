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

use inventory_core::ids::{CategoryId, GroupId, PartId, TransactionId, VariantId};
use inventory_core::ledger::LedgerOp;
use inventory_db::categories::CategoryRecord;
use inventory_db::dimensions::{DimensionDraft, DimensionRecord};
use inventory_db::ledger::{GroupRecord, TransactionRecord};
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
// Bindings builder
// ---------------------------------------------------------------------

/// Every command registered with `tauri_specta`, shared by `main`'s
/// `invoke_handler` wiring and the `export_bindings` test below so the two
/// can never drift apart.
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
            set_attribute,
            get_attributes,
            clear_attribute,
            add_dimension,
            list_dimensions,
            remove_dimension,
            add_variant,
            set_preferred_variant,
            add_supplier_listing,
            list_categories,
            category_attributes,
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

    /// Writes the TypeScript bindings consumed by `apps/desktop/src`. Run as
    /// part of `cargo test --workspace`; also wired into `main`'s dev build
    /// via `#[cfg(debug_assertions)]` for convenience during development.
    #[test]
    fn export_bindings() {
        builder()
            .export(
                specta_typescript::Typescript::default(),
                "../src/bindings.gen.ts",
            )
            .expect("failed to export typescript bindings");
    }
}
