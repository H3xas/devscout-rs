use super::*;
use std::fs;

// --- TS/JS purposes (acceptance cases) -------
//
// Every fixture under fixtures/ts-grammar/ against its expected-output
// string, byte-exact. These are the acceptance cases for TS/JS purpose
// extraction: a mismatch means the extractor changed behaviour, so fix
// the extractor rather than the fixture or the expected string.

use crate::parse::TsGrammar;

fn ts_purpose(src: &str, grammar: TsGrammar) -> Option<String> {
    extract_ts_purpose(src, grammar)
}

const ARTICLE_CARD_TSX: &str = include_str!("../../../fixtures/ts-grammar/ArticleCard.tsx");
const USE_WIDGET_DATA_TS: &str = include_str!("../../../fixtures/ts-grammar/useWidgetData.ts");
const ORDER_SERVICE_TS: &str = include_str!("../../../fixtures/ts-grammar/OrderService.ts");
const WIDGETS_SLICE_TS: &str = include_str!("../../../fixtures/ts-grammar/widgetsSlice.ts");
const STRING_UTILS_TS: &str = include_str!("../../../fixtures/ts-grammar/stringUtils.ts");
const ARTICLE_TYPES_TS: &str = include_str!("../../../fixtures/ts-grammar/articleTypes.ts");
const FORMAT_CURRENCY_TS: &str = include_str!("../../../fixtures/ts-grammar/formatCurrency.ts");
const NOTIFICATION_DIGEST_TS: &str =
    include_str!("../../../fixtures/ts-grammar/notificationDigest.ts");
const PATH_HELPERS_JS: &str = include_str!("../../../fixtures/ts-grammar/pathHelpers.js");
// New fixtures.
const REEXPORT_BARREL_TS: &str = include_str!("../../../fixtures/ts-grammar/reexportBarrel.ts");
const WIDGET_PANEL_WITH_NOTE_TS: &str =
    include_str!("../../../fixtures/ts-grammar/widgetPanelWithNote.ts");

#[test]
fn ts_grammar_fixture_article_card_tsx() {
    assert_eq!(
        ts_purpose(ARTICLE_CARD_TSX, TsGrammar::Tsx).as_deref(),
        Some("function ArticleCard | interface ArticleCardProps")
    );
}

#[test]
fn ts_grammar_fixture_use_widget_data_ts() {
    assert_eq!(
        ts_purpose(USE_WIDGET_DATA_TS, TsGrammar::Typescript).as_deref(),
        Some("function useWidgetData")
    );
}

#[test]
fn ts_grammar_fixture_order_service_ts() {
    assert_eq!(
        ts_purpose(ORDER_SERVICE_TS, TsGrammar::Typescript).as_deref(),
        Some("class OrderService : IOrderService; getOrder, cancelOrder | interface IOrderService")
    );
}

#[test]
fn ts_grammar_fixture_widgets_slice_ts() {
    assert_eq!(
        ts_purpose(WIDGETS_SLICE_TS, TsGrammar::Typescript).as_deref(),
        Some("const widgetsSlice | const setLoading | const setItems | default (anonymous)")
    );
}

#[test]
fn ts_grammar_fixture_string_utils_ts() {
    assert_eq!(
        ts_purpose(STRING_UTILS_TS, TsGrammar::Typescript).as_deref(),
        Some("function truncate | function slugify | const DEFAULT_MAX_LENGTH")
    );
}

#[test]
fn ts_grammar_fixture_article_types_ts() {
    assert_eq!(
            ts_purpose(ARTICLE_TYPES_TS, TsGrammar::Typescript).as_deref(),
            Some("interface ArticleItem | interface ArticleAuthor | type ArticleItemStatus | type ArticlePage")
        );
}

#[test]
fn ts_grammar_fixture_format_currency_ts() {
    assert_eq!(
        ts_purpose(FORMAT_CURRENCY_TS, TsGrammar::Typescript).as_deref(),
        Some("default (anonymous)")
    );
}

#[test]
fn ts_grammar_fixture_notification_digest_ts() {
    assert_eq!(
        ts_purpose(NOTIFICATION_DIGEST_TS, TsGrammar::Typescript).as_deref(),
        Some("class NotificationDigestBuilder; addEvent, build | function buildDailyDigest")
    );
}

#[test]
fn ts_grammar_fixture_path_helpers_js() {
    assert_eq!(
        ts_purpose(PATH_HELPERS_JS, TsGrammar::Javascript).as_deref(),
        Some("function joinSafe | function isAbsolute")
    );
}

// --- reexportBarrel.ts fixture + edge cases -----------

