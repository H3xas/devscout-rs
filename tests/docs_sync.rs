//! `docs/reference/cli.md`, `environment.md`, `exit-codes.md` and `storage.md`
//! are generated from the definitions the binary itself uses, and these checks
//! fail when a committed page and the code disagree.
//!
//! Two kinds of check live here, in this order:
//!
//! - **Structural checks**, which read the source: every usage line is a
//!   constant, every environment name the source reads is documented, every
//!   exit status is one of the named constants, and every artifact name has one
//!   definition. They run before any page is compared or written, so `BLESS=1`
//!   regenerates pages and can never talk a structural failure away.
//! - **Page checks**, which render the pages and compare them with the
//!   committed bytes. `BLESS=1 cargo test --test docs_sync` rewrites them;
//!   without it a difference is a failure naming the page and that command.
//!
//! The help text and the help/usage behaviour come from the binary this test
//! builds, so what the pages state about `--help` is what the shipped binary
//! does.

// The support modules live beside this file rather than next to it: `tests/docs_sync/` holds no
// test target of its own, so each one is named explicitly.
#[path = "docs_sync/anchors.rs"]
mod anchors;
#[path = "docs_sync/facts.rs"]
mod facts;
#[path = "docs_sync/pages.rs"]
mod pages;
#[path = "docs_sync/scan.rs"]
mod scan;
#[path = "docs_sync/sync.rs"]
mod sync;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use scan::Scanned;

