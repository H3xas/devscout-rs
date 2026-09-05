//! Pins the C# construct catalogue in `docs/csharp-coverage.md` to the fixture under
//! `fixtures/csharp-syntax/`.
//!
//! Three layers, each its own test:
//!
//! 1. Grammar: every fixture file parses with the pinned `tree-sitter-c-sharp` without a
//!    single `ERROR` or `MISSING` node, so a construct that stops producing facts after a
//!    grammar bump is classified as a grammar gap rather than an extractor gap.
//! 2. Sync: every fixture file is named in the catalogue and every fixture the catalogue
//!    names exists, so neither side can drift without the other noticing.
//! 3. Extraction: after `devscout init --no-hooks` over the fixture, every catalogue row
//!    marked `must` + `produces` still has its declaration, name, or edge in
//!    `.scout/graph/graph.json`. Rows the catalogue marks `silent`, `partial`, or `leaks`
//!    are deliberately not asserted here in either direction; the catalogue's follow-up
//!    column is their only record.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use tree_sitter::{Node, Parser};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/csharp-syntax")
}

fn catalogue_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/csharp-coverage.md")
}

/// Every `.cs` file of the fixture, sorted by file name.
fn fixture_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixture_dir())
        .expect("fixtures/csharp-syntax exists")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "cs"))
        .collect();
    files.sort();
    assert!(
        files.len() >= 25,
        "expected at least 25 fixture files, found {}",
        files.len()
    );
    files
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------
// Layer 1: grammar
// ---------------------------------------------------------------------------

/// Fixture files whose `ERROR`/`MISSING` node count is known and documented in the
/// catalogue header. Empty while every fixture parses clean; a grammar bump that breaks a
/// file is recorded here together with the catalogue row, never silently.
const GRAMMAR_GAPS: &[(&str, usize)] = &[("GrammarGaps.cs", 2)];

fn parse_defects(source: &str) -> Vec<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_c_sharp::LANGUAGE.into())
        .expect("C# grammar loads");
    let tree = parser.parse(source, None).expect("parse returns a tree");
    let mut defects = Vec::new();
    collect_defects(tree.root_node(), &mut defects);
    defects
}

fn collect_defects(node: Node, defects: &mut Vec<String>) {
    if node.is_error() || node.is_missing() {
        let label = if node.is_missing() {
            "MISSING"
        } else {
            "ERROR"
        };
        let point = node.start_position();
        defects.push(format!(
            "{label} {} at {}:{}",
            node.kind(),
            point.row + 1,
            point.column + 1
        ));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_defects(child, defects);
    }
}