#[test]
fn ts_grammar_fixture_reexport_barrel_ts() {
    assert_eq!(
        ts_purpose(REEXPORT_BARREL_TS, TsGrammar::Typescript).as_deref(),
        Some("reexports WidgetCard, WidgetList, Dialog, *, WidgetTypes")
    );
}

#[test]
fn reexport_bucket_mixes_with_a_real_export_and_lands_last() {
    let src = "export function real() {}\nexport { A, B } from './m';\n";
    assert_eq!(
        ts_purpose(src, TsGrammar::Typescript).as_deref(),
        Some("function real | reexports A, B")
    );
}

#[test]
fn local_export_clause_without_from_is_still_skipped() {
    let src = "const A = 1;\nexport { A };\n";
    assert_eq!(ts_purpose(src, TsGrammar::Typescript), None);
}

#[test]
fn bare_star_reexports_dedupe_to_one_token_regardless_of_statement_count() {
    let src = "export * from './a';\nexport * from './b';\n";
    assert_eq!(
        ts_purpose(src, TsGrammar::Typescript).as_deref(),
        Some("reexports *")
    );
}

// --- Leading-comment hybrid: widgetPanelWithNote.ts fixture (pure vs hybrid) ---
//
// The pure entry point (`extract_ts_purpose`, exercised via `ts_purpose`
// above) never sees the heuristic -- it must stay AST-only even for a
// fixture whose first line IS a comment. `extract_ts_purpose_with_heuristic`
// (mapcmd.rs's real per-file dispatch and run_extract_dump's dump path)
// is what actually applies the comment prefix -- tested separately below
// via a real temp-dir root/rel pair.

#[test]
fn ts_grammar_fixture_widget_panel_with_note_pure_extractor_stays_ast_only() {
    assert_eq!(
        ts_purpose(WIDGET_PANEL_WITH_NOTE_TS, TsGrammar::Typescript).as_deref(),
        Some("function WidgetPanel | interface WidgetPanelProps")
    );
}

fn scratch_root(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "scout-extract-hybrid-test-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn extract_ts_purpose_with_heuristic_prefixes_a_leading_comment() {
    let root = scratch_root("comment");
    fs::write(root.join("widget.ts"), WIDGET_PANEL_WITH_NOTE_TS).unwrap();
    let purpose = extract_ts_purpose_with_heuristic(
        &root,
        "widget.ts",
        WIDGET_PANEL_WITH_NOTE_TS,
        TsGrammar::Typescript,
    );
    assert_eq!(
            purpose.as_deref(),
            Some("Renders the widget summary panel and its retry action — function WidgetPanel | interface WidgetPanelProps")
        );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn extract_ts_purpose_with_heuristic_leaves_a_code_first_line_unprefixed() {
    let root = scratch_root("code-first");
    let src = "export function plain() { return 1; }\n";
    fs::write(root.join("plain.ts"), src).unwrap();
    let purpose = extract_ts_purpose_with_heuristic(&root, "plain.ts", src, TsGrammar::Typescript);
    assert_eq!(purpose.as_deref(), Some("function plain"));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn extract_ts_purpose_with_heuristic_returns_none_for_comment_plus_zero_export() {
    // Zero-export files have no purpose downstream -- no purpose at all
    // comes out of this entry point for them, matching extract_ts_purpose's
    // own None contract.
    let root = scratch_root("comment-zero-export");
    let src = "// just notes, nothing exported\nfunction internalOnly() { return 1; }\n";
    fs::write(root.join("notesOnly.ts"), src).unwrap();
    let purpose =
        extract_ts_purpose_with_heuristic(&root, "notesOnly.ts", src, TsGrammar::Typescript);
    assert!(purpose.is_none());
    fs::remove_dir_all(&root).ok();
}

// --- degrade paths -------------------------------------------------

#[test]
fn broken_syntax_ts_degrades_to_none_not_a_panic() {
    // Genuinely malformed: no recognisable top-level export survives
    // error recovery, so compose_ts_purpose yields no entries -> None,
    // same signal that routes mapcmd.rs's per-file dispatch to the
    // heuristic purpose (broken-syntax TS degrades to the heuristic).
    let purpose = ts_purpose(
        "export class {{{ not valid typescript at all @@@",
        TsGrammar::Typescript,
    );
    assert!(purpose.is_none());
}

#[test]
fn zero_export_file_yields_none_heuristic_fallback() {
    // The zero-export shape follows the established C# convention (no
    // namespace-level types -> None -> heuristic fallback) rather than
    // inventing new behaviour.
    assert!(ts_purpose(
        "function internalOnly() { return 1; }\nconst alsoInternal = 2;\n",
        TsGrammar::Typescript
    )
    .is_none());
    assert!(ts_purpose("// just a comment, no code\n", TsGrammar::Typescript).is_none());
}
