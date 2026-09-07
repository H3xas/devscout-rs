use super::*;

#[test]
fn js_float_string_matches_node_json_stringify_on_checked_values() {
    // Expected values for JSON number formatting of each of these:
    assert_eq!(js_float_string(1.0), "1");
    assert_eq!(js_float_string(100.0), "100");
    assert_eq!(js_float_string(0.0), "0");
    assert_eq!(js_float_string(-0.0), "0");
    assert_eq!(js_float_string(0.15), "0.15");
    assert_eq!(js_float_string(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(js_float_string(1.0 / 3.0), "0.3333333333333333");
    assert_eq!(js_float_string(1e21), "1e+21");
    assert_eq!(js_float_string(0.0000001), "1e-7");
    assert_eq!(js_float_string(f64::NAN), "null");
    assert_eq!(js_float_string(f64::INFINITY), "null");
}

#[test]
fn js_math_round_matches_node_math_round_on_checked_values() {
    // Expected `Math.round`-style rounding for each of these:
    assert_eq!(js_math_round(0.0), 0);
    assert_eq!(js_math_round(1.4), 1);
    assert_eq!(js_math_round(1.5), 2);
    assert_eq!(js_math_round(1.49999), 1);
    assert_eq!(js_math_round(2.5), 3);
    assert_eq!(js_math_round(100.0 / 4.0), 25);
    assert_eq!(js_math_round(101.0 / 4.0), 25);
    assert_eq!(js_math_round(103.0 / 4.0), 26);
}

// --- `clear` --------------------------------------------------------------
//
// In-process coverage of `cmd_clear`, fast and independent of the
// subprocess byte-parity gate in the integration suite.

fn clear_root(prefix: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("scout-cli-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".scout")).unwrap();
    root
}

fn seed_row(conn: &rusqlite::Connection, session_id: &str, rel_path: &str) {
    store::record_fresh(
        conn,
        &store::RecordFresh {
            session_id,
            agent_id: "",
            rel_path,
            sha256: "sha",
            size: 10,
            mtime: 1,
            lines: 1,
            delivered: true,
        },
    )
    .unwrap();
}

#[test]
fn cmd_clear_whole_store_removes_cache_db_and_answers_cache_cleared() {
    let root = clear_root("clear-whole");
    {
        let conn = store::open_store(&root).unwrap();
        seed_row(&conn, "s1", "a.ts");
    }
    assert!(root.join(".scout").join("cache.db").exists());
    let (code, out) = cmd_clear(&root, &[]);
    assert_eq!(code, 0);
    assert_eq!(out, "cache cleared");
    assert!(!root.join(".scout").join("cache.db").exists());
}

#[test]
fn cmd_clear_older_than_rejects_negative_and_non_numeric_with_usage_code_2() {
    let root = clear_root("clear-older-bad");
    for bad in ["-5", "abc", ""] {
        let (code, out) = cmd_clear(&root, &["--older-than".to_string(), bad.to_string()]);
        assert_eq!(code, 2, "arg {bad:?}");
        assert_eq!(
            out, "usage: devscout clear --older-than <days>",
            "arg {bad:?}"
        );
    }
    // A bare `--older-than` with nothing after it (missing value entirely).
    let (code, out) = cmd_clear(&root, &["--older-than".to_string()]);
    assert_eq!(code, 2);
    assert_eq!(out, "usage: devscout clear --older-than <days>");
    assert!(
        !root.join(".scout").join("cache.db").exists(),
        "a rejected flag must not create a store"
    );
}

#[test]
fn cmd_clear_session_unique_prefix_deletes_only_that_sessions_rows() {
    let root = clear_root("clear-session-unique");
    {
        let conn = store::open_store(&root).unwrap();
        seed_row(&conn, "abc-1111", "a.ts");
        seed_row(&conn, "abc-2222", "b.ts");
    }
    let (code, out) = cmd_clear(&root, &["--session".to_string(), "abc-1".to_string()]);
    assert_eq!(code, 0);
    assert_eq!(out, "deleted 1 row for session abc-1111");

    let conn = store::open_store(&root).unwrap();
    assert!(store::lookup_read(&conn, "abc-1111", "a.ts", "")
        .unwrap()
        .is_none());
    assert!(store::lookup_read(&conn, "abc-2222", "b.ts", "")
        .unwrap()
        .is_some());
}

// The ambiguous-prefix refusal: two sessions share the "abc" prefix; asking
// for exactly that prefix must refuse on code 2, name both candidates, and
// delete nothing.
#[test]
fn cmd_clear_session_ambiguous_prefix_refuses_with_code_2_and_lists_both_ids() {
    let root = clear_root("clear-session-ambiguous");
    {
        let conn = store::open_store(&root).unwrap();
        seed_row(&conn, "abc-1111", "a.ts");
        seed_row(&conn, "abc-2222", "b.ts");
    }
    let (code, out) = cmd_clear(&root, &["--session".to_string(), "abc".to_string()]);
    assert_eq!(code, 2);
    assert_eq!(out, "ambiguous session prefix \"abc\": abc-1111, abc-2222");

    // Refused, so read-only: both sessions' rows still stand.
    let conn = store::open_store(&root).unwrap();
    assert!(store::lookup_read(&conn, "abc-1111", "a.ts", "")
        .unwrap()
        .is_some());
    assert!(store::lookup_read(&conn, "abc-2222", "b.ts", "")
        .unwrap()
        .is_some());
}

#[test]
fn cmd_clear_session_no_match_is_a_success_not_a_refusal() {
    let root = clear_root("clear-session-none");
    {
        let conn = store::open_store(&root).unwrap();
        seed_row(&conn, "abc-1111", "a.ts");
    }
    let (code, out) = cmd_clear(&root, &["--session".to_string(), "zzz".to_string()]);
    assert_eq!(code, 0);
    assert_eq!(out, "no sessions match \"zzz\"");
}

#[test]
fn cmd_clear_session_missing_value_is_a_usage_error() {
    let root = clear_root("clear-session-missing");
    let (code, out) = cmd_clear(&root, &["--session".to_string()]);
    assert_eq!(code, 2);
    assert_eq!(out, "usage: devscout clear --session <id-or-prefix>");
}

#[test]
fn cmd_clear_older_than_flag_takes_priority_over_session_flag_like_node_indexof_order() {
    // `clear` checks `--older-than` before `--session`, so a call carrying
    // both takes the age-based branch, whatever `--session` says.
    let root = clear_root("clear-precedence");
    {
        let conn = store::open_store(&root).unwrap();
        seed_row(&conn, "abc-1111", "a.ts");
    }
    let (code, out) = cmd_clear(
        &root,
        &[
            "--session".to_string(),
            "abc-1111".to_string(),
            "--older-than".to_string(),
            "9999".to_string(),
        ],
    );
    assert_eq!(code, 0);
    assert_eq!(
        out, "deleted 0 rows older than 9999d",
        "the --older-than branch must win"
    );
}
