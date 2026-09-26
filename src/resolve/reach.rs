use super::index::DefIndex;
use super::scope::FileContext;
use crate::project::ProjectModel;
use std::collections::HashMap;
use std::sync::Arc;

/// Gives every project-owned file's context the set of defs its project can
/// name, for the ladder's namespace-level walk.
///
/// C# resolves a bare name only among types the compilation can see: its own
/// sources and the assemblies on its transitive `ProjectReference` closure.
/// A namespace is not an assembly boundary, so a type declared in an
/// enclosing namespace by a project the site does not reference is not a
/// candidate there, and without this check it would shadow the same level's
/// using-imported type.
///
/// Only reachability counts; `Admission`'s test-project direction is a
/// heuristic veto the language does not have. A def is inside when any of
/// its declaring files (partial parts and duplicate declarations included)
/// is owned by a reachable project, and also whenever ownership is unknown
/// for one of them, because path ownership cannot see linked sources. Files
/// no project owns get no reach and see every def, as with no model at all.
pub(super) fn attach_reach(
    contexts: &mut HashMap<String, FileContext>,
    model: Option<&ProjectModel>,
    unit_of_file: &HashMap<String, Option<usize>>,
    index: &DefIndex,
) {
    let Some(model) = model else {
        return;
    };
    let owner = |file: &str| unit_of_file.get(file).copied().flatten();
    // `None` when some declaring file has no owning project.
    let declaring_units: Vec<Option<Vec<usize>>> = index
        .defs
        .iter()
        .map(|def| {
            std::iter::once(def.file.as_str())
                .chain(def.also_in.iter().map(|part| part.file.as_str()))
                .map(owner)
                .collect()
        })
        .collect();
    let mut by_unit: HashMap<usize, Arc<[bool]>> = HashMap::new();
    for (file, context) in contexts.iter_mut() {
        let Some(site) = owner(file) else {
            continue;
        };
        let reach = by_unit.entry(site).or_insert_with(|| {
            declaring_units
                .iter()
                .map(|units| {
                    units
                        .as_ref()
                        .is_none_or(|units| units.iter().any(|&u| model.reachable(site, u)))
                })
                .collect()
        });
        context.usings.set_reach(Arc::clone(reach));
    }
}
