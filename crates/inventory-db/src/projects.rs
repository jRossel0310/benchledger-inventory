//! Projects repository: the project lifecycle (planned/active/completed/
//! archived) and duplication, including its BOM structure. The Phase 2a
//! stub `create_project`/`list_projects` (crates/inventory-db/src/ledger.rs)
//! stays in place for the quick-action pickers; this module adds the rich
//! CRUD + lifecycle surface Phase 4 needs.

use inventory_core::ids::{BomItemId, ProjectId};

use crate::{Database, DbError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Planned,
    Active,
    Completed,
    Archived,
}

impl ProjectStatus {
    pub fn as_sql(&self) -> &'static str {
        match self {
            ProjectStatus::Planned => "planned",
            ProjectStatus::Active => "active",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Archived => "archived",
        }
    }

    pub fn from_sql(s: &str) -> Option<Self> {
        Some(match s {
            "planned" => ProjectStatus::Planned,
            "active" => ProjectStatus::Active,
            "completed" => ProjectStatus::Completed,
            "archived" => ProjectStatus::Archived,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProjectDraft {
    pub name: String,
    pub description: String,
    pub build_quantity: i64,
    pub repo_link: Option<String>,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub name: String,
    pub status: ProjectStatus,
    pub description: String,
    pub build_quantity: i64,
    pub repo_link: Option<String>,
    pub notes: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

const PROJECT_COLUMNS: &str =
    "id, name, status, description, build_quantity, repo_link, notes, created_at, completed_at";

impl Database {
    pub fn create_project_full(&mut self, draft: &ProjectDraft) -> Result<ProjectRecord, DbError> {
        let id = ProjectId::new();
        self.raw_conn().execute(
            "INSERT INTO projects (id, name, status, description, build_quantity, repo_link, notes)
             VALUES (?1, ?2, 'planned', ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id.as_str(),
                draft.name,
                draft.description,
                draft.build_quantity,
                draft.repo_link,
                draft.notes,
            ],
        )?;
        self.get_project(&id)?.ok_or(DbError::ProjectNotFound)
    }

    pub fn get_project(&self, id: &ProjectId) -> Result<Option<ProjectRecord>, DbError> {
        let mut stmt = self.raw_conn().prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"
        ))?;
        let mut rows = stmt.query([id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_project(row)?)),
            None => Ok(None),
        }
    }

    /// Every project ordered by name, optionally narrowed to a single
    /// status. The Phase 2a `list_projects` (id + name only) remains for the
    /// quick-action pickers; this is the rich list the Projects screen uses.
    pub fn list_projects_full(
        &self,
        status_filter: Option<ProjectStatus>,
    ) -> Result<Vec<ProjectRecord>, DbError> {
        let mut out = Vec::new();
        match status_filter {
            Some(status) => {
                let mut stmt = self.raw_conn().prepare(&format!(
                    "SELECT {PROJECT_COLUMNS} FROM projects WHERE status = ?1 ORDER BY name"
                ))?;
                let mut rows = stmt.query([status.as_sql()])?;
                while let Some(row) = rows.next()? {
                    out.push(row_to_project(row)?);
                }
            }
            None => {
                let mut stmt = self.raw_conn().prepare(&format!(
                    "SELECT {PROJECT_COLUMNS} FROM projects ORDER BY name"
                ))?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    out.push(row_to_project(row)?);
                }
            }
        }
        Ok(out)
    }

    /// Persists name/description/build_quantity/repo_link/notes. Status has
    /// its own method (`set_project_status`) since it carries `completed_at`
    /// side effects this call must not trigger.
    pub fn update_project(&mut self, record: &ProjectRecord) -> Result<(), DbError> {
        let n = self.raw_conn().execute(
            "UPDATE projects SET name = ?2, description = ?3, build_quantity = ?4,
                                 repo_link = ?5, notes = ?6
             WHERE id = ?1",
            rusqlite::params![
                record.id.as_str(),
                record.name,
                record.description,
                record.build_quantity,
                record.repo_link,
                record.notes,
            ],
        )?;
        if n == 0 {
            return Err(DbError::ProjectNotFound);
        }
        Ok(())
    }

    /// Any status -> any status is allowed; `completed_at` is the only side
    /// effect. Entering `Completed` stamps it; leaving `Completed` for
    /// `Planned`/`Active` clears it; `Archived` never touches it (a project
    /// archived straight from Completed keeps its completion timestamp, and
    /// archiving a never-completed project leaves it NULL).
    pub fn set_project_status(
        &mut self,
        id: &ProjectId,
        status: ProjectStatus,
    ) -> Result<(), DbError> {
        let n = match status {
            ProjectStatus::Completed => self.raw_conn().execute(
                "UPDATE projects SET status = ?2, completed_at = datetime('now') WHERE id = ?1",
                rusqlite::params![id.as_str(), status.as_sql()],
            )?,
            ProjectStatus::Archived => self.raw_conn().execute(
                "UPDATE projects SET status = ?2 WHERE id = ?1",
                rusqlite::params![id.as_str(), status.as_sql()],
            )?,
            ProjectStatus::Planned | ProjectStatus::Active => self.raw_conn().execute(
                "UPDATE projects SET status = ?2, completed_at = NULL WHERE id = ?1",
                rusqlite::params![id.as_str(), status.as_sql()],
            )?,
        };
        if n == 0 {
            return Err(DbError::ProjectNotFound);
        }
        Ok(())
    }

    /// Copies the project (new id, status reset to `planned`, `completed_at`
    /// cleared, same description/build_quantity/repo_link/notes) plus its
    /// entire BOM (new `bom_items` ids, same part/qty/refdes/required/notes)
    /// and each BOM line's substitutes, all in one transaction. No ledger
    /// rows or stock are copied — the duplicate starts with an empty history.
    pub fn duplicate_project(
        &mut self,
        id: &ProjectId,
        new_name: &str,
    ) -> Result<ProjectRecord, DbError> {
        let tx = self.conn_mut().transaction()?;

        let (description, build_quantity, repo_link, notes): (String, i64, Option<String>, String) = {
            let mut stmt = tx.prepare(
                "SELECT description, build_quantity, repo_link, notes FROM projects WHERE id = ?1",
            )?;
            let mut rows = stmt.query([id.as_str()])?;
            match rows.next()? {
                Some(row) => (row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?),
                None => return Err(DbError::ProjectNotFound),
            }
        };

        let new_id = ProjectId::new();
        tx.execute(
            "INSERT INTO projects (id, name, status, description, build_quantity, repo_link, notes)
             VALUES (?1, ?2, 'planned', ?3, ?4, ?5, ?6)",
            rusqlite::params![
                new_id.as_str(),
                new_name,
                description,
                build_quantity,
                repo_link,
                notes,
            ],
        )?;

        type BomRow = (String, String, i64, String, i64, String);
        let bom_rows: Vec<BomRow> = {
            let mut stmt = tx.prepare(
                "SELECT id, part_id, quantity_per_build_milli, reference_designators, required, notes
                 FROM bom_items WHERE project_id = ?1",
            )?;
            let rows = stmt.query_map([id.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?;
            rows.collect::<Result<_, _>>()?
        };

        for (old_item_id, part_id, qty_milli, refdes, required, item_notes) in bom_rows {
            let new_item_id = BomItemId::new();
            tx.execute(
                "INSERT INTO bom_items (id, project_id, part_id, quantity_per_build_milli,
                                        reference_designators, required, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    new_item_id.as_str(),
                    new_id.as_str(),
                    part_id,
                    qty_milli,
                    refdes,
                    required,
                    item_notes,
                ],
            )?;

            let sub_parts: Vec<String> = {
                let mut stmt =
                    tx.prepare("SELECT part_id FROM bom_substitutes WHERE bom_item_id = ?1")?;
                let rows = stmt.query_map([old_item_id.as_str()], |r| r.get(0))?;
                rows.collect::<Result<_, _>>()?
            };
            for sub_part_id in sub_parts {
                tx.execute(
                    "INSERT INTO bom_substitutes (bom_item_id, part_id) VALUES (?1, ?2)",
                    rusqlite::params![new_item_id.as_str(), sub_part_id],
                )?;
            }
        }

        tx.commit()?;
        self.get_project(&new_id)?.ok_or(DbError::ProjectNotFound)
    }

    /// Thin wrapper: archiving is just a status transition.
    pub fn archive_project(&mut self, id: &ProjectId) -> Result<(), DbError> {
        self.set_project_status(id, ProjectStatus::Archived)
    }
}

fn row_to_project(row: &rusqlite::Row<'_>) -> Result<ProjectRecord, DbError> {
    let status_raw: String = row.get(2)?;
    let status = ProjectStatus::from_sql(&status_raw)
        .ok_or_else(|| DbError::Corrupt(format!("unknown project status '{status_raw}'")))?;
    Ok(ProjectRecord {
        id: ProjectId::from_string(row.get(0)?)
            .map_err(|_| DbError::Corrupt("bad project id".into()))?,
        name: row.get(1)?,
        status,
        description: row.get(3)?,
        build_quantity: row.get(4)?,
        repo_link: row.get(5)?,
        notes: row.get(6)?,
        created_at: row.get(7)?,
        completed_at: row.get(8)?,
    })
}
