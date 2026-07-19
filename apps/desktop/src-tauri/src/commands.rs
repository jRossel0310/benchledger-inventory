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

use inventory_core::ids::{
    BomItemId, CategoryId, GroupId, ImportId, ImportLineId, PartId, ProjectId, TransactionId,
    VariantId,
};
use inventory_core::ledger::LedgerOp;
use inventory_core::quantity::Quantity;
use inventory_db::attachment_store::{AttachmentKind, AttachmentRef};
use inventory_db::bins::BinSummary;
use inventory_db::bom::{BomItemDraft, BomItemRecord};
use inventory_db::build::BuildPlan;
use inventory_db::categories::{AttributeDefRow, CategoryRecord};
use inventory_db::dashboard::{DashboardSummary, RecentTxn};
use inventory_db::dimensions::{DimensionDraft, DimensionRecord};
use inventory_db::enrichment::{AppliedField, EnrichmentDiff, DIGIKEY_ENVIRONMENT_SETTING};
use inventory_db::history::{HistoryFilter, HistoryPage};
use inventory_db::import_commit::LineDecision;
use inventory_db::import_review::ImportReview;
use inventory_db::imports::{ImportLineRecord, ImportRecord};
use inventory_db::ledger::{GroupRecord, ProjectRef, TransactionRecord};
use inventory_db::matching::{MatchCandidate, MatchResult};
use inventory_db::parts::{
    ListingDraft, ListingRecord, PartDraft, PartRecord, PartStockRow, VariantDraft, VariantRecord,
};
use inventory_db::projects::{ProjectDraft, ProjectRecord, ProjectStatus};
use inventory_db::search::SearchHit;
use inventory_db::validate::ValidationReport;
use inventory_db::{Database, DbError};
use inventory_import::parser::ImportError;
use inventory_sync::github::{GitHubApi, GitHubError, ReqwestGitHub};
use inventory_sync::publish::{
    PublishConfig, PublishOutcome, BRANCH_SETTING, DEFAULT_BRANCH, DEFAULT_PATH,
    LAST_PUBLISHED_AT_KEY, OWNER_SETTING, PATH_SETTING, PENDING_PUBLISH_KEY, REPO_SETTING,
    VERCEL_URL_SETTING,
};
use inventory_sync::SyncError;

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
            DbError::BomItemNotFound => "bom_item_not_found",
            DbError::BomItemExists => "bom_item_exists",
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
            DbError::AttachmentNotFound => "attachment_not_found",
            DbError::InvalidAttachment(_) => "invalid_attachment",
            DbError::ImportNotFound => "import_not_found",
            DbError::Json(_) => "json",
            DbError::ImportNotCommittable => "import_not_committable",
            DbError::ImportNotReversible => "import_not_reversible",
            DbError::ImportLineNotFound => "import_line_not_found",
            DbError::NonPartLineNotReceivable { .. } => "non_part_line_not_receivable",
            DbError::DuplicateLineDecision { .. } => "duplicate_line_decision",
            DbError::InvalidEnrichmentSource(_) => "invalid_enrichment_source",
            DbError::EnrichmentReviewRequired(_) => "enrichment_review_required",
        };
        CommandError {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}

