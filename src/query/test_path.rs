// What makes a file a TEST file for `tests` and `impact`: the one definition
// every consumer reads, so the two verbs and the `tests` bus filter always
// agree. Three vouches, checked most specific first: an attributed test def
// declared in the file, the project model placing the file in a test project,
// and -- for a TypeScript or JavaScript file neither of those reaches -- the
// file's own name.

use super::coverage::TestVia;
use super::index::GraphIndex;

// The test-runner naming conventions. Case-sensitive, and never every
// extension this tool reads: `.jsx` and the TypeScript module suffixes
// `.mts`/`.cts` are read and graphed by the other verbs but not classified by
// their name.
const TEST_EXTENSIONS: [&str; 5] = [".ts", ".tsx", ".js", ".mjs", ".cjs"];
const TEST_SUFFIXES: [&str; 10] = [
    ".spec.ts",
    ".spec.tsx",
    ".spec.js",
    ".spec.mjs",
    ".spec.cjs",
    ".test.ts",
    ".test.tsx",
    ".test.js",
    ".test.mjs",
    ".test.cjs",
];
const TESTS_SEGMENT: &str = "__tests__";

/// Whether `file` is named as a test file: one of the `*.spec`/`*.test`
/// suffixes this module lists, or a file of a classified extension below a
/// directory segment exactly `__tests__`.
pub(crate) fn is_test_path(file: &str) -> bool {
    if !TEST_EXTENSIONS.iter().any(|ext| file.ends_with(ext)) {
        return false;
    }
    if TEST_SUFFIXES.iter().any(|suffix| file.ends_with(suffix)) {
        return true;
    }
    let mut segments: Vec<&str> = file.split('/').collect();
    segments.pop();
    segments.contains(&TESTS_SEGMENT)
}

/// Which vouch, if any, makes `file` a test file.
pub(crate) fn test_via(index: &GraphIndex, file: &str) -> Option<TestVia> {
    if index.test_defs_by_file.contains_key(file) {
        return Some(TestVia::Attribute);
    }
    if let Some(project) = &index.project {
        if project
            .unit_of_file(file)
            .is_some_and(|u| project.units[u].test)
        {
            return Some(TestVia::Project);
        }
    }
    is_test_path(file).then_some(TestVia::Path)
}
