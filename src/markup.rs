// The one source read in this crate that is not a tree-sitter parse. Markup and
// resource files carry names no C# declaration does (`x:Class`, `x:Name`, a
// `.resw`/`.resx` key), and both grammars are too small to justify a parser
// dependency, so the scan is a literal-prefix walk over each line.
//
// It is a hand-rolled scan rather than a regex so the exact match set is fixed
// and self-evident. The scan records declarations, instantiations and bindings
// but never widens how it reads -- still positional, still literal-anchored,
// still "read to the next `"` on the SAME line" -- so an attribute split across
// lines is simply not recorded.

use crate::extract::NameRecord;

// `.resx` is .NET's desktop/web resource format; `.resw` is its WinRT
// counterpart. Both carry the identical `<data name="Key" ...>` schema, so
// `.resx` gets the exact same treatment as `.resw` everywhere below -- same
// scan function, same resource-key kind, same tier.
pub fn is_markup(rel: &str) -> bool {
    rel.ends_with(".xaml") || is_resource(rel)
}

fn is_resource(rel: &str) -> bool {
    rel.ends_with(".resw") || rel.ends_with(".resx")
}

const RESW_KEY_ATTR: &str = "name=\"";

const X_CLASS_ATTR: &str = "x:Class=\"";
const X_NAME_ATTR: &str = "x:Name=\"";
const XMLNS_PREFIX: &str = "xmlns:";
const BINDING_OPEN: &str = "{Binding";
const XBIND_OPEN: &str = "{x:Bind";
const PATH_PROPERTY: &str = "Path=";

// The two spellings a XAML prefix declaration uses for a CLR namespace:
// WPF/Xamarin write `clr-namespace:Ns[;assembly=A]`, WinUI/UWP write
// `using:Ns`. Anything else (a schema URL, a design-time prefix) declares a
// namespace this index cannot name a type in, so its elements are skipped
// rather than guessed at.
const CLR_NAMESPACE: &str = "clr-namespace:";
const USING_NAMESPACE: &str = "using:";

/// One markup-declared type: the `x:Class` value, already split the way a C#
/// def is. This is the `defs` entry `markup_facts` produces.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkupDef {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub line: usize,
}

/// One markup-contributed ref in the shape the C# extractor emits, so
/// `resolve_graph` needs no markup branch: `qualified` is `Some` only when the
/// source text was dotted (the ladder's exact-qualified step keys off exactly
/// that), `name` is the tail segment, and the ref SITE's namespace is always
/// empty -- markup has no enclosing namespace of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkupRef {
    pub kind: String,
    pub name: String,
    pub qualified: Option<String>,
    pub member: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MarkupFacts {
    pub defs: Vec<MarkupDef>,
    pub refs: Vec<MarkupRef>,
    pub names: Vec<NameRecord>,
}

fn is_name_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

// A prefix may carry `-` and `.` (XML names do); a type's own local name may
// not, which is what stops `<local:Panel.Items>` (property-element syntax)
// being read as a type called `Panel.Items` -- it reads as the type `Panel`,
// which is what the element actually references.
fn is_prefix_char(ch: u8) -> bool {
    is_name_char(ch) || ch == b'-' || ch == b'.'
}

fn is_path_char(ch: u8) -> bool {
    is_name_char(ch) || ch == b'.'
}

// Every literal this scan matches is ASCII, and every character class above
// rejects a byte with the high bit set, so a multi-byte character can never
// sit at a match boundary -- a slice taken at any index this scan produces is
// always on a char boundary. The `is_char_boundary` guard in the walk keeps
// that true for the one index the scan does NOT choose: the raw `pos + 1`
// step over an unmatched byte.
fn quoted(line: &str, start: usize) -> Option<(&str, usize)> {
    let offset = line[start..].find('"')?;
    Some((&line[start..start + offset], start + offset + 1))
}

