use super::ts_fragment_types::TsFragment;
use super::types::{DefRecord, Extraction, Fact, NameRecord, RefRecord, UsingRecord};

// Hand-rolled JSON value + pretty printer matching
// `JSON.stringify(value, null, 2)` byte-for-byte (2-space indent, no
// trailing commas, empty arrays/objects collapse to `[]`/`{}` on one line)
// -- no serde dependency, per the same "no new dependency" rule
// parse.rs's spans_json already follows. Generalized here (unlike
// spans_json's fixed six-field record) because extraction records are
// heterogeneous (optional fields, nested arrays of objects).
pub(super) enum Json {
    Null,
    Bool(bool),
    Num(usize),
    /// A SIGNED number, for the one field that can be negative: an extension
    /// entry's `arityMax`, where -1 is the unbounded-`params` sentinel,
    /// written as a plain JSON number.
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(&'static str, Json)>),
    /// Same encoding as `Obj`, for the one record whose keys are DATA rather
    /// than a fixed schema: a def's `methodReturns`. Insertion order is
    /// significant (first-declaration source order), so this is a Vec of
    /// pairs, never a sorted map.
    Map(Vec<(String, Json)>),
}

impl Json {
    fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => out.push_str(&n.to_string()),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Str(s) => out.push_str(&json_string(s)),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                let pad = "  ".repeat(indent + 1);
                let last = items.len() - 1;
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&pad);
                    item.write(out, indent + 1);
                    if i != last {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&"  ".repeat(indent));
                out.push(']');
            }
            Json::Obj(fields) => {
                write_object(out, indent, fields.iter().map(|(k, v)| (*k, v)));
            }
            Json::Map(entries) => {
                write_object(out, indent, entries.iter().map(|(k, v)| (k.as_str(), v)));
            }
        }
    }
}

fn write_object<'a>(
    out: &mut String,
    indent: usize,
    fields: impl ExactSizeIterator<Item = (&'a str, &'a Json)>,
) {
    if fields.len() == 0 {
        out.push_str("{}");
        return;
    }
    out.push_str("{\n");
    let pad = "  ".repeat(indent + 1);
    let last = fields.len() - 1;
    for (i, (k, v)) in fields.enumerate() {
        out.push_str(&pad);
        out.push_str(&json_string(k));
        out.push_str(": ");
        v.write(out, indent + 1);
        if i != last {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&"  ".repeat(indent));
    out.push('}');
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[allow(
    clippy::too_many_lines,
    reason = "one ordered pass listing every DefRecord field in the shape the JSON output promises"
)]
pub(super) fn def_to_json(d: &DefRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("id", Json::Str(d.id.clone())),
        ("name", Json::Str(d.name.clone())),
        ("namespace", Json::Str(d.namespace.clone())),
        ("kind", Json::Str(d.kind.clone())),
        ("line", Json::Num(d.line)),
        (
            "methods",
            Json::Arr(d.methods.iter().map(|m| Json::Str(m.clone())).collect()),
        ),
    ];
    // Appended last, in declaration order, each only when non-empty.
    if !d.properties.is_empty() {
        fields.push((
            "properties",
            Json::Arr(d.properties.iter().map(|p| Json::Str(p.clone())).collect()),
        ));
    }
    if !d.fields.is_empty() {
        fields.push((
            "fields",
            Json::Arr(d.fields.iter().map(|f| Json::Str(f.clone())).collect()),
        ));
    }
    if !d.method_returns.is_empty() {
        fields.push((
            "methodReturns",
            Json::Map(
                d.method_returns
                    .iter()
                    .map(|(k, v)| (k.clone(), Json::Str(v.clone())))
                    .collect(),
            ),
        ));
    }
    // Appended after the properties/fields/methodReturns trio, entry keys in
    // the serialized order (name, thisType, arityMin, arityMax, thisArgs),
    // omitted when empty. `thisArgs` is present only on a generic
    // this-parameter.
    if !d.extension_methods.is_empty() {
        fields.push((
            "extensionMethods",
            Json::Arr(
                d.extension_methods
                    .iter()
                    .map(|e| {
                        let mut kv: Vec<(&'static str, Json)> = vec![
                            ("name", Json::Str(e.name.clone())),
                            ("thisType", Json::Str(e.this_type.clone())),
                            ("arityMin", Json::Num(e.arity_min)),
                            ("arityMax", Json::Int(e.arity_max)),
                        ];
                        if let Some(args) = &e.this_args {
                            kv.push((
                                "thisArgs",
                                Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
                            ));
                        }
                        Json::Obj(kv)
                    })
                    .collect(),
            ),
        ));
    }
    // Appended after extensionMethods.
    if !d.bases.is_empty() {
        fields.push((
            "bases",
            Json::Arr(d.bases.iter().map(|b| Json::Str(b.clone())).collect()),
        ));
    }
    // Appended after bases, before testMethods, each only when non-empty.
    if !d.type_params.is_empty() {
        fields.push((
            "typeParams",
            Json::Arr(d.type_params.iter().map(|t| Json::Str(t.clone())).collect()),
        ));
    }
    if !d.base_generic_args.is_empty() {
        fields.push((
            "baseGenericArgs",
            Json::Map(
                d.base_generic_args
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Json::Arr(v.iter().map(|a| Json::Str(a.clone())).collect()),
                        )
                    })
                    .collect(),
            ),
        ));
    }
    // Appended after baseGenericArgs.
    if !d.test_methods.is_empty() {
        fields.push((
            "testMethods",
            Json::Arr(
                d.test_methods
                    .iter()
                    .map(|t| Json::Str(t.clone()))
                    .collect(),
            ),
        ));
    }
    // Appended after testMethods, entry keys in source order.
    if !d.property_types.is_empty() {
        fields.push((
            "propertyTypes",
            Json::Map(
                d.property_types
                    .iter()
                    .map(|(name, fact)| (name.clone(), fact_to_json(fact)))
                    .collect(),
            ),
        ));
    }
    // Appended LAST of all, after propertyTypes, entry keys in source order.
    if !d.method_return_args.is_empty() {
        fields.push((
            "methodReturnArgs",
            Json::Map(
                d.method_return_args
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            Json::Arr(v.iter().map(|a| Json::Str(a.clone())).collect()),
                        )
                    })
                    .collect(),
            ),
        ));
    }
    Json::Obj(fields)
}