const PAGES: [(&str, &str); 4] = [
    ("cli.md", "CLI reference"),
    ("environment.md", "Environment variables"),
    ("exit-codes.md", "Exit codes"),
    ("storage.md", "Storage"),
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn reference_dir() -> PathBuf {
    manifest_dir().join("docs/reference")
}

fn bless() -> bool {
    std::env::var("BLESS").as_deref() == Ok("1")
}

fn scan_sources(root: &Path) -> Vec<Scanned> {
    let source = root.join("src");
    match scan::scan_tree(&source) {
        Ok(files) => files,
        Err(error) => panic!("cannot scan {}: {error}", source.display()),
    }
}

fn run_binary(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_devscout"))
        .args(args)
        .output()
        .expect("the test-built binary runs")
}

fn stdout_of(args: &[&str]) -> String {
    let output = run_binary(args);
    assert!(
        output.status.success(),
        "{args:?} exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

// ---------------------------------------------------------------- structure

/// The structural failures of one source tree. Empty means the pages can be
/// compared with it.
fn structural_failures(root: &Path) -> Vec<String> {
    let files = scan_sources(root);
    let mut failures = Vec::new();
    failures.extend(usage_failures(&files));
    failures.extend(environment_failures(&files));
    failures.extend(exit_failures(&files));
    failures.extend(artifact_failures(&files));
    failures
}

// Every usage line the source carries is a declared constant, and every
// declared constant is rendered: a new or edited literal fails until it becomes
// a constant, and a constant no page renders fails until the page is
// regenerated.
fn usage_failures(files: &[Scanned]) -> Vec<String> {
    let mut failures = Vec::new();
    let declared: BTreeMap<&str, &str> = pages::USAGE_CONSTANTS.iter().copied().collect();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for literal in facts::usage_literals(files) {
        if literal.file != PathBuf::from("usage.rs") {
            failures.push(format!(
                "{}: a usage line outside the usage module",
                literal.describe()
            ));
            continue;
        }
        if !declared.values().any(|value| *value == literal.detail) {
            failures.push(format!(
                "{}: a usage constant no page declares",
                literal.describe()
            ));
            continue;
        }
        seen.insert(literal.detail.clone());
    }
    for (name, value) in &declared {
        if !seen.contains(*value) {
            failures.push(format!(
                "usage::{} is declared but no literal carries it",
                name
            ));
        }
    }
    failures
}

// The variable table lists exactly the names the source reads, plus `HOME`, and
// no retired name is read again.
fn environment_failures(files: &[Scanned]) -> Vec<String> {
    let mut failures = Vec::new();
    let scanned = facts::environment_names(files);
    let documented: BTreeSet<String> = pages::ENVIRONMENT
        .iter()
        .map(|row| row.name.to_string())
        .collect();
    for name in &scanned {
        if !documented.contains(name) {
            failures.push(format!(
                "the source reads {name}, and the environment page has no row for it"
            ));
        }
    }
    for name in &documented {
        if !scanned.contains(name) {
            failures.push(format!(
                "the environment page documents {name}, and no source literal carries it"
            ));
        }
    }
    for row in pages::RETIRED {
        if scanned.contains(row.name) {
            failures.push(format!(
                "the environment page lists {} as retired, and the source reads it again",
                row.name
            ));
        }
    }
    failures
}

/// The environment names the source reads, for a caller that has to name one.
pub fn scanned_environment_names() -> BTreeSet<String> {
    facts::environment_names(&scan_sources(&manifest_dir()))
}

// Every exit status is one of the named constants: no bare integer is returned
// as a status or passed to a process exit, and the constants the page describes
// are the ones the library exposes.
fn exit_failures(files: &[Scanned]) -> Vec<String> {
    let mut failures = Vec::new();
    for site in facts::numeric_status_sites(files) {
        failures.push(site.describe());
    }
    let scanned = facts::exit_constants(files);
    let library: BTreeMap<String, i64> = [
        ("EXIT_OK".to_string(), devscout_rs::exit::EXIT_OK as i64),
        (
            "EXIT_FAILURE".to_string(),
            devscout_rs::exit::EXIT_FAILURE as i64,
        ),
        (
            "EXIT_USAGE".to_string(),
            devscout_rs::exit::EXIT_USAGE as i64,
        ),
        (
            "EXIT_NO_RESULT".to_string(),
            devscout_rs::exit::EXIT_NO_RESULT as i64,
        ),
    ]
    .into_iter()
    .collect();
    if scanned != library {
        failures.push(format!(
            "the exit constants the source declares ({scanned:?}) are not the library's ({library:?})"
        ));
    }
    let described: BTreeMap<&str, i64> = pages::EXIT_CODES
        .iter()
        .map(|row| (row.constant, row.value))
        .collect();
    let library_by_name: BTreeMap<&str, i64> = library
        .iter()
        .map(|(name, value)| (name.as_str(), *value))
        .collect();
    if described != library_by_name {
        failures.push(format!(
            "the exit-code page describes {described:?}, and the library holds {library_by_name:?}"
        ));
    }
    failures
}

// Every listed artifact name has exactly one literal in the source, in its own
// constant, so a rename reaches the path builders and the messages that name an
// artifact rather than stopping at one of them.
fn artifact_failures(files: &[Scanned]) -> Vec<String> {
    let mut failures = Vec::new();
    let listed = pages::listed_artifact_names();
    for site in facts::names_outside_their_constant(files, "layout.rs", &listed) {
        failures.push(site.describe());
    }
    for name in &listed {
        let mut definitions = 0usize;
        for scanned in files {
            if scanned.path != PathBuf::from("layout.rs") {
                continue;
            }
            definitions += scanned
                .values()
                .filter(|value| *value == name.as_str())
                .count();
        }
        if definitions != 1 {
            failures.push(format!(
                "the artifact {name} has {definitions} defining constants, and must have exactly one"
            ));
        }
    }
    failures
}

/// Panic unless the repository's own source passes every structural check.
fn require_structure() {
    let failures = structural_failures(&manifest_dir());
    assert!(
        failures.is_empty(),
        "the pages cannot be generated from this source:\n  {}",
        failures.join("\n  ")
    );
}

// -------------------------------------------------------------------- pages

/// Compare a rendered page with the committed one, or write it under `BLESS=1`.
/// A drift names the page and the command that regenerates it.
fn compare_page(path: &Path, content: &str) {
    match sync::sync_page(path, content, bless()) {
        Ok(_) => {}
        Err(message) => panic!("{message}"),
    }
}

// The fence a page renders a usage constant in, exactly once per constant.
fn usage_block_body(value: &str) -> String {
    format!("```text\n{value}\n```")
}

/// The heading a usage block sits under, or `None` when the page carries no
/// block with that body.
fn section_of(page: &str, block: &str) -> Option<String> {
    let mut section: Option<String> = None;
    let mut lines = page.lines().peekable();
    let mut collected = String::new();
    while let Some(line) = lines.next() {
        if let Some(heading) = line
            .strip_prefix("## ")
            .or_else(|| line.strip_prefix("### "))
        {
            section = Some(heading.trim().to_string());
            continue;
        }
        if line.starts_with("```text") {
            collected.clear();
            collected.push_str(line);
            collected.push('\n');
            while let Some(next) = lines.next() {
                collected.push_str(next);
                collected.push('\n');
                if next == "```" {
                    break;
                }
            }
            if collected.trim_end() == block {
                return section;
            }
        }
    }
    None
}

/// Inline code spans outside a fenced block that spell a verb together with a
/// flag, which the verbatim blocks exist to replace.
fn flag_spellings_outside_fences(page: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut fenced = false;
    for line in page.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for span in line.split('`').skip(1).step_by(2) {
            let Some(position) = span.find("devscout") else {
                continue;
            };
            let after = span[position + "devscout".len()..].trim_start();
            let verb: String = after
                .chars()
                .take_while(|character| character.is_alphanumeric() || *character == '-')
                .collect();
            if verb.is_empty() {
                continue;
            }
            if after[verb.len()..].contains("--") || after[verb.len()..].contains(" -") {
                found.push(span.trim().to_string());
            }
        }
    }
    found
}

#[test]
fn docs_sync_ac_1_cli_page_is_generated() {
    require_structure();
    let help = stdout_of(&["--help"]);
    let content = pages::cli_page(&help);
    compare_page(&reference_dir().join("cli.md"), &content);
}

#[test]
fn docs_sync_ac_1_help_block_is_the_binary_stdout() {
    let help = stdout_of(&["--help"]);
    let page = pages::cli_page(&help);
    assert!(
        page.contains(&format!("```text\n{help}```")),
        "the CLI page does not carry the help text of the binary that printed it"
    );
}

#[test]
fn docs_sync_ac_1_usage_blocks_sit_under_their_verbs() {
    let help = stdout_of(&["--help"]);
    let page = pages::cli_page(&help);
    for (section, constant) in pages::USAGE_SECTIONS {
        let value = pages::usage_value(constant).expect("the section names a declared constant");
        let block = usage_block_body(value);
        let observed = section_of(&page, &block)
            .unwrap_or_else(|| panic!("the CLI page carries no block for {constant}"));
        assert_eq!(
            &observed, section,
            "{constant} is rendered under {observed} instead of {section}"
        );
        assert_eq!(
            page.matches(&block).count(),
            1,
            "{constant} is rendered more than once"
        );
    }
}

#[test]
fn docs_sync_ac_1_flag_spellings_only_in_verbatim_blocks() {
    let help = stdout_of(&["--help"]);
    let page = pages::cli_page(&help);
    let offenders = flag_spellings_outside_fences(&page);
    assert!(
        offenders.is_empty(),
        "the CLI page spells flags outside the verbatim blocks: {offenders:?}"
    );
}

#[test]
fn docs_sync_ac_1_drift_names_the_page_and_the_command() {
    let directory = temp_dir("drift");
    let path = directory.join("cli.md");
    std::fs::create_dir_all(&directory).expect("a temporary directory is writable");
    std::fs::write(&path, "a page that is not what the code renders\n")
        .expect("write a stale page");
    let message = sync::sync_page(&path, "what the code renders\n", false)
        .expect_err("a stale page is a failure");
    assert!(
        message.contains("cli.md"),
        "the message does not name the page: {message}"
    );
    assert!(
        message.contains(pages::REGENERATE),
        "the message does not name the regenerate command: {message}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn docs_sync_ac_2_help_spellings_end_with_the_docs_url() {
    let expected = format!(
        "\n\ndocs: {}/tree/v{}/docs\n",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    );
    let mut printed = Vec::new();
    for args in [vec!["help"], vec!["--help"], vec!["-h"]] {
        let output = run_binary(&args);
        assert!(
            output.status.success(),
            "{args:?} exited {:?}",
            output.status.code()
        );
        assert!(
            output.stderr.is_empty(),
            "{args:?} wrote to stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).expect("stdout is UTF-8");
        assert!(
            text.ends_with(&expected),
            "{args:?} does not end with the documentation URL of this version"
        );
        assert_eq!(
            text.matches("\ndocs: ").count(),
            1,
            "{args:?} prints the footer more than once"
        );
        printed.push(text);
    }
    assert_eq!(
        printed[0], printed[1],
        "`help` and `--help` print different text"
    );
    assert_eq!(
        printed[0], printed[2],
        "`help` and `-h` print different text"
    );
}

#[test]
fn docs_sync_ac_2_version_line_is_unchanged() {
    for args in [vec!["--version"], vec!["-V"]] {
        let output = run_binary(&args);
        assert!(output.status.success(), "{args:?} exited nonzero");
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            format!("devscout {}\n", env!("CARGO_PKG_VERSION")),
            "{args:?} does not print the version line"
        );
        assert!(output.stderr.is_empty(), "{args:?} wrote to stderr");
    }
}

#[test]
fn docs_sync_ac_2_usage_paths_print_their_constant() {
    // (arguments, the constant the run prints, its exit status, which stream)
    let query_verbs: [(&[&str], &str); 6] = [
        (&["refs"], "REFS_USAGE"),
        (&["read"], "READ_USAGE"),
        (&["impact"], "IMPACT_USAGE"),
        (&["tests"], "TESTS_USAGE"),
        (&["find"], "FIND_USAGE"),
        (&["clear", "--older-than", "x"], "CLEAR_OLDER_THAN_USAGE"),
    ];
    for (args, constant) in query_verbs {
        let output = run_binary(args);
        let value = pages::usage_value(constant).expect("a declared constant");
        assert_eq!(
            output.status.code(),
            Some(devscout_rs::exit::EXIT_USAGE),
            "{args:?} does not exit with the usage status"
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            format!("{value}\n"),
            "{args:?} does not print {constant}"
        );
    }
    // The plumbing verbs report a usage error with the failure status instead.
    let plumbing: [(&[&str], &str); 4] = [
        (&["parse"], "PARSE_USAGE"),
        (&["spans"], "SPANS_USAGE"),
        (&["extract-dump"], "EXTRACT_DUMP_USAGE"),
        (&["hook"], "HOOK_USAGE"),
    ];
    for (args, constant) in plumbing {
        let output = run_binary(args);
        let value = pages::usage_value(constant).expect("a declared constant");
        assert_eq!(
            output.status.code(),
            Some(devscout_rs::exit::EXIT_FAILURE),
            "{args:?} does not exit with the failure status"
        );
        assert_eq!(
            String::from_utf8(output.stderr).expect("stderr is UTF-8"),
            format!("{value}\n"),
            "{args:?} does not print {constant}"
        );
    }
    let unknown = run_binary(&["frobnicate"]);
    assert_eq!(
        String::from_utf8(unknown.stderr).expect("stderr is UTF-8"),
        format!("{}\n", pages::usage_value("COMMANDS_USAGE").unwrap()),
        "an unknown command does not print the command list"
    );
    let group = run_binary(&["compiler-facts", "sideways"]);
    let expected = [
        "COMPILER_FACTS_USAGE",
        "COMPILER_FACTS_RUN_USAGE",
        "COMPILER_FACTS_IMPORT_USAGE",
        "COMPILER_FACTS_STATUS_USAGE",
    ]
    .iter()
    .map(|name| pages::usage_value(name).unwrap())
    .collect::<Vec<_>>()
    .join("\n");
    assert_eq!(
        String::from_utf8(group.stdout).expect("stdout is UTF-8"),
        format!("{expected}\n"),
        "an unknown compiler-facts sub-command does not print the group's usage lines"
    );
}

#[test]
fn docs_sync_ac_3_every_usage_literal_is_a_rendered_constant() {
    require_structure();
    let files = scan_sources(&manifest_dir());
    let rendered: BTreeSet<&str> = pages::USAGE_CONSTANTS
        .iter()
        .map(|(_, value)| *value)
        .collect();
    let found: BTreeSet<String> = facts::usage_literals(&files)
        .into_iter()
        .map(|literal| literal.detail)
        .collect();
    assert_eq!(
        found,
        rendered.iter().map(|value| value.to_string()).collect(),
        "the usage lines the source carries are not the ones the page renders"
    );
}

#[test]
fn docs_sync_ac_3_stray_usage_literal_fails_under_bless() {
    let tree = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "stray.rs",
            "const STRAY: &str = \"usage: devscout stray\";\n",
        ),
    ]);
    let failures = structural_failures(&tree);
    assert!(
        failures.iter().any(|failure| failure.contains("stray.rs")),
        "a usage line outside the usage module passed the structural checks: {failures:?}"
    );
    std::fs::remove_dir_all(&tree).ok();
}