// A binding expression's PATH, reading from just past `{Binding` / `{x:Bind`.
// Handles the three spellings that name a member: bare (`{Binding Caption}`),
// explicit (`{Binding Path=Caption}`) and dotted (`{x:Bind Vm.Caption}`).
// A run that turns out to be a named property rather than a path
// (`{Binding RelativeSource=...}`) is discarded on the `=` that follows it --
// without that test every markup extension's first argument name would be
// indexed as if it were a member.
fn binding_path(line: &str, from: usize) -> (&str, usize) {
    let b = line.as_bytes();
    let mut i = from;
    while i < b.len() && b[i] == b' ' {
        i += 1;
    }
    if line[i..].starts_with(PATH_PROPERTY) {
        i += PATH_PROPERTY.len();
    }
    let start = i;
    while i < b.len() && is_path_char(b[i]) {
        i += 1;
    }
    if i == start || (i < b.len() && b[i] == b'=') {
        return ("", i);
    }
    (&line[start..i], i)
}

// `<prefix:Local` -- the element form that names a type from a CLR namespace.
// `None` for an unprefixed element (`<Grid`), for a closing/processing/comment
// marker, and for anything that is not a name at all.
fn prefixed_element(line: &str, from: usize) -> Option<(&str, &str, usize)> {
    let b = line.as_bytes();
    let mut i = from;
    let prefix_start = i;
    while i < b.len() && is_prefix_char(b[i]) {
        i += 1;
    }
    if i == prefix_start || i >= b.len() || b[i] != b':' {
        return None;
    }
    let prefix = &line[prefix_start..i];
    i += 1;
    let local_start = i;
    while i < b.len() && is_name_char(b[i]) {
        i += 1;
    }
    if i == local_start {
        return None;
    }
    Some((prefix, &line[local_start..i], i))
}

// What one scan pass found in source order, before prefixes are applied.
enum Pending {
    Element {
        prefix: String,
        local: String,
        line: usize,
    },
    Bind {
        path: String,
        line: usize,
    },
}

struct XamlScan {
    names: Vec<NameRecord>,
    pending: Vec<Pending>,
    prefixes: Vec<(String, String)>,
    decl: Option<(String, usize)>,
}