/// `inventory_import::parser::ImportError` -> `CommandError`, the same
/// Display-text-never-Debug contract as `DbError`'s conversion above. This
/// is a separate source error type (parsing happens before anything touches
/// `inventory-db`), so it gets its own small, exhaustive `code` match rather
/// than folding into `DbError`'s.
impl From<ImportError> for CommandError {
    fn from(e: ImportError) -> Self {
        let code = match &e {
            ImportError::UnsupportedFormat => "unsupported_format",
            ImportError::Empty => "empty",
            ImportError::Malformed(_) => "malformed",
            ImportError::Encoding(_) => "encoding",
            ImportError::Pdf(_) => "pdf",
        };
        CommandError {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}

/// `inventory_core::secrets::SecretsError` -> `CommandError`. The source
/// type carries only a fixed operation label (see its own doc comment) —
/// never a secret or a raw `keyring::Error` payload — so `e.to_string()` is
/// safe to use verbatim as the `message`, the same Display-not-Debug
/// contract every other `From` impl here follows. `SecretsError` has a
/// single variant today, so every conversion gets the same `code`; that's
/// fine (nothing downstream branches on it), and it stays exhaustive to add
/// a per-variant match automatically if a second variant is ever added.
impl From<inventory_core::secrets::SecretsError> for CommandError {
    fn from(e: inventory_core::secrets::SecretsError) -> Self {
        CommandError {
            code: "secrets_backend".to_string(),
            message: e.to_string(),
        }
    }
}

/// `inventory_sync::SyncError` -> `CommandError`, exhaustive over every
/// variant so a future addition fails to compile rather than falling into a
/// generic bucket. `Db` re-uses `DbError`'s own conversion (same codes the
/// rest of this file produces); `GitHub` maps each `GitHubError` variant to
/// its own stable code, with the message being that variant's fixed Display
/// string (never a response body or the token — see `github.rs`'s
/// secrets-discipline doc).
impl From<SyncError> for CommandError {
    fn from(e: SyncError) -> Self {
        match e {
            SyncError::Db(db) => db.into(),
            SyncError::Json(_) => CommandError {
                code: "json".to_string(),
                message: e.to_string(),
            },
            SyncError::NotConfigured => CommandError {
                code: "publish_not_configured".to_string(),
                message: e.to_string(),
            },
            SyncError::TokenMissing => CommandError {
                code: "github_token_missing".to_string(),
                message: e.to_string(),
            },
            SyncError::GitHub(ref gh) => {
                let code = match gh {
                    GitHubError::Auth => "github_auth",
                    GitHubError::NotFound => "github_not_found",
                    GitHubError::Conflict => "github_conflict",
                    GitHubError::RateLimited => "github_rate_limited",
                    GitHubError::Network(_) => "github_network",
                    GitHubError::Api(_) => "github_api",
                };
                CommandError {
                    code: code.to_string(),
                    message: e.to_string(),
                }
            }
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
// Projects and BOMs (Phase 4 Task 5): the rich project lifecycle, BOM
// editing, reserve/release, and atomic build-from-BOM. Every one of these
// is a thin wrapper over `inventory_db::projects`/`bom`/`build` — the
// `list_projects`/`create_project` stub above stays as-is for the
// quick-action pickers.
// ---------------------------------------------------------------------

pub fn create_project_full_impl(
    state: &AppState,
    draft: ProjectDraft,
) -> Result<ProjectRecord, CommandError> {
    Ok(lock(state)?.create_project_full(&draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn create_project_full(
    state: State<'_, AppState>,
    draft: ProjectDraft,
) -> Result<ProjectRecord, CommandError> {
    create_project_full_impl(&state, draft)
}

pub fn list_projects_full_impl(
    state: &AppState,
    status_filter: Option<ProjectStatus>,
) -> Result<Vec<ProjectRecord>, CommandError> {
    Ok(lock(state)?.list_projects_full(status_filter)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_projects_full(
    state: State<'_, AppState>,
    status_filter: Option<ProjectStatus>,
) -> Result<Vec<ProjectRecord>, CommandError> {
    list_projects_full_impl(&state, status_filter)
}

pub fn get_project_impl(
    state: &AppState,
    id: ProjectId,
) -> Result<Option<ProjectRecord>, CommandError> {
    Ok(lock(state)?.get_project(&id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_project(
    state: State<'_, AppState>,
    id: ProjectId,
) -> Result<Option<ProjectRecord>, CommandError> {
    get_project_impl(&state, id)
}

pub fn update_project_impl(state: &AppState, record: ProjectRecord) -> Result<(), CommandError> {
    Ok(lock(state)?.update_project(&record)?)
}

#[tauri::command]
#[specta::specta]
pub fn update_project(
    state: State<'_, AppState>,
    record: ProjectRecord,
) -> Result<(), CommandError> {
    update_project_impl(&state, record)
}

pub fn set_project_status_impl(
    state: &AppState,
    id: ProjectId,
    status: ProjectStatus,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_project_status(&id, status)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_project_status(
    state: State<'_, AppState>,
    id: ProjectId,
    status: ProjectStatus,
) -> Result<(), CommandError> {
    set_project_status_impl(&state, id, status)
}

pub fn duplicate_project_impl(
    state: &AppState,
    id: ProjectId,
    new_name: String,
) -> Result<ProjectRecord, CommandError> {
    Ok(lock(state)?.duplicate_project(&id, &new_name)?)
}

#[tauri::command]
#[specta::specta]
pub fn duplicate_project(
    state: State<'_, AppState>,
    id: ProjectId,
    new_name: String,
) -> Result<ProjectRecord, CommandError> {
    duplicate_project_impl(&state, id, new_name)
}

pub fn archive_project_impl(state: &AppState, id: ProjectId) -> Result<(), CommandError> {
    Ok(lock(state)?.archive_project(&id)?)
}

#[tauri::command]
#[specta::specta]
pub fn archive_project(state: State<'_, AppState>, id: ProjectId) -> Result<(), CommandError> {
    archive_project_impl(&state, id)
}

pub fn add_bom_item_impl(
    state: &AppState,
    project_id: ProjectId,
    draft: BomItemDraft,
) -> Result<BomItemRecord, CommandError> {
    Ok(lock(state)?.add_bom_item(&project_id, &draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn add_bom_item(
    state: State<'_, AppState>,
    project_id: ProjectId,
    draft: BomItemDraft,
) -> Result<BomItemRecord, CommandError> {
    add_bom_item_impl(&state, project_id, draft)
}

pub fn update_bom_item_impl(
    state: &AppState,
    id: BomItemId,
    draft: BomItemDraft,
) -> Result<BomItemRecord, CommandError> {
    Ok(lock(state)?.update_bom_item(&id, &draft)?)
}

#[tauri::command]
#[specta::specta]
pub fn update_bom_item(
    state: State<'_, AppState>,
    id: BomItemId,
    draft: BomItemDraft,
) -> Result<BomItemRecord, CommandError> {
    update_bom_item_impl(&state, id, draft)
}

pub fn remove_bom_item_impl(state: &AppState, id: BomItemId) -> Result<(), CommandError> {
    Ok(lock(state)?.remove_bom_item(&id)?)
}

#[tauri::command]
#[specta::specta]
pub fn remove_bom_item(state: State<'_, AppState>, id: BomItemId) -> Result<(), CommandError> {
    remove_bom_item_impl(&state, id)
}

pub fn set_bom_substitutes_impl(
    state: &AppState,
    bom_item_id: BomItemId,
    part_ids: Vec<PartId>,
) -> Result<(), CommandError> {
    Ok(lock(state)?.set_bom_substitutes(&bom_item_id, &part_ids)?)
}

#[tauri::command]
#[specta::specta]
pub fn set_bom_substitutes(
    state: State<'_, AppState>,
    bom_item_id: BomItemId,
    part_ids: Vec<PartId>,
) -> Result<(), CommandError> {
    set_bom_substitutes_impl(&state, bom_item_id, part_ids)
}

pub fn get_bom_item_impl(
    state: &AppState,
    id: BomItemId,
) -> Result<Option<BomItemRecord>, CommandError> {
    Ok(lock(state)?.get_bom_item(&id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_bom_item(
    state: State<'_, AppState>,
    id: BomItemId,
) -> Result<Option<BomItemRecord>, CommandError> {
    get_bom_item_impl(&state, id)
}

pub fn list_bom_impl(
    state: &AppState,
    project_id: ProjectId,
) -> Result<Vec<BomItemRecord>, CommandError> {
    Ok(lock(state)?.list_bom(&project_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_bom(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<Vec<BomItemRecord>, CommandError> {
    list_bom_impl(&state, project_id)
}

pub fn import_bom_impl(
    state: &AppState,
    project_id: ProjectId,
    rows: Vec<BomItemDraft>,
) -> Result<Vec<BomItemRecord>, CommandError> {
    Ok(lock(state)?.import_bom(&project_id, rows)?)
}

#[tauri::command]
#[specta::specta]
pub fn import_bom(
    state: State<'_, AppState>,
    project_id: ProjectId,
    rows: Vec<BomItemDraft>,
) -> Result<Vec<BomItemRecord>, CommandError> {
    import_bom_impl(&state, project_id, rows)
}

pub fn reserve_bom_impl(
    state: &AppState,
    project_id: ProjectId,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.reserve_bom(&project_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn reserve_bom(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<GroupRecord, CommandError> {
    reserve_bom_impl(&state, project_id)
}

pub fn release_bom_reservations_impl(
    state: &AppState,
    project_id: ProjectId,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.release_bom_reservations(&project_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn release_bom_reservations(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<GroupRecord, CommandError> {
    release_bom_reservations_impl(&state, project_id)
}

pub fn plan_build_impl(state: &AppState, project_id: ProjectId) -> Result<BuildPlan, CommandError> {
    Ok(lock(state)?.plan_build(&project_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn plan_build(
    state: State<'_, AppState>,
    project_id: ProjectId,
) -> Result<BuildPlan, CommandError> {
    plan_build_impl(&state, project_id)
}

pub fn build_from_bom_impl(
    state: &AppState,
    project_id: ProjectId,
    approved_available_lines: Vec<BomItemId>,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.build_from_bom(&project_id, &approved_available_lines)?)
}

#[tauri::command]
#[specta::specta]
pub fn build_from_bom(
    state: State<'_, AppState>,
    project_id: ProjectId,
    approved_available_lines: Vec<BomItemId>,
) -> Result<GroupRecord, CommandError> {
    build_from_bom_impl(&state, project_id, approved_available_lines)
}

pub fn associate_checkout_impl(
    state: &AppState,
    project_id: ProjectId,
    part_id: PartId,
    quantity: Quantity,
) -> Result<TransactionRecord, CommandError> {
    Ok(lock(state)?.associate_checkout(&project_id, &part_id, quantity)?)
}

#[tauri::command]
#[specta::specta]
pub fn associate_checkout(
    state: State<'_, AppState>,
    project_id: ProjectId,
    part_id: PartId,
    quantity: Quantity,
) -> Result<TransactionRecord, CommandError> {
    associate_checkout_impl(&state, project_id, part_id, quantity)
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
// Attachments (Phase 3 Task 10)
// ---------------------------------------------------------------------

pub fn add_attachment_impl(
    state: &AppState,
    bytes: Vec<u8>,
    ext: Option<String>,
    kind: AttachmentKind,
    original_name: Option<String>,
    source: String,
) -> Result<AttachmentRef, CommandError> {
    // Read the attachments dir off the (separate) layout field before locking
    // the DB, so there's no borrow overlap with the guard.
    let dir = state.layout.attachments.clone();
    Ok(lock(state)?.store_attachment(
        &dir,
        &bytes,
        ext.as_deref(),
        kind,
        original_name.as_deref(),
        &source,
    )?)
}

/// Store raw file bytes in the content-addressed store and return the stored
/// blob's metadata. Bytes cross the IPC boundary as a JSON number array
/// (`Vec<u8>` -> `number[]`), which is fine at the local-app scale this targets
/// (datasheets/photos, not gigabyte media). Storing identical bytes always
/// resolves to exactly one on-disk file and one metadata row (deduplication),
/// so the returned hash is safe to link to a part via `attach_to_part`.
#[tauri::command]
#[specta::specta]
pub fn add_attachment(
    state: State<'_, AppState>,
    bytes: Vec<u8>,
    ext: Option<String>,
    kind: AttachmentKind,
    original_name: Option<String>,
    source: String,
) -> Result<AttachmentRef, CommandError> {
    add_attachment_impl(&state, bytes, ext, kind, original_name, source)
}

pub fn attach_to_part_impl(
    state: &AppState,
    part_id: PartId,
    content_hash: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.attach_to_part(&part_id, &content_hash)?)
}

#[tauri::command]
#[specta::specta]
pub fn attach_to_part(
    state: State<'_, AppState>,
    part_id: PartId,
    content_hash: String,
) -> Result<(), CommandError> {
    attach_to_part_impl(&state, part_id, content_hash)
}

pub fn list_part_attachments_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<Vec<AttachmentRef>, CommandError> {
    Ok(lock(state)?.list_part_attachments(&part_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_part_attachments(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<Vec<AttachmentRef>, CommandError> {
    list_part_attachments_impl(&state, part_id)
}

pub fn read_attachment_impl(
    state: &AppState,
    content_hash: String,
) -> Result<Vec<u8>, CommandError> {
    let dir = state.layout.attachments.clone();
    Ok(lock(state)?.read_attachment(&dir, &content_hash)?)
}

/// Read a stored blob's bytes back (as a JSON number array) so the webview can
/// turn them into a blob URL — used to render image thumbnails and to open/
/// download non-image attachments.
#[tauri::command]
#[specta::specta]
pub fn read_attachment(
    state: State<'_, AppState>,
    content_hash: String,
) -> Result<Vec<u8>, CommandError> {
    read_attachment_impl(&state, content_hash)
}

pub fn remove_part_attachment_impl(
    state: &AppState,
    part_id: PartId,
    content_hash: String,
) -> Result<(), CommandError> {
    Ok(lock(state)?.remove_part_attachment(&part_id, &content_hash)?)
}

/// Unlink a blob from a part. The shared blob file and row are intentionally
/// left intact (other parts/dimensions may reference the same content).
#[tauri::command]
#[specta::specta]
pub fn remove_part_attachment(
    state: State<'_, AppState>,
    part_id: PartId,
    content_hash: String,
) -> Result<(), CommandError> {
    remove_part_attachment_impl(&state, part_id, content_hash)
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
// Enrichment (Phase 5c Task 6): `enrich_part_preview`/`apply_enrichment`
// are thin wrappers over `inventory_db::enrichment`, exactly like every
// other command here. `get_digikey_status`/`set_digikey_environment` are
// the sandbox/production toggle plus a credentials-configured probe that
// NEVER returns the secret itself — storing credentials from the UI is
// deferred to Phase 5d's Settings screen (5c stores them via Task 1's dev
// bin, going straight through `inventory_core::secrets`); this module
// deliberately has no set-credentials command.
// ---------------------------------------------------------------------

pub fn enrich_part_preview_impl(
    state: &AppState,
    part_id: PartId,
) -> Result<EnrichmentDiff, CommandError> {
    // Read the cache dir off the (separate) layout field before locking the
    // DB, so there's no borrow overlap with the guard (same pattern as
    // `add_attachment_impl`/`parse_and_store_import_impl`).
    let cache_dir = state.layout.cache.clone();
    Ok(lock(state)?.enrich_part_preview(&part_id, &cache_dir)?)
}

/// Preview: builds an `EnrichInput` from the part's current state, runs the
/// enrichment provider chain (DigiKey, if configured, then the
/// always-available offline description parser), and diffs the resulting
/// candidates against the part's current values. Writes nothing — see
/// `inventory_db::enrichment`'s module doc for the full compare-and-apply
/// design and the `requires_review` rule.
#[tauri::command]
#[specta::specta]
pub fn enrich_part_preview(
    state: State<'_, AppState>,
    part_id: PartId,
) -> Result<EnrichmentDiff, CommandError> {
    enrich_part_preview_impl(&state, part_id)
}

pub fn apply_enrichment_impl(
    state: &AppState,
    part_id: PartId,
    applied: Vec<AppliedField>,
) -> Result<(), CommandError> {
    Ok(lock(state)?.apply_enrichment(&part_id, &applied)?)
}

/// Apply: writes the caller-approved subset of a preview's diffs (each
/// `AppliedField` carries the value+source the caller already saw and
/// approved in `EnrichmentDiff`, so apply never re-runs the provider chain)
/// in ONE all-or-nothing transaction, upserting `field_provenance` for each
/// approved field.
#[tauri::command]
#[specta::specta]
pub fn apply_enrichment(
    state: State<'_, AppState>,
    part_id: PartId,
    applied: Vec<AppliedField>,
) -> Result<(), CommandError> {
    apply_enrichment_impl(&state, part_id, applied)
}

/// Whether DigiKey credentials are configured, plus the sandbox/production
/// environment currently selected — NEVER the credentials themselves (only
/// a `bool` and the non-secret environment string cross the IPC boundary).
/// Lets the enrichment UI explain why the DigiKey provider silently
/// contributed nothing to a preview, without needing a command that could
/// leak a secret.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DigiKeyStatus {
    pub configured: bool,
    pub environment: String,
}

pub fn get_digikey_status_impl(state: &AppState) -> Result<DigiKeyStatus, CommandError> {
    let environment = lock(state)?
        .get_setting(DIGIKEY_ENVIRONMENT_SETTING)?
        .unwrap_or_else(|| "sandbox".to_string());
    // A credential-store backend failure (as opposed to "simply never
    // configured") is treated the same as "not configured" here rather than
    // surfaced as an error: this is a best-effort status probe for the UI,
    // not an operation that itself needs the credentials to succeed, and
    // `SecretsError` never carries the secret either way (see
    // `inventory_core::secrets`'s own tests) — there's nothing sensitive
    // being swallowed by not propagating it.
    let configured = matches!(
        inventory_core::secrets::load_digikey_credentials(),
        Ok(Some(_))
    );
    Ok(DigiKeyStatus {
        configured,
        environment,
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_digikey_status(state: State<'_, AppState>) -> Result<DigiKeyStatus, CommandError> {
    get_digikey_status_impl(&state)
}

pub fn set_digikey_environment_impl(
    state: &AppState,
    environment: String,
) -> Result<(), CommandError> {
    if environment != "sandbox" && environment != "production" {
        return Err(CommandError {
            code: "invalid_digikey_environment".to_string(),
            message: format!(
                "digikey environment must be 'sandbox' or 'production', got '{environment}'"
            ),
        });
    }
    Ok(lock(state)?.set_setting(DIGIKEY_ENVIRONMENT_SETTING, &environment)?)
}

/// The sandbox/production toggle (spec §11/§16): only these two exact
/// values are accepted — anything else comes back as a typed
/// `invalid_digikey_environment` error rather than being silently stored
/// and only caught later at read time by `DigiKeyEnv::from_setting_str`'s
/// own defensive sandbox default.
#[tauri::command]
#[specta::specta]
pub fn set_digikey_environment(
    state: State<'_, AppState>,
    environment: String,
) -> Result<(), CommandError> {
    set_digikey_environment_impl(&state, environment)
}

/// Store the DigiKey OAuth2 client-credentials pair in the OS credential
/// store (Phase 5d Task 1, spec §16/ADR #3): `client_id`/`client_secret`
/// cross the IPC boundary exactly ONCE, write-only — this command returns
/// nothing (not even success echoes a value back), so there is no response
/// payload that could ever carry a credential. Neither value is logged: this
/// module has no `tracing::instrument` on any command (verified — none
/// exist anywhere in this file), so there is no attribute-capture path that
/// could pick these arguments up either.
///
/// Both values are trimmed and rejected if empty (or all-whitespace) as a
/// typed `invalid_credentials` error — never silently stored blank, which
/// would make `get_digikey_status` report `configured: true` for a pair
/// that can't actually authenticate.
pub fn set_digikey_credentials_impl(
    client_id: String,
    client_secret: String,
) -> Result<(), CommandError> {
    let client_id = client_id.trim().to_string();
    let client_secret = client_secret.trim().to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        return Err(CommandError {
            code: "invalid_credentials".to_string(),
            message: "DigiKey client ID and client secret must not be empty".to_string(),
        });
    }
    inventory_core::secrets::store_digikey_credentials(
        &inventory_core::secrets::DigiKeyCredentials {
            client_id,
            client_secret,
        },
    )?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_digikey_credentials(
    client_id: String,
    client_secret: String,
) -> Result<(), CommandError> {
    set_digikey_credentials_impl(client_id, client_secret)
}

/// Delete both DigiKey credential entries from the OS credential store.
/// Idempotent: clearing when nothing (or only a partial pair) is stored is
/// still `Ok(())` (`inventory_core::secrets::clear_digikey_credentials`'s
/// own contract), so the Settings "Remove" action never has to distinguish
/// "already absent" from "just removed".
pub fn clear_digikey_credentials_impl() -> Result<(), CommandError> {
    inventory_core::secrets::clear_digikey_credentials()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn clear_digikey_credentials() -> Result<(), CommandError> {
    clear_digikey_credentials_impl()
}

/// Result of `test_digikey_connection`: a fixed, non-secret status line for
/// the Settings "Test connection" button. `message` is always one of a
/// small set of fixed strings ("not configured" / "connected" / "rejected
/// — check credentials and environment" / "network error or timeout") —
/// NEVER a raw response body, HTTP status detail, or the credential itself;
/// see `test_digikey_connection_impl`'s mapping.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DigiKeyTestResult {
    pub ok: bool,
    pub environment: String,
    pub message: String,
}

/// Verify stored DigiKey credentials actually authenticate, without
/// spending a product-details API call: loads credentials first (no
/// network — absent credentials short-circuit straight to a
/// `{ok: false, message: "not configured"}` result), then, only if present,
/// runs ONLY the OAuth2 token request via `DigiKeyClient::probe_token`
/// (same 15s/5s timeouts every other DigiKey call uses) against the
/// currently-configured sandbox/production environment.
///
/// Every branch maps to one of four fixed strings — never a response body
/// or the secret itself:
/// - no credentials stored: `"not configured"`, `ok: false`, no request sent;
/// - `probe_token` succeeds: `"connected"`, `ok: true`;
/// - the token endpoint rejects the credentials (`EnrichError::Config`,
///   covers any non-success HTTP status from `access_token`, including an
///   auth rejection or an environment/credential mismatch):
///   `"rejected — check credentials and environment"`, `ok: false`;
/// - anything else (`EnrichError::Network`/`Provider`/`Parse` — a transport
///   failure, timeout, or malformed response): `"network error or timeout"`,
///   `ok: false`.
pub fn test_digikey_connection_impl(state: &AppState) -> Result<DigiKeyTestResult, CommandError> {
    let environment_setting = lock(state)?
        .get_setting(DIGIKEY_ENVIRONMENT_SETTING)?
        .unwrap_or_default();
    let environment = inventory_enrich::DigiKeyEnv::from_setting_str(&environment_setting);
    let environment_str = environment.as_str().to_string();

    let configured = matches!(
        inventory_core::secrets::load_digikey_credentials(),
        Ok(Some(_))
    );
    if !configured {
        return Ok(DigiKeyTestResult {
            ok: false,
            environment: environment_str,
            message: "not configured".to_string(),
        });
    }

    let cache_dir = state.layout.cache.clone();
    let client = inventory_enrich::DigiKeyClient::new(inventory_enrich::DigiKeyConfig {
        environment,
        cache_dir,
    });
    let (ok, message) = match client.probe_token() {
        Ok(()) => (true, "connected"),
        Err(inventory_enrich::EnrichError::Config(_)) => {
            (false, "rejected — check credentials and environment")
        }
        Err(_) => (false, "network error or timeout"),
    };
    Ok(DigiKeyTestResult {
        ok,
        environment: environment_str,
        message: message.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn test_digikey_connection(
    state: State<'_, AppState>,
) -> Result<DigiKeyTestResult, CommandError> {
    test_digikey_connection_impl(&state)
}

// ---------------------------------------------------------------------
// Publishing (Phase 6 Task 4): thin wrappers over
// `inventory_sync::publish`. The publish path itself only ever sees the
// `GitHubApi` trait; this layer is the ONE place the production
// `ReqwestGitHub` is constructed (see `github_api`), so `publish.rs`
// stays fully testable against the mock while these commands add nothing
// but config/token plumbing. Token handling mirrors the DigiKey
// credential commands exactly: write-only IPC, trimmed, typed reject on
// empty, never echoed back in any response payload.
// ---------------------------------------------------------------------

/// The single construction site for the live GitHub client: consumes the
/// freshly-loaded token (the `expose()` call is the module's only secret
/// extraction) and hands back the reqwest-backed `GitHubApi`
/// implementation, which holds the token in memory only.
fn github_api(token: inventory_core::secrets::GitHubToken) -> ReqwestGitHub {
    ReqwestGitHub::new(token.expose().to_string())
}

/// Load the GitHub token for a publish attempt: `Ok(None)` (never stored)
/// becomes the typed `TokenMissing`; a credential-store backend failure
/// propagates as its own `secrets_backend` error rather than being
/// conflated with "missing".
fn require_github_token() -> Result<inventory_core::secrets::GitHubToken, CommandError> {
    match inventory_core::secrets::load_github_token() {
        Ok(Some(token)) => Ok(token),
        Ok(None) => Err(SyncError::TokenMissing.into()),
        Err(e) => Err(e.into()),
    }
}

/// Publish state for the Settings screen and the Dashboard card — NEVER
/// the token (only whether config exists, where it points, and when/whether
/// the last publish landed cross the IPC boundary).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct PublishStatus {
    /// Whether `publish_owner`/`publish_repo` are set. Deliberately says
    /// nothing about the token — token presence is only ever probed by
    /// `test_github_connection`, which needs it anyway.
    pub configured: bool,
    /// `"owner/repo"` when configured.
    pub repo: Option<String>,
    pub last_published_at: Option<String>,
    /// Whether a failed publish is waiting for a retry.
    pub pending: bool,
    pub vercel_url: Option<String>,
}

pub fn get_publish_status_impl(state: &AppState) -> Result<PublishStatus, CommandError> {
    let db = lock(state)?;
    let config = PublishConfig::load(&db).map_err(CommandError::from)?;
    let last_published_at = db.get_app_state(LAST_PUBLISHED_AT_KEY)?;
    let pending = db.get_app_state(PENDING_PUBLISH_KEY)?.is_some();
    Ok(PublishStatus {
        configured: config.is_some(),
        repo: config.as_ref().map(|c| format!("{}/{}", c.owner, c.repo)),
        last_published_at,
        pending,
        vercel_url: config.and_then(|c| c.vercel_url),
    })
}

#[tauri::command]
#[specta::specta]
pub fn get_publish_status(state: State<'_, AppState>) -> Result<PublishStatus, CommandError> {
    get_publish_status_impl(&state)
}

/// Store the publish configuration in the `settings` table (none of it is
/// secret). Every value is trimmed; owner/repo must be non-empty (typed
/// `invalid_publish_config` otherwise — never silently stored blank, which
/// would make `get_publish_status` report `configured: true` for a target
/// that can't be addressed); branch/path fall back to their defaults when
/// blank; a blank/absent Vercel URL is stored as `""`, which
/// `PublishConfig::load` reads back as `None`.
pub fn set_publish_config_impl(
    state: &AppState,
    owner: String,
    repo: String,
    branch: String,
    path: String,
    vercel_url: Option<String>,
) -> Result<(), CommandError> {
    let owner = owner.trim().to_string();
    let repo = repo.trim().to_string();
    if owner.is_empty() || repo.is_empty() {
        return Err(CommandError {
            code: "invalid_publish_config".to_string(),
            message: "publish owner and repository must not be empty".to_string(),
        });
    }
    let branch = match branch.trim() {
        "" => DEFAULT_BRANCH,
        trimmed => trimmed,
    };
    let path = match path.trim() {
        "" => DEFAULT_PATH,
        trimmed => trimmed,
    };
    let vercel_url = vercel_url.as_deref().unwrap_or("").trim().to_string();

    let mut db = lock(state)?;
    db.set_setting(OWNER_SETTING, &owner)?;
    db.set_setting(REPO_SETTING, &repo)?;
    db.set_setting(BRANCH_SETTING, branch)?;
    db.set_setting(PATH_SETTING, path)?;
    db.set_setting(VERCEL_URL_SETTING, &vercel_url)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_publish_config(
    state: State<'_, AppState>,
    owner: String,
    repo: String,
    branch: String,
    path: String,
    vercel_url: Option<String>,
) -> Result<(), CommandError> {
    set_publish_config_impl(&state, owner, repo, branch, path, vercel_url)
}

/// Store the GitHub publish token in the OS credential store (spec §16/ADR
/// #3, same contract as `set_digikey_credentials`): the token crosses the
/// IPC boundary exactly ONCE, write-only — this command returns nothing, so
/// no response payload can ever carry it, and nothing here logs it (this
/// module has no `tracing::instrument` on any command). Trimmed; an
/// empty/whitespace-only token is a typed `invalid_token` reject rather
/// than being silently stored blank.
pub fn set_github_token_impl(token: String) -> Result<(), CommandError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(CommandError {
            code: "invalid_token".to_string(),
            message: "GitHub token must not be empty".to_string(),
        });
    }
    inventory_core::secrets::store_github_token(token)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_github_token(token: String) -> Result<(), CommandError> {
    set_github_token_impl(token)
}

/// Delete the stored GitHub token. Idempotent: clearing when nothing is
/// stored is still `Ok(())` (`inventory_core::secrets::clear_github_token`'s
/// own contract).
pub fn clear_github_token_impl() -> Result<(), CommandError> {
    inventory_core::secrets::clear_github_token()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn clear_github_token() -> Result<(), CommandError> {
    clear_github_token_impl()
}

/// Result of `test_github_connection`: a fixed, non-secret status line for
/// the Settings "Test connection" button. `message` is always one of a
/// small set of fixed strings ("not configured" / "connected" /
/// "rejected — check token" / "repo or branch not found" / "network error
/// or timeout") — NEVER a response body, HTTP status detail, or the token
/// itself; see `test_github_connection_impl`'s mapping.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct GitHubTestResult {
    pub ok: bool,
    pub message: String,
}

/// Probe the configured repo without publishing anything: a single
/// `get_file` on the configured snapshot path. Both `Ok(Some(..))` and
/// `Ok(None)` count as connected — a repo that simply doesn't have the
/// snapshot file yet (first publish still ahead) is a perfectly valid
/// target. Missing config OR missing token short-circuit to
/// `"not configured"` with no network I/O at all (a credential-store
/// backend failure reads the same way — best-effort probe, mirroring
/// `get_digikey_status`'s reasoning). The remaining branches map
/// `GitHubError` onto the fixed strings: `Auth` → "rejected — check
/// token", `NotFound` → "repo or branch not found" (unreachable via the
/// real client's `get_file`, which folds 404 into `Ok(None)`, but kept for
/// any `GitHubApi` impl that can distinguish), everything else
/// (`Network`/`RateLimited`/`Conflict`/`Api`) → "network error or timeout".
pub fn test_github_connection_impl(state: &AppState) -> Result<GitHubTestResult, CommandError> {
    // Scope the DB lock to the config read: the probe below is network I/O
    // and must not hold the database mutex.
    let config = {
        let db = lock(state)?;
        PublishConfig::load(&db).map_err(CommandError::from)?
    };
    let Some(config) = config else {
        return Ok(GitHubTestResult {
            ok: false,
            message: "not configured".to_string(),
        });
    };
    let Ok(Some(token)) = inventory_core::secrets::load_github_token() else {
        return Ok(GitHubTestResult {
            ok: false,
            message: "not configured".to_string(),
        });
    };

    let api = github_api(token);
    let (ok, message) = match api.get_file(&config.repo_ref(), &config.path) {
        Ok(_) => (true, "connected"),
        Err(GitHubError::Auth) => (false, "rejected — check token"),
        Err(GitHubError::NotFound) => (false, "repo or branch not found"),
        Err(_) => (false, "network error or timeout"),
    };
    Ok(GitHubTestResult {
        ok,
        message: message.to_string(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn test_github_connection(
    state: State<'_, AppState>,
) -> Result<GitHubTestResult, CommandError> {
    test_github_connection_impl(&state)
}

/// IPC mirror of `inventory_sync::publish::PublishOutcome` (that crate has
/// no specta dependency): `status` is `"published"` or `"unchanged"`, and
/// `digest` accompanies a publish that actually uploaded bytes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct PublishOutcomeDto {
    pub status: String,
    pub digest: Option<String>,
}

impl From<PublishOutcome> for PublishOutcomeDto {
    fn from(outcome: PublishOutcome) -> Self {
        match outcome {
            PublishOutcome::Published { digest } => PublishOutcomeDto {
                status: "published".to_string(),
                digest: Some(digest),
            },
            PublishOutcome::Unchanged => PublishOutcomeDto {
                status: "unchanged".to_string(),
                digest: None,
            },
        }
    }
}

/// Publish now (Settings button / close flow): config and token are
/// checked up front — in that order, so an entirely-unconfigured state
/// reads as `publish_not_configured` rather than a token complaint — then
/// `publish_snapshot` drives the digest check + get/put against the live
/// client. Failures after the digest check have already set the
/// pending-publish marker by the time the typed error reaches the caller
/// (see `inventory_sync::publish`).
pub fn publish_now_impl(state: &AppState) -> Result<PublishOutcomeDto, CommandError> {
    let mut db = lock(state)?;
    if PublishConfig::load(&db)
        .map_err(CommandError::from)?
        .is_none()
    {
        return Err(SyncError::NotConfigured.into());
    }
    let api = github_api(require_github_token()?);
    let outcome =
        inventory_sync::publish::publish_snapshot(&mut db, &api).map_err(CommandError::from)?;
    Ok(outcome.into())
}

#[tauri::command]
#[specta::specta]
pub fn publish_now(state: State<'_, AppState>) -> Result<PublishOutcomeDto, CommandError> {
    publish_now_impl(&state)
}

/// The QUIET retry path (AppShell startup, per plan Task 6): `Ok(None)`
/// whenever there is nothing this call could sensibly do — no pending
/// marker, publishing not configured, or no token stored (including a
/// credential-backend failure) — so the caller never has to special-case
/// "retry wasn't applicable" as an error. Only an actually-attempted
/// publish can fail, and that failure comes back typed (with the pending
/// marker re-set by `publish_snapshot`) for the caller to stay quiet about.
pub fn retry_pending_publish_impl(
    state: &AppState,
) -> Result<Option<PublishOutcomeDto>, CommandError> {
    let mut db = lock(state)?;
    if db.get_app_state(PENDING_PUBLISH_KEY)?.is_none() {
        return Ok(None);
    }
    if PublishConfig::load(&db)
        .map_err(CommandError::from)?
        .is_none()
    {
        return Ok(None);
    }
    let Ok(Some(token)) = inventory_core::secrets::load_github_token() else {
        return Ok(None);
    };
    let api = github_api(token);
    let outcome =
        inventory_sync::publish::publish_snapshot(&mut db, &api).map_err(CommandError::from)?;
    Ok(Some(outcome.into()))
}

#[tauri::command]
#[specta::specta]
pub fn retry_pending_publish(
    state: State<'_, AppState>,
) -> Result<Option<PublishOutcomeDto>, CommandError> {
    retry_pending_publish_impl(&state)
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
// History (Phase 3 Task 9)
// ---------------------------------------------------------------------

pub fn list_history_impl(
    state: &AppState,
    filter: HistoryFilter,
) -> Result<HistoryPage, CommandError> {
    Ok(lock(state)?.list_history(&filter)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_history(
    state: State<'_, AppState>,
    filter: HistoryFilter,
) -> Result<HistoryPage, CommandError> {
    list_history_impl(&state, filter)
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
// Imports (Phase 5b Task 5, spec §10 Upload -> Extract -> Match -> Review ->
// Confirm). Every command here is a thin wrapper over the already-atomic
// `inventory_db` methods (Tasks 1-4): `commit_import`/`reverse_import` are
// the ONLY mutations in the pipeline, both already one atomic transaction —
// nothing extra to coordinate at this layer.
// ---------------------------------------------------------------------

/// Detect `filename`/`bytes`' format and hand off to the matching DigiKey
/// parser. CSV and XLSX always work; PDF needs a `PdfTextSource`
/// implementation, which only exists when this crate is built with the
/// `pdfium` Cargo feature (off by default — see `Cargo.toml`). Never
/// panics: an undetectable format or a parse failure both come back as a
/// typed `CommandError`.
fn parse_invoice(
    format: inventory_import::SourceFormat,
    bytes: &[u8],
) -> Result<inventory_import::ParsedInvoice, CommandError> {
    use inventory_import::digikey::{DigiKeyCsvParser, DigiKeyXlsxParser};
    use inventory_import::{InvoiceParser, SourceFormat};

    match format {
        SourceFormat::Csv => Ok(DigiKeyCsvParser.parse(bytes)?),
        SourceFormat::Xlsx => Ok(DigiKeyXlsxParser.parse(bytes)?),
        SourceFormat::Pdf => parse_pdf(bytes),
    }
}

/// The `pdfium`-feature-enabled PDF path: load `pdfium.dll`/`libpdfium.so`
/// at runtime (`PdfiumTextSource::new`, itself a typed error if the library
/// isn't actually present on disk) and run it through `DigiKeyPdfParser`.
#[cfg(feature = "pdfium")]
fn parse_pdf(bytes: &[u8]) -> Result<inventory_import::ParsedInvoice, CommandError> {
    use inventory_import::digikey::DigiKeyPdfParser;
    use inventory_import::{InvoiceParser, PdfiumTextSource};

    let source = PdfiumTextSource::new()?;
    Ok(DigiKeyPdfParser::new(source).parse(bytes)?)
}

/// PDF import is wired end-to-end (`DigiKeyPdfParser` + the token/row/column
/// reconstruction, Phase 5a) but needs a real `pdfium.dll`/`libpdfium.so` at
/// runtime, loaded through the `pdfium` Cargo feature — off by default,
/// since the desktop build ships without that native library (see
/// `Cargo.toml`, `docs/build.md`). Without the feature there is no
/// `PdfTextSource` implementation to construct at all, so this build
/// rejects a PDF upload with a clear, typed message instead of failing to
/// compile or panicking. CSV and XLSX (DigiKey's other two export formats)
/// are completely unaffected and work fully either way.
#[cfg(not(feature = "pdfium"))]
fn parse_pdf(_bytes: &[u8]) -> Result<inventory_import::ParsedInvoice, CommandError> {
    Err(ImportError::Pdf(
        "PDF import requires pdfium (not available in this build); use CSV/XLSX instead"
            .to_string(),
    )
    .into())
}

pub fn parse_and_store_import_impl(
    state: &AppState,
    bytes: Vec<u8>,
    filename: String,
) -> Result<ImportRecord, CommandError> {
    let format =
        inventory_import::detect_format(&filename, &bytes).ok_or_else(|| CommandError {
            code: "unsupported_import_format".to_string(),
            message: format!(
                "could not detect a supported import format (csv/xlsx/pdf) for '{filename}'"
            ),
        })?;
    let parsed = parse_invoice(format, &bytes)?;

    // Read the attachments dir off the (separate) layout field before
    // locking the DB, so there's no borrow overlap with the guard (same
    // pattern as `add_attachment_impl`).
    let dir = state.layout.attachments.clone();
    Ok(lock(state)?.store_import(&dir, &parsed, &bytes, &filename)?)
}

/// Upload -> Extract: detect the file's format, parse it with the matching
/// DigiKey parser, and persist the result (`store_import`). The bytes cross
/// the IPC boundary as a JSON number array (`Vec<u8>` -> `number[]`), same
/// convention as `add_attachment`. Purely additive against inventory —
/// nothing here creates a part or a receive; that's `commit_import` below.
#[tauri::command]
#[specta::specta]
pub fn parse_and_store_import(
    state: State<'_, AppState>,
    bytes: Vec<u8>,
    filename: String,
) -> Result<ImportRecord, CommandError> {
    parse_and_store_import_impl(&state, bytes, filename)
}

pub fn get_import_review_impl(
    state: &AppState,
    import_id: ImportId,
) -> Result<ImportReview, CommandError> {
    Ok(lock(state)?.build_import_review(&import_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn get_import_review(
    state: State<'_, AppState>,
    import_id: ImportId,
) -> Result<ImportReview, CommandError> {
    get_import_review_impl(&state, import_id)
}

pub fn list_imports_impl(state: &AppState) -> Result<Vec<ImportRecord>, CommandError> {
    Ok(lock(state)?.list_imports()?)
}

#[tauri::command]
#[specta::specta]
pub fn list_imports(state: State<'_, AppState>) -> Result<Vec<ImportRecord>, CommandError> {
    list_imports_impl(&state)
}

pub fn list_import_lines_impl(
    state: &AppState,
    import_id: ImportId,
) -> Result<Vec<ImportLineRecord>, CommandError> {
    Ok(lock(state)?.list_import_lines(&import_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn list_import_lines(
    state: State<'_, AppState>,
    import_id: ImportId,
) -> Result<Vec<ImportLineRecord>, CommandError> {
    list_import_lines_impl(&state, import_id)
}

pub fn commit_import_impl(
    state: &AppState,
    import_id: ImportId,
    decisions: Vec<(ImportLineId, LineDecision)>,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.commit_import(&import_id, &decisions)?)
}

/// Confirm: apply every resolved per-line decision as ONE atomic group
/// (`commit_import`) — new parts/variants/listings, each line's
/// shipped-quantity receive, price history, and remembered SKU/MPN
/// aliases. The only mutation in the Match -> Review -> Commit pipeline;
/// any failure rolls back the entire commit and the import stays `parsed`.
#[tauri::command]
#[specta::specta]
pub fn commit_import(
    state: State<'_, AppState>,
    import_id: ImportId,
    decisions: Vec<(ImportLineId, LineDecision)>,
) -> Result<GroupRecord, CommandError> {
    commit_import_impl(&state, import_id, decisions)
}

pub fn reverse_import_impl(
    state: &AppState,
    import_id: ImportId,
    note: String,
) -> Result<GroupRecord, CommandError> {
    Ok(lock(state)?.reverse_import(&import_id, &note)?)
}

/// Undo a committed import's receive group and flip its status back to
/// `reversed` (`reverse_import`). Parts the commit created are NOT
/// deleted — they simply return to zero stock (history is never deleted).
#[tauri::command]
#[specta::specta]
pub fn reverse_import(
    state: State<'_, AppState>,
    import_id: ImportId,
    note: String,
) -> Result<GroupRecord, CommandError> {
    reverse_import_impl(&state, import_id, note)
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
            create_project_full,
            list_projects_full,
            get_project,
            update_project,
            set_project_status,
            duplicate_project,
            archive_project,
            add_bom_item,
            update_bom_item,
            remove_bom_item,
            set_bom_substitutes,
            get_bom_item,
            list_bom,
            import_bom,
            reserve_bom,
            release_bom_reservations,
            plan_build,
            build_from_bom,
            associate_checkout,
            set_attribute,
            get_attributes,
            clear_attribute,
            add_dimension,
            list_dimensions,
            remove_dimension,
            add_attachment,
            attach_to_part,
            list_part_attachments,
            read_attachment,
            remove_part_attachment,
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
            list_history,
            list_bins,
            rename_bin,
            get_setting,
            set_setting,
            enrich_part_preview,
            apply_enrichment,
            get_digikey_status,
            set_digikey_environment,
            set_digikey_credentials,
            clear_digikey_credentials,
            test_digikey_connection,
            get_publish_status,
            set_publish_config,
            set_github_token,
            clear_github_token,
            test_github_connection,
            publish_now,
            retry_pending_publish,
            parse_and_store_import,
            get_import_review,
            list_imports,
            list_import_lines,
            commit_import,
            reverse_import,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use inventory_core::quantity::{Quantity, QuantityUnit};
    use std::sync::{Mutex, MutexGuard, Once, OnceLock};

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

    fn project_draft(name: &str) -> ProjectDraft {
        ProjectDraft {
            name: name.to_string(),
            description: "A test project".to_string(),
            build_quantity: 1,
            repo_link: None,
            notes: String::new(),
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
    fn list_history_command_round_trips_filters_and_paging() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("History command part")).unwrap();
        apply_ledger_op_impl(
            &state,
            LedgerOp::Receive {
                part_id: part.id.clone(),
                quantity: Quantity::from_whole(5).unwrap(),
                note: "initial".to_string(),
            },
        )
        .unwrap();

        let filter = inventory_db::history::HistoryFilter {
            date_from: None,
            date_to: None,
            txn_type: Some("receive".to_string()),
            part_id: Some(part.id.clone()),
            project_id: None,
            group_id: None,
            limit: 10,
            offset: 0,
        };
        let page = list_history_impl(&state, filter).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].part_id, part.id);
        assert_eq!(page.rows[0].display_name, "History command part");
        assert!(page.rows[0].reversible);
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
    fn attachment_commands_store_link_list_and_dedupe() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("Part with a datasheet")).unwrap();

        let stored = add_attachment_impl(
            &state,
            b"PDF bytes".to_vec(),
            Some("pdf".to_string()),
            AttachmentKind::Datasheet,
            Some("ds.pdf".to_string()),
            "upload".to_string(),
        )
        .unwrap();
        attach_to_part_impl(&state, part.id.clone(), stored.content_hash.clone()).unwrap();

        let listed = list_part_attachments_impl(&state, part.id.clone()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].content_hash, stored.content_hash);
        assert_eq!(listed[0].kind, AttachmentKind::Datasheet);

        // read_attachment round-trips the exact bytes.
        let bytes = read_attachment_impl(&state, stored.content_hash.clone()).unwrap();
        assert_eq!(bytes, b"PDF bytes");

        // Storing the identical bytes again returns the same hash (dedup) and
        // leaves exactly one file in the attachments dir.
        let again = add_attachment_impl(
            &state,
            b"PDF bytes".to_vec(),
            Some("pdf".to_string()),
            AttachmentKind::Datasheet,
            Some("ds.pdf".to_string()),
            "upload".to_string(),
        )
        .unwrap();
        assert_eq!(again.content_hash, stored.content_hash);
        let file_count = std::fs::read_dir(&state.layout.attachments)
            .unwrap()
            .count();
        assert_eq!(file_count, 1, "identical bytes must dedupe to one file");

        // Unlinking removes the link but not the blob.
        remove_part_attachment_impl(&state, part.id.clone(), stored.content_hash.clone()).unwrap();
        assert!(list_part_attachments_impl(&state, part.id)
            .unwrap()
            .is_empty());
        assert!(read_attachment_impl(&state, stored.content_hash).is_ok());
    }

    /// A caller invoking `add_attachment` directly (bypassing the frontend's
    /// well-formed `File.name`) with a traversal payload in `ext` must be
    /// rejected with a typed `invalid_attachment` error, and must not write
    /// anything outside (or even inside) the attachments directory.
    #[test]
    fn add_attachment_command_rejects_a_traversal_ext() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let err = add_attachment_impl(
            &state,
            b"malicious payload".to_vec(),
            Some("../../evil".to_string()),
            AttachmentKind::Other,
            None,
            "upload".to_string(),
        )
        .unwrap_err();

        assert_eq!(err.code, "invalid_attachment");
        assert!(!err.message.contains("InvalidAttachment("));

        // Nothing landed in the attachments dir (it may not even exist yet).
        let count = std::fs::read_dir(&state.layout.attachments)
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(count, 0, "a rejected ext must not write any file");
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
    fn create_project_full_command_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let created = create_project_full_impl(&state, project_draft("Blinky Board")).unwrap();
        assert_eq!(created.name, "Blinky Board");
        assert_eq!(created.status, ProjectStatus::Planned);
        assert_eq!(created.build_quantity, 1);
        assert!(created.completed_at.is_none());

        let fetched = get_project_impl(&state, created.id.clone())
            .unwrap()
            .unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, "Blinky Board");
        assert_eq!(fetched.status, ProjectStatus::Planned);
    }

    #[test]
    fn list_projects_full_command_filters_by_status() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let planned = create_project_full_impl(&state, project_draft("Planned Project")).unwrap();
        let to_activate =
            create_project_full_impl(&state, project_draft("Active Project")).unwrap();
        set_project_status_impl(&state, to_activate.id.clone(), ProjectStatus::Active).unwrap();

        let all = list_projects_full_impl(&state, None).unwrap();
        assert_eq!(all.len(), 2);

        let planned_only = list_projects_full_impl(&state, Some(ProjectStatus::Planned)).unwrap();
        assert_eq!(planned_only.len(), 1);
        assert_eq!(planned_only[0].id, planned.id);

        let active_only = list_projects_full_impl(&state, Some(ProjectStatus::Active)).unwrap();
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].id, to_activate.id);
    }

    #[test]
    fn add_bom_item_and_list_bom_command_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let project = create_project_full_impl(&state, project_draft("Blinky Board")).unwrap();
        let part = create_part_impl(&state, part_draft("10k resistor")).unwrap();

        let draft = BomItemDraft {
            part_id: part.id.clone(),
            quantity_per_build: Quantity::from_whole(4).unwrap(),
            reference_designators: "R1,R2,R3,R4".to_string(),
            required: true,
            notes: String::new(),
        };
        let added = add_bom_item_impl(&state, project.id.clone(), draft).unwrap();
        assert_eq!(added.part_id, part.id);
        assert_eq!(added.total_required, Quantity::from_whole(4).unwrap());

        let items = list_bom_impl(&state, project.id.clone()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, added.id);

        remove_bom_item_impl(&state, added.id).unwrap();
        assert!(list_bom_impl(&state, project.id).unwrap().is_empty());
    }

    #[test]
    fn build_from_bom_command_maps_empty_group_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let project = create_project_full_impl(&state, project_draft("Blinky Board")).unwrap();

        // No BOM items at all, so build_from_bom's derived op list is empty
        // — the thin command wrapper must surface apply_group's typed
        // EmptyGroup error (Display text, never Debug) rather than
        // panicking or leaking a raw DbError.
        let err = build_from_bom_impl(&state, project.id, Vec::new()).unwrap_err();
        assert_eq!(err.code, "empty_group");
        assert!(!err.message.contains("EmptyGroup"));
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

    #[test]
    fn enrich_part_preview_and_apply_enrichment_round_trip_through_the_command_layer() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let mut draft = part_draft("loose resistor");
        draft.description = "RES 10K OHM 1% 1/4W 0603".to_string();
        let part = create_part_impl(&state, draft).unwrap();

        // No variant at all, so the DigiKey provider's identity check finds
        // no mpn/supplier_sku and contributes nothing before ever touching
        // the credential store — this is hermetic regardless of what (if
        // anything) is in the machine's real OS keyring. The offline
        // description parser still proposes attributes from the free-text
        // description.
        let diff = enrich_part_preview_impl(&state, part.id.clone()).unwrap();
        assert_eq!(diff.part_id, part.id);
        let resistance = diff
            .diffs
            .iter()
            .find(|d| d.key == "attr.resistance")
            .expect("description parser should have proposed a resistance candidate");
        assert_eq!(resistance.proposed, "10K");

        apply_enrichment_impl(
            &state,
            part.id.clone(),
            vec![inventory_db::enrichment::AppliedField {
                key: resistance.key.clone(),
                value: resistance.proposed.clone(),
                source: resistance.source.clone(),
                acknowledge_review: false,
            }],
        )
        .unwrap();

        let attrs = get_attributes_impl(&state, part.id).unwrap();
        assert!(attrs
            .iter()
            .any(|(k, text, _)| k == "resistance" && text == "10K"));
    }

    #[test]
    fn apply_enrichment_maps_an_invalid_source_to_a_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let part = create_part_impl(&state, part_draft("a part")).unwrap();
        let err = apply_enrichment_impl(
            &state,
            part.id,
            vec![inventory_db::enrichment::AppliedField {
                key: "variant.datasheet_url".to_string(),
                value: "https://example.com/ds.pdf".to_string(),
                source: "not_a_real_source".to_string(),
                acknowledge_review: false,
            }],
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_enrichment_source");
    }

    // ---------------------------------------------------------------------
    // DigiKey credentials (Phase 5d Task 1): a hand-rolled, process-global
    // mock credential store installed as `keyring`'s default backend for
    // this test binary. `keyring`'s own built-in `mock` module does NOT
    // persist across separately-constructed `Entry`s with the same
    // service/user (every function under `inventory_core::secrets` opens a
    // fresh `Entry` per call) — `inventory_core::secrets`'s own test module
    // works around this with an identical hand-rolled mock, but that one is
    // `#[cfg(test)]`-private to the `inventory-core` crate's own test
    // binary and not visible here, so it is reproduced rather than shared.
    //
    // Installing this makes EVERY test in this file that touches DigiKey
    // credentials hermetic — including
    // `digikey_status_defaults_to_sandbox_and_unconfigured_then_tracks_the_environment_setting`
    // below, which now runs against this mock instead of the real OS
    // credential store — so no test in this binary ever reads, writes, or
    // clears a real stored credential.
    struct DigiKeyMockCredential {
        key: (String, String),
    }

    static DIGIKEY_MOCK_STORE: OnceLock<
        Mutex<std::collections::HashMap<(String, String), String>>,
    > = OnceLock::new();

    fn digikey_mock_store() -> &'static Mutex<std::collections::HashMap<(String, String), String>> {
        DIGIKEY_MOCK_STORE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
    }

    impl keyring::credential::CredentialApi for DigiKeyMockCredential {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            let value = String::from_utf8(secret.to_vec())
                .map_err(|e| keyring::Error::BadEncoding(e.into_bytes()))?;
            digikey_mock_store()
                .lock()
                .unwrap()
                .insert(self.key.clone(), value);
            Ok(())
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            digikey_mock_store()
                .lock()
                .unwrap()
                .get(&self.key)
                .map(|v| v.clone().into_bytes())
                .ok_or(keyring::Error::NoEntry)
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            let mut guard = digikey_mock_store().lock().unwrap();
            if guard.remove(&self.key).is_some() {
                Ok(())
            } else {
                Err(keyring::Error::NoEntry)
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct DigiKeyMockBuilder;

    impl keyring::credential::CredentialBuilderApi for DigiKeyMockBuilder {
        fn build(
            &self,
            _target: Option<&str>,
            service: &str,
            user: &str,
        ) -> keyring::Result<Box<keyring::credential::Credential>> {
            Ok(Box::new(DigiKeyMockCredential {
                key: (service.to_string(), user.to_string()),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn persistence(&self) -> keyring::credential::CredentialPersistence {
            keyring::credential::CredentialPersistence::ProcessOnly
        }
    }

    static DIGIKEY_MOCK_INSTALL: Once = Once::new();
    static DIGIKEY_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Install the mock credential backend (once, process-wide) and
    /// serialize every test that touches DigiKey credentials against every
    /// other one — the mock store is a single shared `HashMap`, so
    /// concurrent `cargo test` threads must not interleave. Clears the
    /// store before returning so each test starts from a known-empty ("not
    /// configured") state regardless of run order.
    fn digikey_credentials_test_lock() -> MutexGuard<'static, ()> {
        DIGIKEY_MOCK_INSTALL.call_once(|| {
            keyring::set_default_credential_builder(Box::new(DigiKeyMockBuilder));
        });
        let guard = DIGIKEY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        digikey_mock_store().lock().unwrap().clear();
        guard
    }

    #[test]
    fn digikey_status_defaults_to_sandbox_and_unconfigured_then_tracks_the_environment_setting() {
        let _guard = digikey_credentials_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        let status = get_digikey_status_impl(&state).unwrap();
        assert_eq!(status.environment, "sandbox");
        // `configured` reads the credential store (not per-data-dir) —
        // `digikey_credentials_test_lock()` installs and clears the
        // process-mock backend before this runs, so it deterministically
        // starts "not configured" regardless of what, if anything, is
        // stored in the real OS credential store on the machine running
        // this test.
        assert!(!status.configured);

        set_digikey_environment_impl(&state, "production".to_string()).unwrap();
        let status = get_digikey_status_impl(&state).unwrap();
        assert_eq!(status.environment, "production");

        let err = set_digikey_environment_impl(&state, "prod".to_string()).unwrap_err();
        assert_eq!(err.code, "invalid_digikey_environment");
        // The rejected value must not have overwritten the last valid one.
        let status = get_digikey_status_impl(&state).unwrap();
        assert_eq!(status.environment, "production");
    }

    /// `get_digikey_status`'s response crosses the IPC boundary to the
    /// webview — this asserts, at the type level, that no plausible secret
    /// field name can ever appear in its serialized form (spec §16: the
    /// DigiKey client id/secret must never leave the OS credential store /
    /// process memory).
    #[test]
    fn digikey_status_json_never_carries_a_secret_looking_field() {
        let status = DigiKeyStatus {
            configured: true,
            environment: "production".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        for marker in ["client_id", "client_secret", "secret", "token", "password"] {
            assert!(
                !json.to_lowercase().contains(marker),
                "serialized DigiKeyStatus must never contain '{marker}': {json}"
            );
        }
    }

    #[test]
    fn set_then_get_status_then_clear_digikey_credentials_round_trips() {
        let _guard = digikey_credentials_test_lock();

        set_digikey_credentials_impl(
            "test-client-id".to_string(),
            "test-client-secret".to_string(),
        )
        .expect("set should succeed");
        assert!(matches!(
            inventory_core::secrets::load_digikey_credentials(),
            Ok(Some(_))
        ));

        clear_digikey_credentials_impl().expect("clear should succeed");
        assert!(matches!(
            inventory_core::secrets::load_digikey_credentials(),
            Ok(None)
        ));

        // Idempotent: clearing an already-empty store is still Ok.
        clear_digikey_credentials_impl().expect("clearing twice should still succeed");
    }

    #[test]
    fn set_digikey_credentials_rejects_blank_or_whitespace_only_values() {
        let _guard = digikey_credentials_test_lock();

        let err = set_digikey_credentials_impl(String::new(), "test-client-secret".to_string())
            .unwrap_err();
        assert_eq!(err.code, "invalid_credentials");

        let err = set_digikey_credentials_impl("test-client-id".to_string(), "   ".to_string())
            .unwrap_err();
        assert_eq!(err.code, "invalid_credentials");

        // Nothing was stored by either rejected attempt.
        assert!(matches!(
            inventory_core::secrets::load_digikey_credentials(),
            Ok(None)
        ));
    }

    #[test]
    fn set_digikey_credentials_trims_surrounding_whitespace() {
        let _guard = digikey_credentials_test_lock();

        set_digikey_credentials_impl(
            "  test-client-id  ".to_string(),
            "  test-client-secret  ".to_string(),
        )
        .expect("set should succeed");

        let loaded = inventory_core::secrets::load_digikey_credentials()
            .unwrap()
            .unwrap();
        assert_eq!(loaded.client_id, "test-client-id");
        assert_eq!(loaded.client_secret, "test-client-secret");
    }

    #[test]
    fn test_digikey_connection_with_no_credentials_reports_not_configured_without_network() {
        let _guard = digikey_credentials_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        // No credentials stored (the mock store was just cleared) — this
        // must short-circuit before any request, which is what makes this
        // test hermetic: there is no live DigiKey endpoint reachable from
        // the test gate.
        let result = test_digikey_connection_impl(&state).unwrap();
        assert!(!result.ok);
        assert_eq!(result.environment, "sandbox");
        assert_eq!(result.message, "not configured");
    }

    #[test]
    fn test_digikey_connection_reflects_the_configured_environment_when_not_configured() {
        let _guard = digikey_credentials_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };

        set_digikey_environment_impl(&state, "production".to_string()).unwrap();
        let result = test_digikey_connection_impl(&state).unwrap();
        assert_eq!(result.environment, "production");
        assert_eq!(result.message, "not configured");
    }

    /// `DigiKeyTestResult`'s response crosses the IPC boundary to the
    /// webview — same secret-shaped-field guard as
    /// `digikey_status_json_never_carries_a_secret_looking_field`, covering
    /// every fixed `message` string this command can produce (not just the
    /// one a particular test run happens to hit).
    #[test]
    fn digikey_test_result_json_never_carries_a_secret_looking_field() {
        for message in [
            "not configured",
            "connected",
            "rejected — check credentials and environment",
            "network error or timeout",
        ] {
            let result = DigiKeyTestResult {
                ok: message == "connected",
                environment: "production".to_string(),
                message: message.to_string(),
            };
            let json = serde_json::to_string(&result).unwrap();
            for marker in ["client_id", "client_secret", "secret", "token", "password"] {
                assert!(
                    !json.to_lowercase().contains(marker),
                    "serialized DigiKeyTestResult must never contain '{marker}': {json}"
                );
            }
        }
    }

    // -----------------------------------------------------------------
    // Publishing (Phase 6 Task 4). These cover exactly the pre-network
    // command surface: config/status round-trips, validation, token
    // plumbing (against the same process-mock keyring the DigiKey tests
    // install — one global builder covers every service), and the typed
    // config/token short-circuits of `publish_now`/`retry_pending_publish`
    // /`test_github_connection`. Everything from the digest check onward
    // is exercised hermetically at the `inventory_sync::publish` level
    // (`crates/inventory-sync/tests/publish.rs`) through the `GitHubApi`
    // mock; no test here ever constructs the live client.
    // -----------------------------------------------------------------

    fn publish_test_state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        let init = crate::app::AppInit::initialize(Some(root.to_str().unwrap()), None).unwrap();
        let state = AppState {
            layout: init.layout,
            db: Mutex::new(init.db),
        };
        (dir, state)
    }

    #[test]
    fn publish_status_defaults_to_unconfigured_with_nothing_pending() {
        let (_dir, state) = publish_test_state();
        let status = get_publish_status_impl(&state).unwrap();
        assert!(!status.configured);
        assert_eq!(status.repo, None);
        assert_eq!(status.last_published_at, None);
        assert!(!status.pending);
        assert_eq!(status.vercel_url, None);
    }

    #[test]
    fn set_publish_config_trims_applies_defaults_and_shows_up_in_status() {
        let (_dir, state) = publish_test_state();
        set_publish_config_impl(
            &state,
            "  jacob  ".to_string(),
            " bench-ledger-public ".to_string(),
            "   ".to_string(),
            String::new(),
            Some("  https://bench.vercel.app  ".to_string()),
        )
        .unwrap();

        let status = get_publish_status_impl(&state).unwrap();
        assert!(status.configured);
        assert_eq!(status.repo.as_deref(), Some("jacob/bench-ledger-public"));
        assert_eq!(
            status.vercel_url.as_deref(),
            Some("https://bench.vercel.app")
        );

        // Blank branch/path fell back to the documented defaults in the
        // stored settings themselves (not just at load time).
        let db = lock(&state).unwrap();
        assert_eq!(
            db.get_setting(BRANCH_SETTING).unwrap().as_deref(),
            Some(DEFAULT_BRANCH)
        );
        assert_eq!(
            db.get_setting(PATH_SETTING).unwrap().as_deref(),
            Some(DEFAULT_PATH)
        );
    }

    #[test]
    fn set_publish_config_rejects_blank_owner_or_repo_without_storing_anything() {
        let (_dir, state) = publish_test_state();
        let err = set_publish_config_impl(
            &state,
            "   ".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_publish_config");

        let err = set_publish_config_impl(
            &state,
            "owner".to_string(),
            String::new(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_publish_config");

        assert!(!get_publish_status_impl(&state).unwrap().configured);
    }

    #[test]
    fn clearing_the_vercel_url_reads_back_as_none() {
        let (_dir, state) = publish_test_state();
        set_publish_config_impl(
            &state,
            "jacob".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            Some("https://bench.vercel.app".to_string()),
        )
        .unwrap();
        // Re-saving without a URL stores "" — which loads as None.
        set_publish_config_impl(
            &state,
            "jacob".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap();
        assert_eq!(get_publish_status_impl(&state).unwrap().vercel_url, None);
    }

    #[test]
    fn publish_status_reflects_pending_marker_and_last_published_at() {
        let (_dir, state) = publish_test_state();
        {
            let mut db = lock(&state).unwrap();
            db.set_app_state(PENDING_PUBLISH_KEY, "1").unwrap();
            db.set_app_state(LAST_PUBLISHED_AT_KEY, "2026-07-19T10:00:00Z")
                .unwrap();
        }
        let status = get_publish_status_impl(&state).unwrap();
        assert!(status.pending);
        assert_eq!(
            status.last_published_at.as_deref(),
            Some("2026-07-19T10:00:00Z")
        );
    }

    /// `PublishStatus` crosses the IPC boundary to the webview; like the
    /// DigiKey status/test-result guards above, its serialized JSON must
    /// never contain anything token-shaped — the shape simply has no field
    /// that could carry one, and this pins that.
    #[test]
    fn publish_status_json_never_carries_a_token_looking_field() {
        let status = PublishStatus {
            configured: true,
            repo: Some("jacob/bench-ledger-public".to_string()),
            last_published_at: Some("2026-07-19T10:00:00Z".to_string()),
            pending: false,
            vercel_url: Some("https://bench.vercel.app".to_string()),
        };
        let json = serde_json::to_string(&status).unwrap();
        for marker in ["token", "secret", "credential", "authorization"] {
            assert!(
                !json.to_lowercase().contains(marker),
                "serialized PublishStatus must never contain '{marker}': {json}"
            );
        }
    }

    #[test]
    fn set_github_token_trims_stores_write_only_and_clear_is_idempotent() {
        let _guard = digikey_credentials_test_lock();
        set_github_token_impl("  fake-token-abc  ".to_string()).unwrap();
        let loaded = inventory_core::secrets::load_github_token()
            .unwrap()
            .expect("token should be stored");
        assert_eq!(loaded.expose(), "fake-token-abc");

        clear_github_token_impl().unwrap();
        assert!(inventory_core::secrets::load_github_token()
            .unwrap()
            .is_none());
        clear_github_token_impl().expect("clearing when absent should still succeed");
    }

    #[test]
    fn set_github_token_rejects_blank_or_whitespace_only_values() {
        let _guard = digikey_credentials_test_lock();
        let err = set_github_token_impl(String::new()).unwrap_err();
        assert_eq!(err.code, "invalid_token");
        let err = set_github_token_impl("   ".to_string()).unwrap_err();
        assert_eq!(err.code, "invalid_token");
        assert!(inventory_core::secrets::load_github_token()
            .unwrap()
            .is_none());
    }

    #[test]
    fn publish_now_without_config_errors_publish_not_configured() {
        let _guard = digikey_credentials_test_lock();
        let (_dir, state) = publish_test_state();
        let err = publish_now_impl(&state).unwrap_err();
        assert_eq!(err.code, "publish_not_configured");
    }

    #[test]
    fn publish_now_with_config_but_no_token_errors_github_token_missing() {
        let _guard = digikey_credentials_test_lock();
        let (_dir, state) = publish_test_state();
        set_publish_config_impl(
            &state,
            "jacob".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap();
        let err = publish_now_impl(&state).unwrap_err();
        assert_eq!(err.code, "github_token_missing");
        // A config/token short-circuit is not a failed publish attempt —
        // nothing may be marked pending.
        assert!(!get_publish_status_impl(&state).unwrap().pending);
    }

    #[test]
    fn retry_pending_publish_is_quietly_none_without_a_pending_marker() {
        let _guard = digikey_credentials_test_lock();
        let (_dir, state) = publish_test_state();
        assert_eq!(retry_pending_publish_impl(&state).unwrap(), None);
    }

    #[test]
    fn retry_pending_publish_is_quietly_none_when_pending_but_unconfigured_or_tokenless() {
        let _guard = digikey_credentials_test_lock();
        let (_dir, state) = publish_test_state();
        lock(&state)
            .unwrap()
            .set_app_state(PENDING_PUBLISH_KEY, "1")
            .unwrap();

        // Pending but no config: quiet None.
        assert_eq!(retry_pending_publish_impl(&state).unwrap(), None);

        // Pending + config but no token: still quiet None.
        set_publish_config_impl(
            &state,
            "jacob".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap();
        assert_eq!(retry_pending_publish_impl(&state).unwrap(), None);

        // The quiet path never cleared the marker — the retry never ran.
        assert!(get_publish_status_impl(&state).unwrap().pending);
    }

    #[test]
    fn test_github_connection_short_circuits_to_not_configured_without_network() {
        let _guard = digikey_credentials_test_lock();
        let (_dir, state) = publish_test_state();

        // No config (regardless of token state): "not configured", and no
        // client is ever constructed, so this is hermetic by construction.
        let result = test_github_connection_impl(&state).unwrap();
        assert!(!result.ok);
        assert_eq!(result.message, "not configured");

        // Config but no token: same fixed string, still no network.
        set_publish_config_impl(
            &state,
            "jacob".to_string(),
            "repo".to_string(),
            String::new(),
            String::new(),
            None,
        )
        .unwrap();
        let result = test_github_connection_impl(&state).unwrap();
        assert!(!result.ok);
        assert_eq!(result.message, "not configured");
    }

    /// `GitHubTestResult` crosses the IPC boundary; every message is one of
    /// the five fixed strings and the shape has no field that could carry a
    /// token — pinned the same way as the DigiKey test-result guard.
    #[test]
    fn github_test_result_json_never_carries_a_secret_looking_field() {
        for message in [
            "not configured",
            "connected",
            "rejected — check token",
            "repo or branch not found",
            "network error or timeout",
        ] {
            let result = GitHubTestResult {
                ok: message == "connected",
                message: message.to_string(),
            };
            let json = serde_json::to_string(&result).unwrap();
            for marker in ["secret", "credential", "authorization", "bearer"] {
                assert!(
                    !json.to_lowercase().contains(marker),
                    "serialized GitHubTestResult must never contain '{marker}': {json}"
                );
            }
        }
    }

    /// Every `SyncError` variant maps to its own stable `code` (and `Db`
    /// re-uses `DbError`'s mapping) — the frontend branches on these.
    #[test]
    fn sync_error_maps_to_stable_command_error_codes() {
        use inventory_sync::github::GitHubError;

        let cases: Vec<(CommandError, &str)> = vec![
            (SyncError::NotConfigured.into(), "publish_not_configured"),
            (SyncError::TokenMissing.into(), "github_token_missing"),
            (SyncError::GitHub(GitHubError::Auth).into(), "github_auth"),
            (
                SyncError::GitHub(GitHubError::NotFound).into(),
                "github_not_found",
            ),
            (
                SyncError::GitHub(GitHubError::Conflict).into(),
                "github_conflict",
            ),
            (
                SyncError::GitHub(GitHubError::RateLimited).into(),
                "github_rate_limited",
            ),
            (
                SyncError::GitHub(GitHubError::Network("network error or timeout".into())).into(),
                "github_network",
            ),
            (
                SyncError::GitHub(GitHubError::Api(502)).into(),
                "github_api",
            ),
            (
                SyncError::Db(DbError::PartNotFound).into(),
                "part_not_found",
            ),
        ];
        for (err, code) in cases {
            assert_eq!(err.code, code, "wrong code for message '{}'", err.message);
        }
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