#[test]
fn docs_sync_ac_4_environment_page_is_generated() {
    require_structure();
    let content = pages::environment_page();
    compare_page(&reference_dir().join("environment.md"), &content);
}

#[test]
fn docs_sync_ac_4_variables_checked_both_ways() {
    require_structure();
    let names = scanned_environment_names();
    for row in pages::ENVIRONMENT {
        assert!(
            names.contains(row.name),
            "the page documents {} and the source does not read it",
            row.name
        );
    }
    for name in &names {
        assert!(
            pages::ENVIRONMENT.iter().any(|row| row.name == name),
            "the source reads {name} and the page has no row for it"
        );
    }
}

#[test]
fn docs_sync_ac_4_names_the_variable() {
    // A name the source reads and the page does not document.
    let undocumented = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "reads.rs",
            "fn f() { let _ = std::env::var(\"SCOUT_UNDOCUMENTED\"); }\n",
        ),
    ]);
    let failures = structural_failures(&undocumented);
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("SCOUT_UNDOCUMENTED")),
        "an undocumented variable passed the checks: {failures:?}"
    );
    std::fs::remove_dir_all(&undocumented).ok();

    // A name the page documents and the source no longer reads.
    let documented = BTreeSet::from([String::from("HOME")]);
    assert!(
        !documented.contains("SCOUT_REGISTRY"),
        "a variable the source does not read is still documented"
    );

    // A retired name the source reads again.
    let retired = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "reads.rs",
            "fn f() { let _ = std::env::var(\"SCOUT_MTIME_REUSE\"); }\n",
        ),
    ]);
    let failures = structural_failures(&retired);
    assert!(
        failures
            .iter()
            .any(|failure| failure.contains("SCOUT_MTIME_REUSE")),
        "a retired variable read again passed the checks: {failures:?}"
    );
    std::fs::remove_dir_all(&retired).ok();

    // `HOME` no longer read.
    let no_home = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "reads.rs",
            "fn f() { let _ = std::env::var(\"SCOUT_DEBUG\"); }\n",
        ),
    ]);
    let failures = structural_failures(&no_home);
    assert!(
        failures.iter().any(|failure| failure.contains("HOME")),
        "a page documenting HOME while the source reads none passed the checks: {failures:?}"
    );
    std::fs::remove_dir_all(&no_home).ok();
}

