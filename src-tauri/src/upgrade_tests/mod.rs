//! Upgrade and robustness harness (v0.7 WP1). Test-only.
//!
//! * `db`: databases written by every released schema, migrated by the
//!   current code.
//! * `settings_files`: settings files written by released versions, and a
//!   table of hostile files, through both the start-up and the hot-reload path.
//!
//! Fixtures live in `src-tauri/tests/fixtures/`; see `docs/VALIDATION.md` for
//! how to add one when the schema or the settings change.

mod db;
mod settings_files;
