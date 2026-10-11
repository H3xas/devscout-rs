// What makes a file a TEST file for `tests` and `impact`: the one definition
// every consumer reads, so the two verbs and the `tests` bus filter always
// agree. Three vouches, checked most specific first: an attributed test def
// declared in the file, the project model placing the file in a test project,
// and -- for a TypeScript file neither of those reaches -- the file's own name.

use super::coverage::TestVia;
use super::index::GraphIndex;

// The TypeScript test-runner naming conventions. Case-sensitive, and `.ts`/
// `.tsx` only: a JavaScript file is read by the other verbs but not
// classified by its name.
const TEST_SUFFIXES: [&str; 4] = [".spec.ts", ".spec.tsx", ".test.ts", ".test.tsx"];
const TESTS_SEGMENT: &str = "__tests__";

/// Whether `file` is named as a TypeScript test file: one of the four
/// `*.spec`/`*.test` suffixes, or a `.ts`/`.tsx` file below a directory
/// segment exactly `__tests__`.
pub(crate) fn is_test_path(file: &str) -> bool {
    if !(file.ends_with(".ts") || file.ends_with(".tsx")) {
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
