use super::*;

#[test]
fn module_resolution_ac_2_workspace_patterns_match_segments_and_exclusions() {
    assert!(glob("packages/*", "packages/core"));
    assert!(!glob("packages/*", "packages/deep/core"));
    assert!(glob("packages/**", "packages/deep/core"));
    assert!(glob("packages/**", "packages"));
    assert!(glob("plugin-*", "plugin-a"));
    assert!(!glob("plugin-*", "other-a"));
}

#[test]
fn module_resolution_ac_2_pnpm_packages_allow_extra_top_level_keys() {
    let text = "packages:\n  - 'packages/*'\n  - '!packages/skip'\noverrides:\n  foo: bar\ncatalogs:\n  dev:\n    x: y\n";
    assert_eq!(
        parse_pnpm(text).unwrap(),
        vec!["packages/*", "!packages/skip"]
    );
    assert_eq!(
        parse_pnpm("packages: ['apps/*', 'libs/**']").unwrap(),
        vec!["apps/*", "libs/**"]
    );
}

#[test]
fn module_resolution_ac_6_pnpm_rejects_unreadable_packages() {
    for text in [
        "packages:\n  x: y",
        "packages:\n  - !skip",
        "packages:\n\t- a",
        "packages:\n  - a\n---",
    ] {
        assert!(parse_pnpm(text).is_err(), "{text}");
    }
}
