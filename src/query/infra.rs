/// Default hub brake: a file whose in-degree is at or above this stops
/// widening. In-degree here is the number of DISTINCT OTHER FILES that
/// reference a file through a `direct` (inherits/uses-type/uses-member) or
/// heuristic edge. Derived from a corpus histogram rather than guessed: 824
/// files carry at least one referrer, every value from 1 to 33 is populated
/// (holding 777 of the 824, 94.3%), and the first empty slot is at 34, above
/// which the tail is sparse and gapped. The comparison is `>=`, so 34 is the
/// first value that is not an ordinary file. `0` disables the in-degree half
/// of the brake; the name-pattern half ([`is_infra_file`]) is a
/// classification, not a threshold, and stays on.
pub const DEFAULT_HUB_MAX_INDEGREE: usize = 34;

// The name-pattern half of the hub classification, extending the `infra` idea
// from TYPE names to FILE shapes: files the rest of an estate refers to BY
// JOB, not by dependency -- registering the container, composing the
// application, entering the process, or holding the setup every test class in
// a suite inherits.
//
// Spelled as explicit lowercase suffix/segment tests rather than a regex list,
// so the predicate is exact on any path, including ones no fixture covered.
const INFRA_BASENAMES: [&str; 2] = ["program.cs", "startup.cs"];
const INFRA_SUFFIXES: [&str; 8] = [
    "serviceextensions.cs",
    "servicecollectionextensions.cs",
    "registration.cs",
    "testbase.cs",
    "testsbase.cs",
    "basetest.cs",
    "basetests.cs",
    "basefixture.cs",
];
const INFRA_DIR: &str = "dependencyresolution/";
const COMPOSITION_ROOT: &str = "compositionroot";

/// A file whose PATH says it is infrastructure. Always on: unlike the
/// in-degree threshold, this is a classification of what the file is FOR, and
/// `0` on the threshold does not turn a composition root back into an ordinary
/// file.
pub fn is_infra_file(file: &str) -> bool {
    let lower = file.to_lowercase();
    let base = match lower.rfind('/') {
        Some(i) => &lower[i + 1..],
        None => &lower[..],
    };
    if INFRA_BASENAMES.contains(&base) {
        return true;
    }
    if INFRA_SUFFIXES.iter().any(|sfx| lower.ends_with(sfx)) {
        return true;
    }
    if lower.starts_with(INFRA_DIR) || lower.contains(&format!("/{INFRA_DIR}")) {
        return true;
    }
    // `CompositionRoot<Anything alphanumeric>.cs`, anywhere in the path's last
    // segment -- the composition root itself and the test class that asserts it.
    if let Some(at) = base.find(COMPOSITION_ROOT) {
        if base.ends_with(".cs") {
            let middle = &base[at + COMPOSITION_ROOT.len()..base.len() - 3];
            if middle.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    false
}
