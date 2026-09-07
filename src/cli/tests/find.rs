use super::*;

// The expected strings below pin the find output caps; the integration suite
// also exercises the same caps end to end.
fn find_cap_root(prefix: &str, entry_count: usize) -> PathBuf {
    let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let scout = root.join(".scout");
    std::fs::create_dir_all(&scout).unwrap();
    let entries: Vec<String> = (0..entry_count)
        .map(|i| {
            format!(r#""src/widget-{i:02}.cs": {{ "purpose": "widget number {i}", "mtime": {i} }}"#)
        })
        .collect();
    let json = format!(r#"{{ "entries": {{ {} }} }}"#, entries.join(", "));
    std::fs::write(scout.join("manifest.json"), json).unwrap();
    root
}

#[test]
fn cmd_find_caps_full_pool_at_25_with_honest_tail() {
    let root = find_cap_root("findcap-full", 30);
    let (code, out) = cmd_find(&root, "widget", false);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 26);
    assert_eq!(lines[25], "… +5 more (refine query)");
    assert!(lines[..25].iter().all(|l| l.contains("widget")));
}

#[test]
fn cmd_find_caps_fallback_pool_at_10_with_honest_tail() {
    let root = find_cap_root("findcap-fallback", 12);
    let (code, out) = cmd_find(&root, "widget zzz123nomatch", false);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.split('\n').collect();
    assert_eq!(lines.len(), 11);
    assert_eq!(lines[10], "… +2 more (refine query)");
}

// The note text and the stream it lands on are pinned in
// tests/cli_zero_hit.rs, which can see both streams; what belongs here is
// that the code the verb computes is EXIT_NO_RESULT and not 0.
#[test]
fn cmd_find_reports_a_zero_hit_on_its_own_exit_code_with_stdout_unchanged() {
    let root = find_cap_root("findcap-zero", 5);
    let (code, out) = cmd_find(&root, "zzz123nosuchpurpose", false);
    assert_eq!(code, EXIT_NO_RESULT);
    assert_eq!(
        out,
        "no matches for \"zzz123nosuchpurpose\" (run 'devscout map' if manifest is missing)"
    );
}

#[test]
fn cmd_find_below_cap_prints_no_tail() {
    let root = find_cap_root("findcap-small", 5);
    let (code, out) = cmd_find(&root, "widget", false);
    assert_eq!(code, 0);
    assert_eq!(out.split('\n').count(), 5);
    assert!(!out.contains("more (refine query)"));
}

// --- the declaration block ----------------------------------------------

const LEDGER_CS: &str = "namespace Gadgets;\n\npublic class WidgetLedger\n{\n\tprivate int _entryCount;\n\n\tpublic string Label { get; set; }\n\n\tpublic event EventHandler Retired;\n\n\tprivate void PopulateSlots() { }\n}\n";
const PANEL_XAML: &str = "<UserControl\n\tx:Class=\"Gadgets.PanelView\">\n\t<Button x:Name=\"ShipButton\" />\n</UserControl>\n";
const STRINGS_RESW: &str =
    "<root>\n\t<resheader name=\"resmimetype\" />\n\t<data name=\"ShipButton.Content\" xml:space=\"preserve\" />\n</root>\n";

// A repo whose graph.json is the one `map` would write for these three files
// -- built through the real resolver, not hand-authored JSON, so the test
// cannot drift from the index the map path actually produces.
fn name_index_root(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src").join("WidgetLedger.cs"), LEDGER_CS).unwrap();
    std::fs::write(root.join("src").join("Panel.xaml"), PANEL_XAML).unwrap();
    std::fs::write(root.join("src").join("Strings.resw"), STRINGS_RESW).unwrap();
    std::fs::create_dir_all(root.join(".scout")).unwrap();
    std::fs::write(
        root.join(".scout").join("manifest.json"),
        r#"{ "entries": {} }"#,
    )
    .unwrap();

    let fragments = vec![
        (
            "src/WidgetLedger.cs".to_string(),
            graph::fragment_from_extraction(&crate::extract::extract(LEDGER_CS)),
        ),
        (
            "src/Panel.xaml".to_string(),
            graph::markup_fragment(&root, "src/Panel.xaml").unwrap(),
        ),
        (
            "src/Strings.resw".to_string(),
            graph::markup_fragment(&root, "src/Strings.resw").unwrap(),
        ),
    ];
    let g = crate::resolve::resolve_graph(&root, &fragments);
    std::fs::create_dir_all(root.join(".scout").join("graph")).unwrap();
    std::fs::write(
        root.join(".scout").join("graph").join("graph.json"),
        serde_json::to_string(&g).unwrap(),
    )
    .unwrap();
    root
}

#[test]
fn cmd_find_resolves_every_code_and_markup_category_to_its_declaration_line() {
    // A resource key (`ShipButton.Content`) is not in this loop: it is tier
    // 3, hidden by default. Its own default/--resources behavior is pinned
    // separately below.
    let root = name_index_root("findnames");
    for (query, expected) in [
        (
            "PopulateSlots",
            "src/WidgetLedger.cs:11  private void PopulateSlots() { }",
        ),
        (
            "Label",
            "src/WidgetLedger.cs:7  public string Label { get; set; }",
        ),
        (
            "_entryCount",
            "src/WidgetLedger.cs:5  private int _entryCount;",
        ),
        (
            "Retired",
            "src/WidgetLedger.cs:9  public event EventHandler Retired;",
        ),
        (
            "Gadgets.PanelView",
            "src/Panel.xaml:2  x:Class=\"Gadgets.PanelView\">",
        ),
    ] {
        let (code, out) = cmd_find(&root, query, false);
        assert_eq!(code, 0, "{query} should be a hit");
        assert_eq!(out, expected, "{query}");
    }
}

// --- kind tiering --------------------------------------------------------

#[test]
fn cmd_find_a_resource_key_only_query_brakes_correctly_by_default_and_is_a_hit_under_resources() {
    // `ShipButton.Content` matches ONLY the resource-key row -- no
    // markup-name row contains the full stop, and the manifest is empty -- so
    // this is the drowned-query case: a query that would otherwise return
    // resource keys (here, one) and nothing else brakes, correctly, instead
    // of looking like a hit.
    let root = name_index_root("findnames-resource-only");
    let (code, out) = cmd_find(&root, "ShipButton.Content", false);
    assert_eq!(
        code, EXIT_NO_RESULT,
        "a resource-key-only match is a zero hit by default"
    );
    assert_eq!(
        out,
        "no matches for \"ShipButton.Content\" (run 'devscout map' if manifest is missing)"
    );

    let (code, out) = cmd_find(&root, "ShipButton.Content", true);
    assert_eq!(code, 0, "--resources lifts the demotion");
    assert_eq!(
        out,
        "src/Strings.resw:3  <data name=\"ShipButton.Content\" xml:space=\"preserve\" />"
    );
}

#[test]
fn cmd_find_default_view_hides_resource_keys_behind_a_trailer_and_resources_shows_them_inline() {
    // `ShipButton` matches BOTH the tier-2 markup-name `ShipButton` and
    // the tier-3 resource key `ShipButton.Content` (substring). Default:
    // the tier-2 row shows, the tier-3 row is a one-line trailer. Under
    // `--resources`: both rows show, inline, in build order, no trailer.
    let root = name_index_root("findnames-tiering");
    let (code, out) = cmd_find(&root, "ShipButton", false);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "src/Panel.xaml:3  <Button x:Name=\"ShipButton\" />\n+1 resource-key hits, use --resources",
        "tier 3 is a trailer, never an inline row, by default"
    );

    let (code, out) = cmd_find(&root, "ShipButton", true);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "src/Panel.xaml:3  <Button x:Name=\"ShipButton\" />\nsrc/Strings.resw:3  <data name=\"ShipButton.Content\" xml:space=\"preserve\" />",
        "--resources includes the resource-key row inline and drops the trailer"
    );
}

