-- Phase 5b Task 1: link an import to the transaction_groups row its atomic
-- commit creates (spec §10 Match/Review/Commit).
--
-- `commit_group_id` is NULL until `commit_import` (Task 4) runs; once set it
-- names the `transaction_groups.id` that group contains every `receive`
-- transaction the import produced, so `reverse_import` can look it up and
-- call `reverse_group` on it, and History can jump from an import back to
-- "what this import did" as one unit.
--
-- This column is intentionally FK-less, the same way `transactions.bom_item_id`
-- (migration 0002/0006) stays FK-less: SQLite's `ALTER TABLE ... ADD COLUMN`
-- cannot attach a foreign key to an existing STRICT table without a full
-- table rebuild, and `commit_import` (the only writer of this column) always
-- populates it from a `transaction_groups.id` it just created in the same
-- transaction, so a DB-level FK would only ever catch a domain bug, never bad
-- user input. Enforced in code, not the schema.
ALTER TABLE imports ADD COLUMN commit_group_id TEXT;
CREATE INDEX idx_imports_commit_group ON imports(commit_group_id);