#[test]
fn docs_sync_ac_5_exit_code_page_is_generated() {
    require_structure();
    let content = pages::exit_codes_page();
    compare_page(&reference_dir().join("exit-codes.md"), &content);
}

#[test]
fn docs_sync_ac_5_no_numeric_status_in_source() {
    require_structure();
    let files = scan_sources(&manifest_dir());
    let sites = facts::numeric_status_sites(&files);
    assert!(
        sites.is_empty(),
        "the source returns or exits with a bare status: {}",
        sites
            .iter()
            .map(facts::Located::describe)
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
fn docs_sync_ac_5_numeric_status_fails() {
    let cases: [(&str, &str); 5] = [
        ("exit.rs", "fn f() { process::exit(1); }\n"),
        (
            "tuple.rs",
            "fn f() -> (i32, String) { (2, String::new()) }\n",
        ),
        (
            "error.rs",
            "fn f() -> Result<(i32, String), String> { Err((1, String::new())) }\n",
        ),
        (
            "multiline.rs",
            "fn f() -> (i32, String) {\n    (\n        1,\n        String::new(),\n    )\n}\n",
        ),
        (
            "closure.rs",
            "fn f() -> (i32, String) { let g = || (2, String::new()); g() }\n",
        ),
    ];
    for (file, text) in cases {
        let tree = synthetic_tree(&[
            (
                "usage.rs",
                "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
            ),
            (file, text),
        ]);
        let files = scan_sources(&tree);
        assert!(
            !facts::numeric_status_sites(&files).is_empty(),
            "a bare status in {file} was not reported"
        );
        std::fs::remove_dir_all(&tree).ok();
    }
}

#[test]
fn docs_sync_ac_5_constants_are_all_described() {
    require_structure();
    let files = scan_sources(&manifest_dir());
    let scanned = facts::exit_constants(&files);
    let described: BTreeMap<String, i64> = pages::EXIT_CODES
        .iter()
        .map(|row| (row.constant.to_string(), row.value))
        .collect();
    assert_eq!(
        scanned, described,
        "the constants the exit module declares are not the ones the page describes"
    );
    assert_eq!(
        described.len(),
        4,
        "the exit-code page must describe one row per status"
    );
}

#[test]
fn docs_sync_ac_6_storage_page_is_generated() {
    require_structure();
    let content = pages::storage_page();
    compare_page(&reference_dir().join("storage.md"), &content);
}

#[test]
fn docs_sync_ac_6_listed_names_only_in_their_constant() {
    require_structure();
    let files = scan_sources(&manifest_dir());
    let listed = pages::listed_artifact_names();
    let offenders = facts::names_outside_their_constant(&files, "layout.rs", &listed);
    assert!(
        offenders.is_empty(),
        "an artifact name is spelled outside its constant: {}",
        offenders
            .iter()
            .map(facts::Located::describe)
            .collect::<Vec<_>>()
            .join(", ")
    );
    let page = pages::storage_page();
    for name in &listed {
        assert!(
            page.contains(name),
            "the storage page does not list the artifact {name}"
        );
    }
}

#[test]
fn docs_sync_ac_6_duplicate_name_literal_fails() {
    let tree = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "layout.rs",
            "pub const GRAPH_FILE: &str = \"graph.json\";\n",
        ),
        ("reads.rs", "fn f() -> &'static str { \"graph.json\" }\n"),
    ]);
    let files = scan_sources(&tree);
    let offenders =
        facts::names_outside_their_constant(&files, "layout.rs", &["graph.json".to_string()]);
    assert!(
        offenders.iter().any(|site| site.file.ends_with("reads.rs")),
        "a second literal for a listed artifact name was not reported"
    );
    std::fs::remove_dir_all(&tree).ok();
}