#[test]
fn cmd_find_puts_the_declaration_block_above_the_manifest_block() {
    let root = name_index_root("findnames-order");
    std::fs::write(
        root.join(".scout").join("manifest.json"),
        r#"{ "entries": { "src/WidgetLedger.cs": { "purpose": "class WidgetLedger", "mtime": 1 } } }"#,
    )
    .unwrap();
    let (code, out) = cmd_find(&root, "WidgetLedger", false);
    assert_eq!(code, 0);
    assert_eq!(
        out,
        "src/WidgetLedger.cs:3  public class WidgetLedger\nsrc/WidgetLedger.cs:3: class WidgetLedger"
    );
}

// The manifest-pool row carries its own file's first declaration line, not
// just the declaration block above it. `Panel.xaml` has no manifest entry in
// `name_index_root`'s fixture, so this proves the line on a row that has ONE
// (a real manifest hit for a file the name index also carries a declaration
// for), independent of the declaration block.
#[test]
fn cmd_find_manifest_pool_row_carries_its_files_first_declaration_line() {
    let root = name_index_root("findnames-manifest-line");
    std::fs::write(
        root.join(".scout").join("manifest.json"),
        r#"{ "entries": { "src/WidgetLedger.cs": { "purpose": "class WidgetLedger; Ship", "mtime": 1 } } }"#,
    )
    .unwrap();
    let (code, out) = cmd_find(&root, "WidgetLedger", false);
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.split('\n').collect();
    let manifest_row = lines
        .iter()
        .find(|l| l.starts_with("src/WidgetLedger.cs:") && l.contains("; Ship"));
    assert_eq!(
        manifest_row,
        Some(&"src/WidgetLedger.cs:3: class WidgetLedger; Ship"),
        "{out}"
    );
}

// A manifest hit whose file the name index carries NO declaration for at all
// (a plain, undeclared source file) falls back to line 1 -- an always-valid
// "open the file" anchor -- rather than emitting a row with no line.
#[test]
fn cmd_find_manifest_pool_row_for_a_file_with_no_declaration_falls_back_to_line_1() {
    let root = name_index_root("findnames-manifest-no-decl");
    std::fs::write(root.join("src").join("Notes.md"), "# widget notes\n").unwrap();
    std::fs::write(
        root.join(".scout").join("manifest.json"),
        r#"{ "entries": { "src/Notes.md": { "purpose": "widget notes doc", "mtime": 1 } } }"#,
    )
    .unwrap();
    let (code, out) = cmd_find(&root, "widget", false);
    assert_eq!(code, 0);
    assert!(out.contains("src/Notes.md:1: widget notes doc"), "{out}");
}

#[test]
fn cmd_find_still_takes_the_zero_hit_exit_when_neither_index_matches() {
    let root = name_index_root("findnames-zero");
    let (code, out) = cmd_find(&root, "Zzzznomatch", false);
    assert_eq!(code, EXIT_NO_RESULT);
    assert_eq!(
        out,
        "no matches for \"Zzzznomatch\" (run 'devscout map' if manifest is missing)"
    );
}
