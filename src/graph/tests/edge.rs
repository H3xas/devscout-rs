use super::*;

// --- Edge: shape-per-kind, tag first ---------------------------------

#[test]
fn imports_edge_has_target_not_to() {
    let e = Edge::Imports {
        from_file: "F.cs".into(),
        from_line: 1,
        target: "System".into(),
    };
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"imports","from_file":"F.cs","from_line":1,"target":"System"}"#
    );
}

#[test]
fn uses_type_edge_shape() {
    let e = Edge::UsesType {
        from_file: "F.cs".into(),
        from_line: 1,
        to: "Ns.T".into(),
        to_file: "Ns/T.cs".into(),
        heuristic: false,
    };
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"uses-type","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs"}"#
    );
}

// The serialized shape in one assertion: `heuristic` is the LAST key, it
// appears only when set, and a precise edge of the same kind is
// byte-for-byte what it was before the tag existed.
#[test]
fn heuristic_flag_is_appended_last_and_omitted_when_false() {
    let precise = Edge::UsesMember {
        from_file: "F.cs".into(),
        from_line: 1,
        to: "Ns.T".into(),
        to_file: "Ns/T.cs".into(),
        heuristic: false,
        tier: None,
        member: None,
    };
    assert_eq!(
        serde_json::to_string(&precise).unwrap(),
        r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs"}"#
    );
    let guess = Edge::UsesMember {
        from_file: "F.cs".into(),
        from_line: 1,
        to: "Ns.T".into(),
        to_file: "Ns/T.cs".into(),
        heuristic: true,
        tier: Some(HeuristicTier::Guess),
        member: Some("M".into()),
    };
    assert_eq!(
        serde_json::to_string(&guess).unwrap(),
        r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs","heuristic":true,"tier":"guess","member":"M"}"#
    );
    let ext = Edge::UsesMember {
        from_file: "F.cs".into(),
        from_line: 1,
        to: "Ns.T".into(),
        to_file: "Ns/T.cs".into(),
        heuristic: true,
        tier: Some(HeuristicTier::Ext),
        member: Some("M".into()),
    };
    assert_eq!(
        serde_json::to_string(&ext).unwrap(),
        r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs","heuristic":true,"tier":"ext","member":"M"}"#
    );
    // And it reads back: an edge written by either runtime round-trips
    // with the flag intact, absent meaning precise -- and now with its
    // tier and member intact too, which is what makes an older graph.json
    // (neither key written) still parse, as two `None`s.
    assert_eq!(
        serde_json::from_str::<Edge>(&serde_json::to_string(&guess).unwrap()).unwrap(),
        guess
    );
    assert_eq!(
        serde_json::from_str::<Edge>(&serde_json::to_string(&ext).unwrap()).unwrap(),
        ext
    );
    assert_eq!(
        serde_json::from_str::<Edge>(&serde_json::to_string(&precise).unwrap()).unwrap(),
        precise
    );
}

// The append ORDER, pinned on its own: `heuristic`, then `tier`, then
// `member`, each omitted when it has nothing to say. A precise edge that
// does name its member -- which is every precise uses-member edge the
// resolver emits -- carries `member` and nothing else, so its bytes gain
// exactly one key over the pre-tier shape.
#[test]
fn uses_member_edge_appends_tier_then_member_after_heuristic_and_omits_both_when_precise() {
    let precise = Edge::uses_member(
        "F.cs".into(),
        1,
        "Ns.T".into(),
        "Ns/T.cs".into(),
        Some("M".into()),
        None,
    );
    assert_eq!(
        serde_json::to_string(&precise).unwrap(),
        r#"{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs","member":"M"}"#
    );
    assert!(!precise.is_heuristic(), "no tier means no guess");
    assert_eq!(precise.tier(), None);

    for (tier, word) in [(HeuristicTier::Ext, "ext"), (HeuristicTier::Guess, "guess")] {
        let e = Edge::uses_member(
            "F.cs".into(),
            1,
            "Ns.T".into(),
            "Ns/T.cs".into(),
            Some("M".into()),
            Some(tier),
        );
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            format!(
                r#"{{"kind":"uses-member","from_file":"F.cs","from_line":1,"to":"Ns.T","to_file":"Ns/T.cs","heuristic":true,"tier":"{word}","member":"M"}}"#
            ),
            "heuristic, then tier, then member"
        );
        // The constructor is what makes the flag and the tier one fact:
        // pass a tier and the edge is a guess, pass none and it is not.
        assert!(e.is_heuristic());
        assert_eq!(e.tier(), Some(tier));
    }

    // The two kinds that carry the flag but no tier answer `None` rather
    // than guessing on the reader's behalf.
    assert_eq!(
        Edge::UsesType {
            from_file: "F.cs".into(),
            from_line: 1,
            to: "Ns.T".into(),
            to_file: "Ns/T.cs".into(),
            heuristic: true,
        }
        .tier(),
        None
    );
}

#[test]
fn ambiguous_edge_shape() {
    let e = Edge::Ambiguous {
        origin: "uses-type".into(),
        from_file: "F.cs".into(),
        from_line: 4,
        raw: "Money".into(),
        candidates: vec![Candidate {
            id: "A.Money".into(),
            file: "A/Money.cs".into(),
        }],
        candidate_count: 2,
    };
    let json = serde_json::to_string(&e).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"ambiguous","origin":"uses-type","from_file":"F.cs","from_line":4,"raw":"Money","candidates":[{"id":"A.Money","file":"A/Money.cs"}],"candidate_count":2}"#
    );
}

// --- Stats: fixed edges_by_kind order ---------------------------------

#[test]
fn edges_by_kind_field_order_is_fixed_not_alphabetical() {
    let s = EdgesByKind {
        inherits: 1,
        uses_type: 2,
        imports: 3,
        uses_member: 4,
        ctor_di: 5,
        implements: 6,
        overrides: 7,
        ..Default::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(
        json,
        r#"{"inherits":1,"uses-type":2,"imports":3,"uses-member":4,"ctor-di":5,"implements":6,"overrides":7}"#
    );
}

// The four TS counts land AFTER `ctor-di`, in this order, and
// only when the repo carries a TS fragment at all: a C#-only repo's stats
// block keeps the exact bytes the assertion above pins.
#[test]
fn edges_by_kind_appends_the_four_ts_counts_after_ctor_di_when_present() {
    let s = EdgesByKind {
        inherits: 1,
        uses_type: 2,
        imports: 3,
        uses_member: 4,
        ctor_di: 5,
        implements: 10,
        overrides: 11,
        import: Some(6),
        call: Some(7),
        jsx_use: Some(8),
        dispatch: Some(9),
    };
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(
        json,
        r#"{"inherits":1,"uses-type":2,"imports":3,"uses-member":4,"ctor-di":5,"implements":10,"overrides":11,"import":6,"call":7,"jsx-use":8,"dispatch":9}"#
    );
}