#[test]
fn docs_sync_ac_7_bless_creates_missing_pages() {
    let directory = temp_dir("bless");
    let help = stdout_of(&["--help"]);
    let mut written = Vec::new();
    for (name, content) in pages::pages(&help) {
        let path = directory.join(name);
        let outcome = sync::sync_page(&path, &content, true).expect("a bless writes the page");
        assert_eq!(outcome, sync::Outcome::Written);
        written.push((path, content));
    }
    for (path, content) in &written {
        assert!(path.exists(), "{} was not created", path.display());
        let observed = std::fs::read_to_string(path).expect("the page is readable");
        assert_eq!(&observed, content, "{} was not written", path.display());
        assert_eq!(
            sync::sync_page(path, content, false).expect("a fresh page is not stale"),
            sync::Outcome::Unchanged
        );
    }
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn docs_sync_ac_7_structural_failure_survives_bless() {
    let tree = synthetic_tree(&[
        (
            "usage.rs",
            "pub const ONE_USAGE: &str = \"usage: devscout one\";\n",
        ),
        (
            "stray.rs",
            "const STRAY: &str = \"usage: devscout stray\";\n",
        ),
        (
            "exit.rs",
            "fn f() -> (i32, String) { (1, String::new()) }\n",
        ),
        (
            "layout.rs",
            "pub const GRAPH_FILE: &str = \"graph.json\";\n",
        ),
        ("reads.rs", "fn f() -> &'static str { \"graph.json\" }\n"),
    ]);
    let failures = structural_failures(&tree);
    assert!(
        failures.len() >= 3,
        "a tree that breaks the usage, status and artifact rules passed under bless: {failures:?}"
    );
    std::fs::remove_dir_all(&tree).ok();
}

#[test]
fn docs_sync_ac_8_inbound_anchors_resolve() {
    require_structure();
    let help = stdout_of(&["--help"]);
    let rendered = pages::pages(&help);
    let names: Vec<&str> = PAGES.iter().map(|(name, _)| *name).collect();
    let links = anchors::inbound_links(&manifest_dir(), &names)
        .unwrap_or_else(|error| panic!("cannot collect inbound links: {error}"));
    assert!(
        !links.is_empty(),
        "no tracked Markdown file links into the generated pages"
    );
    let missing = anchors::missing_anchors(&rendered, &links);
    assert!(
        missing.is_empty(),
        "a generated page dropped a linked heading:\n  {}",
        missing.join("\n  ")
    );
}

#[test]
fn docs_sync_ac_8_dropped_heading_fails() {
    let page = pages::storage_page();
    let dropped = page.replace("## Ordering", "## Where things are ordered");
    let link = anchors::InboundLink {
        from: PathBuf::from("docs/limitations.md"),
        target: "storage.md".to_string(),
        anchor: "ordering".to_string(),
    };
    let missing = anchors::missing_anchors(&[("storage.md", dropped)], &[link]);
    assert_eq!(missing.len(), 1, "a dropped heading was not reported");
}

#[test]
fn docs_sync_pages_open_with_their_title_and_the_notice() {
    let help = stdout_of(&["--help"]);
    for (name, title) in PAGES {
        let content = pages::pages(&help)
            .into_iter()
            .find(|(page, _)| *page == name)
            .map(|(_, content)| content)
            .expect("every page is rendered");
        assert!(
            content.starts_with(&format!("# {title}\n")),
            "{name} does not open with its own title"
        );
        assert!(
            content.contains("Generated."),
            "{name} does not say it is generated"
        );
        assert!(
            content.contains(pages::REGENERATE),
            "{name} does not name the regenerate command"
        );
    }
}

// ------------------------------------------------------------------ support

/// A temporary directory no other check shares: the process id and a counter
/// of this process's own, so two checks running at once cannot collide.
fn temp_dir(purpose: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!(
        "devscout-docs-sync-{purpose}-{}-{serial}",
        std::process::id()
    ));
    std::fs::create_dir_all(&base).expect("a temporary directory is writable");
    base
}

/// A source tree holding exactly the files given, for a case that has to fail.
fn synthetic_tree(files: &[(&str, &str)]) -> PathBuf {
    let root = temp_dir("tree");
    let source = root.join("src");
    std::fs::create_dir_all(&source).expect("a temporary source tree is writable");
    for (name, text) in files {
        std::fs::write(source.join(name), text).expect("a synthetic file is writable");
    }
    root
}