// One positional pass over a `.xaml` file, producing everything the graph needs
// from it. `names` is emitted in COLUMN ORDER within a line and line order
// across the file, with binding paths interleaved at the column they appear at.
// `pending` keeps element and binding occurrences in that same scan order so the
// resolution phase emits refs in a pinned, deterministic order too.
//
// Prefix declarations are collected during the scan but APPLIED after it:
// `xmlns:` sits on an ancestor element, so a forward-only application would work
// for well-formed markup and silently differ for anything else. Resolving once
// at the end has no such dependence on where the declaration sits. The prefix
// table is a Vec of pairs, not a map, because first-declaration-wins is the rule
// and iteration order has to stay fixed.
fn scan_xaml(text: &str) -> XamlScan {
    let mut scan = XamlScan {
        names: Vec::new(),
        pending: Vec::new(),
        prefixes: Vec::new(),
        decl: None,
    };

    for (i, line) in text.split('\n').enumerate() {
        let line_no = i + 1;
        let bytes = line.as_bytes();
        let mut pos = 0usize;
        while pos < line.len() {
            if !line.is_char_boundary(pos) {
                pos += 1;
                continue;
            }
            let rest = &line[pos..];
            if rest.starts_with(X_CLASS_ATTR) {
                let Some((value, next)) = quoted(line, pos + X_CLASS_ATTR.len()) else {
                    break;
                };
                if !value.is_empty() {
                    scan.names.push(NameRecord {
                        name: value.to_string(),
                        kind: "markup-class".to_string(),
                        line: line_no,
                        owner: String::new(),
                    });
                    if scan.decl.is_none() {
                        scan.decl = Some((value.to_string(), line_no));
                    }
                }
                pos = next;
                continue;
            }
            if rest.starts_with(X_NAME_ATTR) {
                let Some((value, next)) = quoted(line, pos + X_NAME_ATTR.len()) else {
                    break;
                };
                if !value.is_empty() {
                    scan.names.push(NameRecord {
                        name: value.to_string(),
                        kind: "markup-name".to_string(),
                        line: line_no,
                        owner: String::new(),
                    });
                }
                pos = next;
                continue;
            }
            if rest.starts_with(XMLNS_PREFIX) {
                let mut j = pos + XMLNS_PREFIX.len();
                let start = j;
                while j < bytes.len() && is_prefix_char(bytes[j]) {
                    j += 1;
                }
                if j == start || !line[j..].starts_with("=\"") {
                    pos = if j > pos { j } else { pos + 1 };
                    continue;
                }
                let prefix = line[start..j].to_string();
                let Some((value, next)) = quoted(line, j + 2) else {
                    break;
                };
                let ns = if let Some(rest) = value.strip_prefix(CLR_NAMESPACE) {
                    Some(match rest.find(';') {
                        Some(semi) => &rest[..semi],
                        None => rest,
                    })
                } else {
                    value.strip_prefix(USING_NAMESPACE)
                };
                if let Some(ns) = ns {
                    if !ns.is_empty() && !scan.prefixes.iter().any(|(p, _)| p == &prefix) {
                        scan.prefixes.push((prefix, ns.to_string()));
                    }
                }
                pos = next;
                continue;
            }
            if rest.starts_with(BINDING_OPEN) || rest.starts_with(XBIND_OPEN) {
                let bind = rest.starts_with(XBIND_OPEN);
                let from = pos
                    + if bind {
                        XBIND_OPEN.len()
                    } else {
                        BINDING_OPEN.len()
                    };
                let (path, next) = binding_path(line, from);
                if !path.is_empty() {
                    scan.names.push(NameRecord {
                        name: path.to_string(),
                        kind: "markup-binding".to_string(),
                        line: line_no,
                        owner: String::new(),
                    });
                    if bind {
                        scan.pending.push(Pending::Bind {
                            path: path.to_string(),
                            line: line_no,
                        });
                    }
                }
                pos = if next > pos { next } else { pos + 1 };
                continue;
            }
            if bytes[pos] == b'<' {
                if let Some((prefix, local, next)) = prefixed_element(line, pos + 1) {
                    scan.pending.push(Pending::Element {
                        prefix: prefix.to_string(),
                        local: local.to_string(),
                        line: line_no,
                    });
                    pos = next;
                    continue;
                }
            }
            pos += 1;
        }
    }
    scan
}

// `<data name="Key" xml:space="preserve">` -- the `<data` anchor is what keeps
// this off every other `name="` attribute a resource file can carry.
fn scan_resw(text: &str) -> Vec<NameRecord> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let mut pos = 0usize;
        loop {
            let Some(data) = line[pos..].find("<data") else {
                break;
            };
            let data = pos + data;
            let Some(attr) = line[data..].find(RESW_KEY_ATTR) else {
                break;
            };
            let start = data + attr + RESW_KEY_ATTR.len();
            let Some(offset) = line[start..].find('"') else {
                break;
            };
            let name = &line[start..start + offset];
            if !name.is_empty() {
                out.push(NameRecord {
                    name: name.to_string(),
                    kind: "resource-key".to_string(),
                    line: i + 1,
                    owner: String::new(),
                });
            }
            pos = start + offset + 1;
        }
    }
    out
}

/// The name facts a markup or resource file contributes. `owner` is empty on
/// every entry -- nothing in a markup file declares a member of a graph def.
pub fn markup_names(rel: &str, text: &str) -> Vec<NameRecord> {
    if is_resource(rel) {
        scan_resw(text)
    } else {
        scan_xaml(text).names
    }
}