#[test]
fn every_fixture_file_parses_without_error_or_missing_nodes() {
    let mut report = Vec::new();
    for path in fixture_files() {
        let name = file_name(&path);
        let source = fs::read_to_string(&path).unwrap();
        let defects = parse_defects(&source);
        let allowed = GRAMMAR_GAPS
            .iter()
            .find(|(file, _)| *file == name)
            .map_or(0, |(_, count)| *count);
        if defects.len() != allowed {
            report.push(format!(
                "{name}: {} defect(s), {allowed} documented:\n    {}",
                defects.len(),
                defects.join("\n    ")
            ));
        }
    }
    assert!(
        report.is_empty(),
        "grammar pin failed:\n{}",
        report.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Layer 2: catalogue <-> fixture sync
// ---------------------------------------------------------------------------

#[test]
fn the_catalogue_names_every_fixture_file_and_every_named_fixture_exists() {
    let catalogue = fs::read_to_string(catalogue_path()).expect("docs/csharp-coverage.md exists");
    let on_disk: BTreeSet<String> = fixture_files().iter().map(|p| file_name(p)).collect();

    let unlisted: Vec<&String> = on_disk
        .iter()
        .filter(|name| !catalogue.contains(name.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "fixture files missing from docs/csharp-coverage.md: {unlisted:?}"
    );

    let named: BTreeSet<String> = catalogue
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | '`' | ',' | ';' | '(' | ')'))
        .filter(|token| token.ends_with(".cs"))
        .map(str::to_string)
        .collect();
    let missing: Vec<&String> = named
        .iter()
        .filter(|name| !on_disk.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/csharp-coverage.md names fixture files that do not exist: {missing:?}"
    );
}

// ---------------------------------------------------------------------------
// Layer 3: extraction
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
    registry: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "devscout-csharp-syntax-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        for path in fixture_files() {
            fs::copy(&path, root.join(file_name(&path))).unwrap();
        }
        let registry = root.join("registry.json");
        let fixture = Self { root, registry };
        fixture.ok(&["init", "--no-hooks"]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_devscout"))
            .current_dir(&self.root)
            .env("SCOUT_REGISTRY", &self.registry)
            .env("HOME", &self.root)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn graph(&self) -> serde_json::Value {
        let path = self.root.join(".scout/graph/graph.json");
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Which heuristic tier an expected `uses-member` edge may carry.
#[derive(Clone, Copy, PartialEq)]
enum Tier {
    /// No `tier` field: the receiver was typed by syntax.
    Precise,
    /// The extension-method tier.
    Ext,
    /// Any tier, including the scored guess: the fact reaches the graph, the receiver may
    /// not be typed.
    Any,
}

/// An edge the catalogue marks `must` + `produces`: fixture file, line (0 = any line),
/// edge kind, target, member (empty = none), tier.
struct Edge(
    &'static str,
    usize,
    &'static str,
    &'static str,
    &'static str,
    Tier,
);

fn str_field<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn edge_matches(edge: &serde_json::Value, want: &Edge) -> bool {
    let Edge(file, line, kind, to, member, tier) = want;
    let actual_tier = str_field(edge, "tier");
    let tier_ok = match tier {
        Tier::Precise => actual_tier.is_empty(),
        Tier::Ext => actual_tier == "ext",
        Tier::Any => true,
    };
    let target = if *kind == "imports" { "target" } else { "to" };
    str_field(edge, "from_file") == *file
        && (*line == 0 || edge["from_line"].as_u64() == Some(*line as u64))
        && str_field(edge, "kind") == *kind
        && str_field(edge, target) == *to
        && str_field(edge, "member") == *member
        && tier_ok
}

fn describe_edge(want: &Edge) -> String {
    let Edge(file, line, kind, to, member, tier) = want;
    let tier = match tier {
        Tier::Precise => "precise",
        Tier::Ext => "ext",
        Tier::Any => "any tier",
    };
    let member = if member.is_empty() {
        String::new()
    } else {
        format!(" member {member}")
    };
    let line = if *line == 0 {
        String::new()
    } else {
        format!(":{line}")
    };
    format!("{file}{line} {kind} -> {to}{member} ({tier})")
}

/// Declarations the catalogue marks `must` and that produce at base: fixture file, def id,
/// def kind. The leading comment names the catalogue row.
#[rustfmt::skip]
const DEFS: &[(&str, &str, &str)] = &[
    // D01: Block namespace, dotted block namespace
    ("NamespaceBlock.cs", "Syntax.Blocks.Inner.InnerHost", "class"),
    ("NamespaceBlock.cs", "Syntax.Blocks.BlockHost", "class"),
    // D02: File-scoped namespace
    ("NamespaceFileScoped.cs", "Syntax.Scoped.ScopedHost", "class"),
    // D03: class (incl. abstract, sealed, static, file modifiers)
    ("TypeKinds.cs", "Syntax.Kinds.KindClass", "class"),
    ("TypeKinds.cs", "Syntax.Kinds.KindAbstract", "class"),
    ("TypeKinds.cs", "Syntax.Kinds.KindStatic", "class"),
    ("TypeKinds.cs", "Syntax.Kinds.KindFileLocal", "class"),
    ("TypeKinds.cs", "Syntax.Kinds.KindSealed", "class"),
    // D04: struct, readonly struct, ref struct
    ("TypeKinds.cs", "Syntax.Kinds.KindStruct", "struct"),
    ("TypeKinds.cs", "Syntax.Kinds.KindRefStruct", "struct"),
    // D05: record, record class
    ("TypeKinds.cs", "Syntax.Kinds.KindRecord", "record"),
    ("TypeKinds.cs", "Syntax.Kinds.KindRecordClass", "record"),
    // D06: record struct, readonly record struct
    ("TypeKinds.cs", "Syntax.Kinds.KindRecordStruct", "record"),
    ("TypeKinds.cs", "Syntax.Kinds.KindReadonlyRecordStruct", "record"),
    // D07: interface
    ("TypeKinds.cs", "Syntax.Kinds.IKindInterface", "interface"),
    // D08: enum and enum members (incl. explicit values, base type, [Flags])
    ("TypeKinds.cs", "Syntax.Kinds.KindEnum", "enum"),
    ("TypeKinds.cs", "Syntax.Kinds.KindEnum.Beta", "enum-member"),
    ("TypeKinds.cs", "Syntax.Kinds.KindFlags", "enum"),
    // D09: delegate
    ("TypeKinds.cs", "Syntax.Kinds.KindDelegate", "delegate"),
    // D10: Nested types of every kind, multi-level nesting
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestInner", "class"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestMode", "enum"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestPoint", "struct"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+INestHook", "interface"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestRecord", "record"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestCallback", "delegate"),
    ("NestedTypes.cs", "Syntax.Nesting.NestOuter+NestInner2+NestDeep", "class"),
    // D12: Partial type across files (class, struct, interface, record)
    ("PartialTypesA.cs", "Syntax.Partial.PartialHost", "class"),
    ("PartialTypesA.cs", "Syntax.Partial.PartialPoint", "struct"),
    ("PartialTypesA.cs", "Syntax.Partial.IPartialHook", "interface"),
    ("PartialTypesA.cs", "Syntax.Partial.PartialRecord", "record"),
    // D15: Generic type parameters and arity (same name, different arity)
    ("Generics.cs", "Syntax.Generics.GenericBox", "class"),
    ("Generics.cs", "Syntax.Generics.GenericPair", "class"),
    // D19: Record primary constructor (parameters as properties)
    ("PrimaryConstructors.cs", "Syntax.Primary.PrimaryRecord", "record"),
    // D20: Class/struct primary constructor (C# 12), captured parameter
    ("PrimaryConstructors.cs", "Syntax.Primary.PrimaryService", "class"),
    // D22: Static class
    ("StaticAndExtension.cs", "Syntax.Statics.StaticOnly", "class"),
    // D47: Types declared after top-level statements
    ("Program.cs", "Syntax.TopLevel.TopLevelHost", "class"),
    ("Program.cs", "Syntax.TopLevel.TopLevelHelper", "class"),
    // R86: `ref struct`, `ref field`, `scoped`, `readonly ref struct`
    ("RefAndUnsafe.cs", "Syntax.Memory.RefBuffer", "struct"),
    ("RefAndUnsafe.cs", "Syntax.Memory.RefView", "struct"),
    // R92: Verbatim identifiers `@class`, `@event`
    ("StringsAndTrivia.cs", "Syntax.Trivia.@class", "class"),
    // R93: Non-ASCII identifiers and strings
    ("StringsAndTrivia.cs", "Syntax.Trivia.Zażółć", "class"),
];

/// Member names the catalogue marks `must` and that produce at base: fixture file, owner def
/// id, name, name kind.
#[rustfmt::skip]
const NAMES: &[(&str, &str, &str, &str)] = &[
    // D13: Partial method (declaration + implementation parts)
    ("PartialTypesA.cs", "Syntax.Partial.PartialHost", "OnLoaded", "method"),
    ("PartialTypesB.cs", "Syntax.Partial.PartialHost", "OnLoaded", "method"),
    // D14: Partial property (C# 13)
    ("PartialTypesA.cs", "Syntax.Partial.PartialHost", "Count", "property"),
    ("PartialTypesB.cs", "Syntax.Partial.PartialHost", "Count", "property"),
    // D23: Extension method declaration (`this` parameter)
    ("StaticAndExtension.cs", "Syntax.Statics.TextExtensions", "Shout", "method"),
    ("StaticAndExtension.cs", "Syntax.Statics.TargetExtensions", "Ping", "method"),
    // D26: Fields: instance, static, readonly, const, volatile, multiple declarators
    ("Members.cs", "Syntax.Members.MemberHost", "InstanceField", "field"),
    ("Members.cs", "Syntax.Members.MemberHost", "StaticField", "field"),
    ("Members.cs", "Syntax.Members.MemberHost", "Limit", "field"),
    ("Members.cs", "Syntax.Members.MemberHost", "VolatileField", "field"),
    ("Members.cs", "Syntax.Members.MemberHost", "Left", "field"),
    ("Members.cs", "Syntax.Members.MemberHost", "Right", "field"),
    // D27: Properties: auto, backed, get-only, expression-bodied, private set
    ("Members.cs", "Syntax.Members.MemberHost", "AutoProperty", "property"),
    ("Members.cs", "Syntax.Members.MemberHost", "ExpressionProperty", "property"),
    // D28: `init` and `required` properties
    ("Members.cs", "Syntax.Members.MemberHost", "InitProperty", "property"),
    ("Members.cs", "Syntax.Members.MemberHost", "RequiredProperty", "property"),
    // D29: `field` keyword accessor (C# 13 preview)
    ("Members.cs", "Syntax.Members.MemberHost", "Age", "property"),
    // D31: Field-like event
    ("Members.cs", "Syntax.Members.MemberHost", "Changed", "event"),
    // D32: Event with add/remove accessors
    ("Members.cs", "Syntax.Members.MemberHost", "Clicked", "event"),
    // D36: Methods: instance, static, async, expression-bodied, virtual, override, abstract, new, protected internal
    ("Members.cs", "Syntax.Members.MemberHost", "AsyncMethod", "method"),
    ("Members.cs", "Syntax.Members.MemberHost", "StaticMethod", "method"),
    ("Members.cs", "Syntax.Members.MemberHost", "Describe", "method"),
    ("Members.cs", "Syntax.Members.MemberBase", "Describe", "method"),
    ("Members.cs", "Syntax.Members.MemberHost", "ExpressionMethod", "method"),
    ("Members.cs", "Syntax.Members.MemberHost", "ProtectedInternalMethod", "method"),
    ("Members.cs", "Syntax.Members.MemberHost", "Hide", "method"),
    // D37: Method overloads (arity)
    ("Members.cs", "Syntax.Members.MemberHost", "Add", "method"),
    // D49: Default interface member body
    ("InterfaceMembers.cs", "Syntax.Interfaces.IShapeContract", "Perimeter", "method"),
    // D50: Static abstract / static virtual interface members
    ("InterfaceMembers.cs", "Syntax.Interfaces.IShapeContract", "Create", "method"),
    ("InterfaceMembers.cs", "Syntax.Interfaces.IShapeContract", "Kind", "property"),
    // D51: Interface static field and static method
    ("InterfaceMembers.cs", "Syntax.Interfaces.IShapeContract", "Counter", "field"),
    // R85: `params T[]` and `params ReadOnlySpan<T>` (C# 13)
    ("RefAndUnsafe.cs", "Syntax.Memory.MemUser", "Sum", "method"),
    ("RefAndUnsafe.cs", "Syntax.Memory.MemUser", "SumSpan", "method"),
];

/// Edges the catalogue marks `must` and that produce at base (for a `partial` row, only the
/// shape that produces). One entry per line on purpose: the table is read by row.
#[rustfmt::skip]
const EDGES: &[Edge] = &[
    // D01: Block namespace, dotted block namespace
    Edge("NamespaceBlock.cs", 14, "imports", "Syntax.Blocks", "", Tier::Precise),
    // D11: Nested type referenced through its outer type (`Outer.Inner`, `new Outer.A.B()`)
    Edge("NestedTypes.cs", 43, "uses-type", "Syntax.Nesting.NestOuter+NestInner", "", Tier::Precise),
    Edge("NestedTypes.cs", 51, "uses-type", "Syntax.Nesting.NestOuter+NestInner2+NestDeep", "", Tier::Precise),
    Edge("NestedTypes.cs", 53, "uses-member", "Syntax.Nesting.NestOuter+NestMode.Fast", "Fast", Tier::Precise),
    // D17: Generic method (explicit and inferred type arguments at the call)
    Edge("Generics.cs", 99, "uses-member", "Syntax.Generics.GenericMethods", "Convert", Tier::Precise),
    Edge("Generics.cs", 102, "uses-member", "Syntax.Generics.GenericMethods", "UseSelfReferential", Tier::Precise),
    // D20: Class/struct primary constructor (C# 12), captured parameter
    Edge("PrimaryConstructors.cs", 15, "uses-member", "Syntax.Primary.PrimaryDependency", "Describe", Tier::Precise),
    // D21: Primary constructor base call `: Base(args)`
    Edge("PrimaryConstructors.cs", 6, "inherits", "Syntax.Primary.PrimaryRecord", "", Tier::Precise),
    Edge("PrimaryConstructors.cs", 28, "inherits", "Syntax.Primary.PrimaryBase", "", Tier::Precise),
    // D22: Static class
    Edge("StaticAndExtension.cs", 46, "uses-member", "Syntax.Statics.StaticOnly", "Reset", Tier::Precise),
    // D24: Extension method call on a typed receiver
    Edge("StaticAndExtension.cs", 45, "uses-member", "Syntax.Statics.TargetExtensions", "Ping", Tier::Ext),
    // D25: Extension method called as a static method
    Edge("StaticAndExtension.cs", 47, "uses-member", "Syntax.Statics.TextExtensions", "Shout", Tier::Precise),
    // D29: `field` keyword accessor (C# 13 preview)
    Edge("Members.cs", 135, "uses-member", "Syntax.Members.MemberHost", "Age", Tier::Precise),
    // D34: Constructor parameters as injection seams (`ctor-di`)
    Edge("Members.cs", 74, "ctor-di", "Syntax.Members.MemberDependency", "", Tier::Precise),
    // D38: Return and parameter types of methods
    Edge("Members.cs", 74, "uses-type", "Syntax.Members.IMemberDependency", "", Tier::Precise),
    Edge("Members.cs", 79, "uses-type", "Syntax.Members.IMemberDependency", "", Tier::Precise),
    // D45: References inside a local function body
    Edge("LocalFunctions.cs", 21, "uses-member", "Syntax.Locals.LocalItem", "Score", Tier::Precise),
    Edge("LocalFunctions.cs", 22, "uses-member", "Syntax.Locals.LocalItem", "Value", Tier::Precise),
    // D46: Top-level statements
    Edge("Program.cs", 4, "uses-type", "Syntax.TopLevel.TopLevelHost", "", Tier::Precise),
    Edge("Program.cs", 6, "uses-member", "Syntax.TopLevel.TopLevelHelper", "Answer", Tier::Precise),
    // D51: Interface static field and static method
    Edge("InterfaceMembers.cs", 66, "uses-member", "Syntax.Interfaces.IShapeContract", "Bump", Tier::Precise),
    // U01: `using Ns;`
    Edge("Program.cs", 2, "imports", "Syntax.TopLevel", "", Tier::Precise),
    // U02: `using static T;`
    Edge("Usings.cs", 5, "imports", "Syntax.Usings.UsingStatics", "", Tier::Precise),
    // U03: `using Alias = T;`
    Edge("Usings.cs", 6, "imports", "System.Text.StringBuilder", "", Tier::Precise),
    // U05: `global using` (plain, static, alias)
    Edge("Usings.cs", 2, "imports", "System.Collections.Concurrent", "", Tier::Precise),
    Edge("Usings.cs", 3, "imports", "System.Math", "", Tier::Precise),
    Edge("Usings.cs", 4, "imports", "System.Text.StringBuilder", "", Tier::Precise),
    // U07: Base list: class base and interface list
    Edge("TypeKinds.cs", 4, "inherits", "Syntax.Kinds.IKindInterface", "", Tier::Precise),
    Edge("InterfaceMembers.cs", 25, "inherits", "Syntax.Interfaces.IShapeContract", "", Tier::Precise),
    // U08: Type annotations on fields, properties, parameters, returns, locals
    Edge("Members.cs", 74, "uses-type", "Syntax.Members.IMemberDependency", "", Tier::Precise),
    Edge("Members.cs", 79, "uses-type", "Syntax.Members.IMemberDependency", "", Tier::Precise),
    // U09: Generic type arguments in annotations and creations
    Edge("Generics.cs", 88, "uses-type", "Syntax.Generics.GenericItem", "", Tier::Precise),
    Edge("Generics.cs", 94, "uses-type", "Syntax.Generics.GenericItem", "", Tier::Precise),
    // U10: Nullable and array annotations (`T?`, `T[]`, `T[][]`)
    Edge("MemberAccess.cs", 8, "uses-type", "Syntax.Access.AccessTarget", "", Tier::Precise),
    // U11: Tuple type annotation (`(int Id, string Name)`)
    Edge("TuplesAndWith.cs", 24, "uses-type", "Syntax.Tuples.TupleRecord", "", Tier::Precise),
    // R01: Member access on a field / property / parameter / local receiver
    Edge("MemberAccess.cs", 33, "uses-member", "Syntax.Access.AccessTarget", "Count", Tier::Precise),
    Edge("MemberAccess.cs", 34, "uses-member", "Syntax.Access.AccessTarget", "Name", Tier::Precise),
    // R02: Invocation `a.M()`
    Edge("MemberAccess.cs", 35, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R03: Generic invocation `a.M<T>()`
    Edge("MemberAccess.cs", 36, "uses-member", "Syntax.Access.AccessTarget", "Generic", Tier::Precise),
    // R04: Static member access `T.M`, `T.M()`
    Edge("MemberAccess.cs", 45, "uses-member", "Syntax.Access.AccessTarget", "Shared", Tier::Precise),
    // R05: `this.M`
    Edge("MemberAccess.cs", 41, "uses-member", "Syntax.Access.AccessUser", "Run2", Tier::Precise),
    Edge("MemberAccess.cs", 42, "uses-member", "Syntax.Access.AccessBase", "BaseField", Tier::Precise),
    // R06: `base.M()`
    Edge("MemberAccess.cs", 43, "uses-member", "Syntax.Access.AccessBase", "Hook", Tier::Precise),
    // R08: Conditional access `a?.M()`
    Edge("MemberAccess.cs", 37, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R09: Chained conditional access `a?.B?.C`
    Edge("MemberAccess.cs", 38, "uses-member", "Syntax.Access.AccessTarget", "Next", Tier::Precise),
    // R12: One-hop call chain `a.M().N()` and the typed hop of `a.B.M()`
    Edge("MemberAccess.cs", 61, "uses-member", "Syntax.Access.AccessTarget", "Next", Tier::Precise),
    Edge("MemberAccess.cs", 63, "uses-member", "Syntax.Access.AccessTarget", "Self", Tier::Precise),
    Edge("MemberAccess.cs", 63, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R17: Method group `Action a = t.M;`
    Edge("MemberAccess.cs", 47, "uses-member", "Syntax.Access.AccessTarget", "Hook2", Tier::Precise),
    // R19: Receiver typed by `var x = new T()`
    Edge("MemberAccess.cs", 54, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R20: Receiver typed by explicit local type `T x = ...`
    Edge("MemberAccess.cs", 56, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R21: Receiver typed by a parameter, property, or static property (field: R01)
    Edge("MemberAccess.cs", 59, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    Edge("MemberAccess.cs", 60, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    Edge("MemberAccess.cs", 68, "uses-member", "Syntax.Access.AccessTarget", "Compute", Tier::Precise),
    // R22: Object creation `new T()`, `new T(args)`, generic `new T<U>()`
    Edge("ObjectCreation.cs", 27, "uses-type", "Syntax.Creation.CreatedItem", "", Tier::Precise),
    Edge("ObjectCreation.cs", 43, "uses-type", "Syntax.Creation.CreatedItem", "", Tier::Precise),
    Edge("ObjectCreation.cs", 28, "uses-type", "Syntax.Creation.CreatedItem", "", Tier::Precise),
    // R23: Target-typed `new()` on a typed local / field / argument
    Edge("ObjectCreation.cs", 30, "uses-member", "Syntax.Creation.CreatedItem", "Tags", Tier::Precise),
    // R34: `typeof(T)`, `typeof(T[])`
    Edge("TypeOperators.cs", 19, "uses-type", "Syntax.TypeOps.TypeOpTarget", "", Tier::Precise),
    Edge("TypeOperators.cs", 24, "uses-type", "Syntax.TypeOps.TypeOpTarget", "", Tier::Precise),
    // R35: `typeof(T<>)` unbound and `typeof(T<int>)` closed on an in-graph generic
    Edge("TypeOperators.cs", 21, "uses-type", "Syntax.TypeOps.TypeOpGeneric", "", Tier::Precise),
    // R39: Cast typing a local `var x = (T)o`
    Edge("TypeOperators.cs", 52, "uses-member", "Syntax.TypeOps.TypeOpTarget", "Value", Tier::Precise),
    // R41: Declaration pattern `x is T t` typing `t`
    Edge("TypeOperators.cs", 38, "uses-member", "Syntax.TypeOps.TypeOpTarget", "Value", Tier::Precise),
    // R47: Switch expression arms (incl. `when`, `_`) walked
    Edge("Patterns.cs", 68, "uses-member", "Syntax.Patterns.PatternShape", "Area", Tier::Precise),
    // R48: Switch statement sections with patterns walked
    Edge("Patterns.cs", 79, "uses-member", "Syntax.Patterns.PatternShape", "Area", Tier::Precise),
    Edge("Patterns.cs", 80, "uses-member", "Syntax.Patterns.PatternShape", "Area", Tier::Precise),
    // R49: Enum member in a pattern or case label
    Edge("Patterns.cs", 52, "uses-member", "Syntax.Patterns.PatternKind.A", "A", Tier::Precise),
    Edge("Patterns.cs", 53, "uses-member", "Syntax.Patterns.PatternKind.A", "A", Tier::Precise),
    // R50: Lambda with a single implicit parameter typed from the receiver (`xs.Where(x => x.M())`)
    Edge("Lambdas.cs", 27, "uses-member", "Syntax.Lambdas.LambdaItem", "Score", Tier::Precise),
    Edge("Lambdas.cs", 28, "uses-member", "Syntax.Lambdas.LambdaItem", "Owner", Tier::Precise),
    // R52: Lambda with an explicitly typed parameter `(T x) => x.M()`
    Edge("Lambdas.cs", 29, "uses-member", "Syntax.Lambdas.LambdaItem", "Score", Tier::Precise),
    // R53: Lambda body references (block and expression bodies)
    Edge("Lambdas.cs", 68, "uses-member", "Syntax.Lambdas.LambdaOwner", "Rank", Tier::Precise),
    // R54: Anonymous method `delegate (T i) { }`
    Edge("Lambdas.cs", 30, "uses-member", "Syntax.Lambdas.LambdaItem", "Score", Tier::Precise),
    // R67: `var x = await M()` typing `x` from `Task<T>`
    Edge("AsyncAndYield.cs", 44, "uses-member", "Syntax.Async.AsyncItem", "Score", Tier::Precise),
    // R69: `await using var r = new R()`
    Edge("AsyncAndYield.cs", 50, "uses-member", "Syntax.Async.AsyncRes", "Touch", Tier::Precise),
    Edge("AsyncAndYield.cs", 54, "uses-member", "Syntax.Async.AsyncRes", "Touch", Tier::Precise),
    // R70: `await foreach (var x in M())` and typed `await foreach (T x in ...)`
    Edge("AsyncAndYield.cs", 64, "uses-member", "Syntax.Async.AsyncItem", "Score", Tier::Precise),
    // R72: Async lambda, `async void`, `Task.Run(async () => ...)`
    Edge("AsyncAndYield.cs", 72, "uses-member", "Syntax.Async.AsyncService", "GetAsync", Tier::Precise),
    Edge("AsyncAndYield.cs", 83, "uses-member", "Syntax.Async.AsyncService", "GetAsync", Tier::Precise),
    // R73: `foreach (var x in xs)` typing `x` from the element type
    Edge("StatementsAndScopes.cs", 185, "uses-member", "Syntax.Statements.StmtRes", "Touch", Tier::Precise),
    // R74: `foreach (T x in xs)` explicit element type (typed loop variable; no uses-type for the annotation)
    Edge("StatementsAndScopes.cs", 190, "uses-member", "Syntax.Statements.StmtRes", "Touch", Tier::Precise),
    // R75: Control flow bodies walked (if/else, for, while, do, switch, try, labels, goto)
    Edge("StatementsAndScopes.cs", 34, "uses-member", "Syntax.Statements.StmtGate", "Sync", Tier::Precise),
    // R77: `throw new T()`, throw expression `?? throw`, rethrow
    Edge("StatementsAndScopes.cs", 100, "uses-type", "Syntax.Statements.StmtError", "", Tier::Precise),
    Edge("StatementsAndScopes.cs", 125, "uses-type", "Syntax.Statements.StmtError", "", Tier::Precise),
    // R78: `lock (x)` target walked (any tier: the local comes from `??`)
    Edge("StatementsAndScopes.cs", 126, "uses-member", "Syntax.Statements.StmtGate", "Sync", Tier::Any),
    // R79: `using (var r = new R())` and `using var r = new R();` typing `r`
    Edge("StatementsAndScopes.cs", 133, "uses-member", "Syntax.Statements.StmtRes", "Touch", Tier::Precise),
    Edge("StatementsAndScopes.cs", 137, "uses-member", "Syntax.Statements.StmtRes", "Touch", Tier::Precise),
    // R82: `ref`/`in`/`out` parameters and arguments
    Edge("RefAndUnsafe.cs", 42, "uses-type", "Syntax.Memory.MemPoint", "", Tier::Precise),
    Edge("RefAndUnsafe.cs", 46, "uses-type", "Syntax.Memory.MemPoint", "", Tier::Precise),
    // R83: `out var x` / `out T x` typing `x`
    Edge("RefAndUnsafe.cs", 70, "uses-member", "Syntax.Memory.MemPoint", "Y", Tier::Precise),
    // R91: Interpolated string holes (`$"{a.B} {a.M()}"`, raw `$$"""`)
    Edge("StringsAndTrivia.cs", 51, "uses-member", "Syntax.Trivia.TriviaHost", "Name", Tier::Precise),
    Edge("StringsAndTrivia.cs", 51, "uses-member", "Syntax.Trivia.TriviaHost", "Describe", Tier::Precise),
    Edge("StringsAndTrivia.cs", 51, "uses-member", "Syntax.Trivia.TriviaHost", "Value", Tier::Precise),
    // R92: Verbatim identifiers `@class`, `@event`
    Edge("StringsAndTrivia.cs", 71, "uses-member", "Syntax.Trivia.@class", "@event", Tier::Precise),
    // R93: Non-ASCII identifiers and strings
    Edge("StringsAndTrivia.cs", 97, "uses-member", "Syntax.Trivia.Zażółć", "Wartość", Tier::Precise),
    // T01: `#if` / `#elif` / `#else` / `#endif` live branch walked
    Edge("Preprocessor.cs", 7, "imports", "System.Text", "", Tier::Precise),
    Edge("Preprocessor.cs", 56, "uses-member", "Syntax.Preproc.PreprocMarker", "Compound", Tier::Precise),
    // T04: Nested `#if` and `&&`/`!` conditions
    Edge("Preprocessor.cs", 62, "uses-member", "Syntax.Preproc.PreprocMarker", "Nested", Tier::Precise),
];

#[test]
fn every_must_row_that_produces_at_base_keeps_producing() {
    let fixture = Fixture::new();
    let graph = fixture.graph();
    let defs = graph["defs"].as_array().unwrap();
    let names = graph["names"].as_array().unwrap();
    let edges = graph["edges"].as_array().unwrap();
    let mut missing = Vec::new();

    for (file, id, kind) in DEFS {
        let found = defs.iter().any(|def| {
            str_field(def, "file") == *file
                && str_field(def, "id") == *id
                && str_field(def, "kind") == *kind
        });
        if !found {
            missing.push(format!("def {file} {id} ({kind})"));
        }
    }

    for (file, owner, name, kind) in NAMES {
        let found = names.iter().any(|entry| {
            str_field(entry, "file") == *file
                && str_field(entry, "owner") == *owner
                && str_field(entry, "name") == *name
                && str_field(entry, "kind") == *kind
        });
        if !found {
            missing.push(format!("name {file} {owner}::{name} ({kind})"));
        }
    }

    for want in EDGES {
        if !edges.iter().any(|edge| edge_matches(edge, want)) {
            missing.push(format!("edge {}", describe_edge(want)));
        }
    }

    assert!(
        missing.is_empty(),
        "{} catalogue row(s) stopped producing their fact:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

#[test]
fn partial_types_merge_into_one_declaration_with_every_site() {
    let fixture = Fixture::new();
    let graph = fixture.graph();
    let host = graph["defs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|def| str_field(def, "id") == "Syntax.Partial.PartialHost")
        .expect("PartialHost is declared");
    assert_eq!(str_field(host, "file"), "PartialTypesA.cs");
    assert_eq!(
        host["also_in"][0]["file"].as_str(),
        Some("PartialTypesB.cs"),
        "{host}"
    );
    let methods: Vec<&str> = host["methods"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m.as_str())
        .collect();
    assert!(
        methods.contains(&"Alpha") && methods.contains(&"Beta"),
        "{methods:?}"
    );
}
