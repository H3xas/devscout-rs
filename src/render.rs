// Default / --compact rendering for `refs`/`impact`. This module owns
// `render_refs_text`/`render_impact_text` (default), `render_refs_compact`/
// `render_impact_compact` (`--compact`), and their shared helpers
// (`ref_kind_block`/`compact_block`/`rle`/`group_by_file`). Analytics/dev output
// (triage/analyze/report) is out of scope here.
//
// `--json` output is NOT built here -- it lives in cli.rs alongside the rest of
// the CLI-level `--json` flag handling, matching the module split.
//
// Rendering notes (`ref_kind_block`/`compact_block`):
//
// - `ref_kind_block` (default renderer) prints its header line whenever the
//   table itself is present, EVEN IF `total == 0` -- only a genuinely
//   missing/undefined table suppresses the header entirely. `compact_block`
//   additionally suppresses on `total == 0` (empty tables print nothing in
//   `--compact`, not even a header). This asymmetry is real and preserved:
//   see `ref_kind_block_present_but_empty_table_still_prints_header` below.
// - "Missing table" tolerance (the `Option<&Table<R>>` parameter): query.rs's
//   typed `RefsModel`/`ImpactModel` always supply a concrete (possibly
//   zero-total) `Table` for every kind, so the `None` branch is unreachable from
//   a real model. It is kept because the block helpers are tested directly with
//   an absent table to exercise exactly this path; it is not reachable through
//   the public render_* functions.

mod blocks;
mod coverage;
mod impact;
mod markers;
mod read;
mod refs;

pub use coverage::{render_tests_compact, render_tests_text};
pub use impact::{
    render_impact_compact, render_impact_compact_with_imports, render_impact_text,
    render_impact_text_with_imports,
};
pub use markers::seed_kind_str;
pub use read::{render_read_compact, render_read_text};
pub use refs::{render_refs_compact, render_refs_text};

#[cfg(test)]
mod tests;