// A dotted declaration splits into the namespace and the simple name the C#
// half of the same partial type carries; an undotted one is a type in the
// global namespace, exactly as a C# def with an empty `namespace` is.
fn split_qualified(qualified: &str) -> (&str, &str) {
    match qualified.rfind('.') {
        Some(dot) => (&qualified[..dot], &qualified[dot + 1..]),
        None => ("", qualified),
    }
}

fn type_ref(kind: &str, qualified: &str, member: Option<String>, line: usize) -> MarkupRef {
    let (_, name) = split_qualified(qualified);
    MarkupRef {
        kind: kind.to_string(),
        name: name.to_string(),
        qualified: if qualified.contains('.') {
            Some(qualified.to_string())
        } else {
            None
        },
        member,
        line,
    }
}

/// The graph facts a markup file contributes.
///
///   `x:Class="Ns.TheControl"` -> a def with the SAME id the code-behind's
///   `partial class TheControl` produces. `build_def_index` merges two defs
///   sharing an id into one record with two declaring sites, so the markup and
///   the code-behind become two locations of one symbol for free -- which is
///   what makes `refs` list both and `impact` seed from either.
///
///   `<local:TheControl .../>` where `xmlns:local` names a CLR namespace -> an
///   ordinary `uses-type` ref to `Ns.TheControl`. Without this edge a control
///   instantiated only in markup would have no inbound reference at all.
///
///   `{x:Bind Prop}` -> a `uses-member` ref against the file's OWN `x:Class`
///   type. `x:Bind`'s root is that type by language rule, so the binding target
///   is statically known and earns an edge. `{Binding Prop}` binds against a
///   DataContext assigned at runtime, which markup alone cannot name, so it
///   contributes the `markup-binding` NAME fact only -- the never-guess rule
///   applies to a binding exactly as it applies to a call.
pub fn markup_facts(rel: &str, text: &str) -> MarkupFacts {
    if is_resource(rel) {
        return MarkupFacts {
            defs: Vec::new(),
            refs: Vec::new(),
            names: scan_resw(text),
        };
    }
    let scan = scan_xaml(text);

    let mut defs = Vec::new();
    if let Some((qualified, line)) = &scan.decl {
        let (namespace, name) = split_qualified(qualified);
        defs.push(MarkupDef {
            id: qualified.clone(),
            name: name.to_string(),
            namespace: namespace.to_string(),
            kind: "class".to_string(),
            line: *line,
        });
    }

    let mut refs = Vec::new();
    for p in &scan.pending {
        match p {
            Pending::Element {
                prefix,
                local,
                line,
            } => {
                let Some((_, ns)) = scan.prefixes.iter().find(|(name, _)| name == prefix) else {
                    continue;
                };
                refs.push(type_ref("uses-type", &format!("{ns}.{local}"), None, *line));
            }
            Pending::Bind { path, line } => {
                let Some((qualified, _)) = &scan.decl else {
                    continue;
                };
                // The bound member is the FIRST path segment: `{x:Bind Vm.Name}`
                // reads `Vm` off the x:Class type and everything after it off
                // whatever `Vm` returns, which is a type this scan cannot name.
                let member = match path.find('.') {
                    Some(dot) => &path[..dot],
                    None => path.as_str(),
                };
                refs.push(type_ref(
                    "uses-member",
                    qualified,
                    Some(member.to_string()),
                    *line,
                ));
            }
        }
    }

    MarkupFacts {
        defs,
        refs,
        names: scan.names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(rel: &str, text: &str) -> Vec<(String, String, usize)> {
        markup_names(rel, text)
            .into_iter()
            .map(|n| (n.name, n.kind, n.line))
            .collect()
    }

    #[test]
    fn xaml_records_class_and_every_element_name_in_source_order() {
        let src = "<UserControl\n\tx:Class=\"Gadgets.PanelView\">\n\t<Button x:Name=\"ShipButton\" />\n</UserControl>\n";
        assert_eq!(
            names("src/Panel.xaml", src),
            vec![
                (
                    "Gadgets.PanelView".to_string(),
                    "markup-class".to_string(),
                    2
                ),
                ("ShipButton".to_string(), "markup-name".to_string(), 3),
            ]
        );
    }

    #[test]
    fn xaml_takes_two_names_on_one_line_left_to_right() {
        let src = "<Grid x:Name=\"Outer\"><Grid x:Name=\"Inner\" /></Grid>\n";
        assert_eq!(
            names("a.xaml", src),
            vec![
                ("Outer".to_string(), "markup-name".to_string(), 1),
                ("Inner".to_string(), "markup-name".to_string(), 1)
            ]
        );
    }

    #[test]
    fn xaml_skips_an_empty_value_and_an_unterminated_attribute() {
        assert!(names("a.xaml", "<Grid x:Name=\"\" />\n").is_empty());
        assert!(names("a.xaml", "<Grid x:Name=\"Unclosed\n").is_empty());
    }

    #[test]
    fn resw_keys_come_from_data_elements_only() {
        let src = "<root>\n\t<resheader name=\"resmimetype\" />\n\t<data name=\"Ship.Content\" xml:space=\"preserve\">\n\t\t<value>Ship</value>\n\t</data>\n</root>\n";
        assert_eq!(
            names("a.resw", src),
            vec![("Ship.Content".to_string(), "resource-key".to_string(), 3)]
        );
    }

    // (2026-08-23) -- `.resx` (.NET desktop/web resources) carries the exact
    // same `<data name="Key">` schema as `.resw` (WinRT resources) and gets
    // identical treatment: same scan, same `resource-key` kind, same tier.
    #[test]
    fn resx_keys_come_from_data_elements_only_same_as_resw() {
        let src = "<root>\n\t<resheader name=\"resmimetype\" />\n\t<data name=\"Ship.Content\" xml:space=\"preserve\">\n\t\t<value>Ship</value>\n\t</data>\n</root>\n";
        assert_eq!(
            names("a.resx", src),
            vec![("Ship.Content".to_string(), "resource-key".to_string(), 3)]
        );
    }

    #[test]
    fn markup_extensions_are_xaml_resw_and_resx_only() {
        assert!(is_markup("a/b.xaml"));
        assert!(is_markup("a/b.resw"));
        assert!(is_markup("a/b.resx"));
        assert!(!is_markup("a/b.xaml.cs"));
        assert!(!is_markup("a/b.xml"));
    }

    // --- markup graph facts ------------------------------------------------

    const CONTROL: &str = "<UserControl\n    x:Class=\"Demo.Controls.ShipPanel\"\n    xmlns:x=\"http://schemas.microsoft.com/winfx/2006/xaml\">\n    <TextBlock Text=\"{x:Bind Caption}\" />\n</UserControl>\n";
    const PAGE: &str = "<Page\n    x:Class=\"Demo.Pages.HomePage\"\n    xmlns:local=\"clr-namespace:Demo.Controls\">\n    <local:ShipPanel x:Name=\"Panel\" Header=\"{Binding Title}\" />\n</Page>\n";

    #[test]
    fn x_class_becomes_a_def_with_the_code_behind_id() {
        let facts = markup_facts("Controls/ShipPanel.xaml", CONTROL);
        assert_eq!(
            facts.defs,
            vec![MarkupDef {
                id: "Demo.Controls.ShipPanel".to_string(),
                name: "ShipPanel".to_string(),
                namespace: "Demo.Controls".to_string(),
                kind: "class".to_string(),
                line: 2,
            }]
        );
    }

    #[test]
    fn a_prefixed_element_becomes_a_uses_type_ref_at_the_mapped_namespace() {
        let facts = markup_facts("Pages/HomePage.xaml", PAGE);
        assert_eq!(
            facts.refs,
            vec![MarkupRef {
                kind: "uses-type".to_string(),
                name: "ShipPanel".to_string(),
                qualified: Some("Demo.Controls.ShipPanel".to_string()),
                member: None,
                line: 4,
            }]
        );
    }

    #[test]
    fn x_bind_earns_a_member_ref_against_the_files_own_class_and_binding_does_not() {
        let control = markup_facts("Controls/ShipPanel.xaml", CONTROL);
        assert_eq!(
            control.refs,
            vec![MarkupRef {
                kind: "uses-member".to_string(),
                name: "ShipPanel".to_string(),
                qualified: Some("Demo.Controls.ShipPanel".to_string()),
                member: Some("Caption".to_string()),
                line: 4,
            }]
        );
        let page = markup_facts("Pages/HomePage.xaml", PAGE);
        assert!(
            page.refs.iter().all(|r| r.kind != "uses-member"),
            "a runtime DataContext is not statically known, so `{{Binding Title}}` earns a name fact and no edge"
        );
        assert!(page
            .names
            .iter()
            .any(|n| n.name == "Title" && n.kind == "markup-binding" && n.line == 4));
    }

    #[test]
    fn an_unmapped_prefix_and_a_bare_element_contribute_nothing() {
        let facts = markup_facts("a.xaml", "<Page xmlns:d=\"http://schemas.microsoft.com/expression/blend/2008\">\n  <d:DesignHost />\n  <Grid />\n</Page>\n");
        assert!(
            facts.refs.is_empty(),
            "a prefix that names no CLR namespace is skipped, never guessed at"
        );
    }

    #[test]
    fn a_closing_tag_never_doubles_the_instantiation_and_a_property_element_reads_as_its_type() {
        let src = "<Page\n    xmlns:local=\"using:Demo.Controls\">\n    <local:ShipPanel>\n        <local:ShipPanel.Header>x</local:ShipPanel.Header>\n    </local:ShipPanel>\n</Page>\n";
        let facts = markup_facts("a.xaml", src);
        let lines: Vec<usize> = facts.refs.iter().map(|r| r.line).collect();
        assert_eq!(
            lines,
            vec![3, 4],
            "the open tag and the property element, never a closing tag"
        );
        assert!(facts
            .refs
            .iter()
            .all(|r| r.qualified.as_deref() == Some("Demo.Controls.ShipPanel")));
    }

    #[test]
    fn a_clr_namespace_drops_its_assembly_suffix() {
        let src = "<Page xmlns:local=\"clr-namespace:Demo.Controls;assembly=Demo\">\n  <local:ShipPanel />\n</Page>\n";
        let facts = markup_facts("a.xaml", src);
        assert_eq!(
            facts.refs[0].qualified.as_deref(),
            Some("Demo.Controls.ShipPanel")
        );
    }

    #[test]
    fn a_named_markup_extension_property_is_not_a_binding_path() {
        let src = "<Page>\n  <T A=\"{Binding RelativeSource={RelativeSource Self}}\" B=\"{Binding Path=Vm.Name, Mode=OneWay}\" />\n</Page>\n";
        let facts = markup_facts("a.xaml", src);
        assert_eq!(
            facts.names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(),
            vec!["Vm.Name"],
            "`RelativeSource=` is a property name, not a path; `Path=` is consumed and the dotted path kept whole"
        );
    }

    #[test]
    fn a_markup_file_with_no_x_class_declares_nothing_and_binds_nothing() {
        let facts = markup_facts(
            "a.xaml",
            "<ResourceDictionary>\n  <T V=\"{x:Bind Orphan}\" />\n</ResourceDictionary>\n",
        );
        assert!(facts.defs.is_empty());
        assert!(
            facts.refs.is_empty(),
            "x:Bind with no x:Class root has no type to resolve against"
        );
        assert!(facts
            .names
            .iter()
            .any(|n| n.name == "Orphan" && n.kind == "markup-binding"));
    }
}
