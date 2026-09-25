use crate::graph::HeuristicTier;
use crate::query;

// The ONE place a heuristic row is marked in the default text renderer, and the
// marker is a plain-language word rather than a symbol: the consumer is a model
// reading a wall of `file:line kind` rows, and a sigil it has to look up is the
// same as no marker at all. A row with no flag renders no suffix.
//
// This one is the UMBRELLA word: it stays what the headers and the summary
// counts say, and it is what a row falls back to when it declares itself a
// guess but names no tier -- a graph written before the tier tag existed.
const HEURISTIC_SUFFIX: &str = " (heuristic)";

// The two tiers spelled out, one word each. Telling them apart is the point:
// an extension row is C#'s own lookup rule with only the receiver left
// unverified, a guess row is the scored tier picking among the defs that
// happen to declare a member of that name. A reader should never have to
// remember which of two markers means which -- the word says it.
const EXT_SUFFIX: &str = " (extension)";
const GUESS_SUFFIX: &str = " (guess)";

// The same shape of suffix as `HEURISTIC_SUFFIX`, for the hub-file class, so a
// row carries its own reason and the two never need to be told apart by
// position.
pub(crate) const INFRA_SUFFIX: &str = " (infra)";

// The one wording every `bus-hop` disclosure -- `refs`/`read`'s own bus
// rows, and `impact`/`tests`' possible-route rows -- states, so the several
// renderers that carry it can never drift from each other's wording again.
pub(crate) const BUS_HOP_UNVERIFIED: &str = "possible route, runtime routing unverified";

// `tests`-only: marks a row whose file earned its place through the project
// model alone (`TestVia::Project`) rather than an attributed test def in the
// file itself. Composes after the heuristic suffix, never instead of it -- a
// row can be both a guess AND project-vouched.
const TEST_PROJECT_SUFFIX: &str = " (test project)";

// The row's own tier word when the row is a guess, `""` otherwise. Applied
// inside each row formatter rather than inside `ref_kind_block`, because the two
// tables that can never hold a guess (imports, ambiguous) carry no flag to read
// -- they get no suffix, the same as any non-guess row.
//
// `tier` is read ONLY under `heuristic`: a precise row has nothing to declare
// even if some future caller hands one a tier by mistake.
pub(crate) fn heuristic_suffix(heuristic: bool, tier: Option<HeuristicTier>) -> &'static str {
    match (heuristic, tier) {
        (false, _) => "",
        (true, Some(HeuristicTier::Ext)) => EXT_SUFFIX,
        (true, Some(HeuristicTier::Guess)) => GUESS_SUFFIX,
        (true, None) => HEURISTIC_SUFFIX,
    }
}

// `tests`-only: the `TEST_PROJECT_SUFFIX` when the row's vouch is
// `TestVia::Project`, `""` for `TestVia::Attribute`. Kept as its own function
// (rather than folded into `heuristic_suffix`) because the two facts are
// independent -- a row's vouch has nothing to do with whether its reference
// was guessed.
pub(crate) fn test_via_suffix(via: query::TestVia) -> &'static str {
    match via {
        query::TestVia::Attribute => "",
        query::TestVia::Project => TEST_PROJECT_SUFFIX,
    }
}

// `--compact`'s one-character spelling of the same fact, appended to a line
// number (`5x`, `5h`). One character rather than the default renderer's word
// because compact exists to spend as few bytes as possible; still mandatory,
// because an unmarked guess sitting in a list of facts is exactly the failure
// this resolver is built to avoid and a small output is no defence.
//
// The run-length collapse in `rle` composes with it without ambiguity: a `5x`
// row appearing twice is `5xx2`, which reads as "line 5, extension, twice" --
// the marker belongs to the value, the `x2` is the count.
pub(crate) fn compact_marker(heuristic: bool, tier: Option<HeuristicTier>) -> &'static str {
    match (heuristic, tier) {
        (false, _) => "",
        (true, Some(HeuristicTier::Ext)) => "x",
        (true, _) => "h",
    }
}

// The source snippet, two spaces then the text, appended AFTER the heuristic
// suffix by `ref_kind_block`. Only inbound rows ever carry a source line; every
// other table's rows leave it empty and render no suffix.
pub(crate) fn source_suffix(source: &str) -> String {
    if source.is_empty() {
        String::new()
    } else {
        format!("  {source}")
    }
}

/// The impact seed kind as a string (`"file"`/`"symbol"`), used in the impact
/// renderers' header line and in cli.rs's `--json` and error messages.
pub fn seed_kind_str(kind: query::SeedKind) -> &'static str {
    match kind {
        query::SeedKind::File => "file",
        query::SeedKind::Symbol => "symbol",
    }
}
