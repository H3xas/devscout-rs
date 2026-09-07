// Arg parsing + dispatch for the subcommand set. Dispatch is hand-rolled; no
// flag-parsing dependency is pulled in until a subcommand needs real flag
// parsing.
//
// Alongside the user-facing verbs (README.md), four dev/diagnostic subcommands
// are wired here and nowhere else: `noop` (cold-start floor), `parse` and
// `spans` (seed AST dumps), and `extract-dump` (full-extraction JSON).
//
// `init` dispatches through `initcmd::cmd_init_full`, NOT `cmd_init` directly --
// see initcmd.rs for why that split is load-bearing rather than stylistic.

mod admin;
mod answer;
mod args;
mod coverage;
mod dispatch;
mod find;
mod impact;
mod import_edges;
mod read;
mod refs;
mod root;

pub use dispatch::dispatch;
pub(crate) use root::require_repo;

#[cfg(test)]
mod tests;