// One declared type fact: `{type}`, or `{type, args}` when the declared type
// carried a top-level type-argument list. Field order is significant, and
// `args` is omitted when absent.
fn fact_to_json(fact: &Fact) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![("type", Json::Str(fact.type_name.clone()))];
    if let Some(args) = &fact.args {
        fields.push((
            "args",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    Json::Obj(fields)
}

fn using_to_json(u: &UsingRecord) -> Json {
    match u {
        UsingRecord::Alias {
            alias,
            target,
            global,
        } => Json::Obj(vec![
            ("alias", Json::Str(alias.clone())),
            ("target", Json::Str(target.clone())),
            ("global", Json::Bool(*global)),
        ]),
        UsingRecord::Plain { text, global } => Json::Obj(vec![
            ("text", Json::Str(text.clone())),
            ("global", Json::Bool(*global)),
        ]),
    }
}

fn ref_to_json(r: &RefRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("kind", Json::Str(r.kind.clone())),
        ("name", Json::Str(r.name.clone())),
    ];
    if let Some(q) = &r.qualified {
        fields.push(("qualified", Json::Str(q.clone())));
    }
    if let Some(m) = &r.member {
        fields.push(("member", Json::Str(m.clone())));
    }
    fields.push(("line", Json::Num(r.line)));
    fields.push((
        "namespace",
        match &r.namespace {
            Some(ns) => Json::Str(ns.clone()),
            None => Json::Null,
        },
    ));
    if let Some(arity) = r.type_arg_count {
        fields.push(("typeArgCount", Json::Num(arity)));
    }
    // Both appended last, in that order, and only when set.
    if r.generic {
        fields.push(("generic", Json::Bool(true)));
    }
    if let Some(rt) = &r.receiver_type {
        fields.push(("receiverType", Json::Str(rt.clone())));
    }
    // Appended after receiverType, and only when the access was a callee. The
    // test is presence, not truthiness: argCount 0 is a real value.
    if let Some(ac) = r.arg_count {
        fields.push(("argCount", Json::Num(ac)));
    }
    // Appended LAST, after argCount, and only
    // when the receiver's DECLARED type was generic.
    if let Some(args) = &r.receiver_args {
        fields.push((
            "receiverArgs",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    // Appended LAST of all, after receiverArgs, and only when the ref sits
    // inside a type.
    if !r.outer_types.is_empty() {
        fields.push((
            "outerTypes",
            Json::Arr(r.outer_types.iter().map(|t| Json::Str(t.clone())).collect()),
        ));
    }
    // Appended LAST of all, after outerTypes, and only set for a 'ctor-param'
    // ref whose type was generic.
    if let Some(args) = &r.args {
        fields.push((
            "args",
            Json::Arr(args.iter().map(|a| Json::Str(a.clone())).collect()),
        ));
    }
    // Appended after args, and only for a two-segment chain whose
    // head the enclosing scope could type.
    if let Some(owner) = &r.receiver_property_owner {
        fields.push(("receiverPropertyOwner", Json::Str(owner.clone())));
    }
    // Appended LAST of all, and always as a pair.
    if let Some(owner) = &r.receiver_call_owner {
        fields.push(("receiverCallOwner", Json::Str(owner.clone())));
    }
    if let Some(member) = &r.receiver_call_member {
        fields.push(("receiverCallMember", Json::Str(member.clone())));
    }
    Json::Obj(fields)
}

// Field order (`name`, `kind`, `line`, `owner`) is significant, `owner`
// omitted when empty.
fn name_to_json(n: &NameRecord) -> Json {
    let mut fields: Vec<(&'static str, Json)> = vec![
        ("name", Json::Str(n.name.clone())),
        ("kind", Json::Str(n.kind.clone())),
        ("line", Json::Num(n.line)),
    ];
    if !n.owner.is_empty() {
        fields.push(("owner", Json::Str(n.owner.clone())));
    }
    Json::Obj(fields)
}

/// Serializes a C# extraction as JSON.
pub fn extraction_to_json(e: &Extraction) -> String {
    let purpose_json = match &e.purpose {
        Some(p) => Json::Str(p.clone()),
        None => Json::Null,
    };
    let root = Json::Obj(vec![
        ("purpose", purpose_json),
        ("defs", Json::Arr(e.defs.iter().map(def_to_json).collect())),
        (
            "usings",
            Json::Arr(e.usings.iter().map(using_to_json).collect()),
        ),
        ("refs", Json::Arr(e.refs.iter().map(ref_to_json).collect())),
        (
            "names",
            Json::Arr(e.names.iter().map(name_to_json).collect()),
        ),
    ]);
    root.to_pretty_string()
}

/// Render a TS/JS file's dump-extract JSON. The reader expects
/// `{purpose, defs, usings, refs, names}`, and a TS fragment carries no
/// `usings` and no `names`, so both keys are dropped -- the dump is a
/// three-key object.
pub fn ts_extraction_to_json(purpose: &Option<String>, fragment: &TsFragment) -> String {
    let purpose_json = match purpose {
        Some(p) => Json::Str(p.clone()),
        None => Json::Null,
    };
    let root = Json::Obj(vec![
        ("purpose", purpose_json),
        (
            "defs",
            Json::Arr(
                fragment
                    .defs
                    .iter()
                    .map(|d| {
                        Json::Obj(vec![
                            ("name", Json::Str(d.name.clone())),
                            ("kind", Json::Str(d.kind.clone())),
                            ("line", Json::Num(d.line)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "refs",
            Json::Arr(
                fragment
                    .refs
                    .iter()
                    .map(|r| {
                        let mut fields = vec![
                            ("kind", Json::Str(r.kind.clone())),
                            ("name", Json::Str(r.name.clone())),
                        ];
                        if let Some(m) = &r.member {
                            fields.push(("member", Json::Str(m.clone())));
                        }
                        fields.push(("line", Json::Num(r.line)));
                        Json::Obj(fields)
                    })
                    .collect(),
            ),
        ),
    ]);
    root.to_pretty_string()
}
