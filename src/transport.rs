//! MassTransit transport configuration facts, recognized through a closed
//! catalogue of package types.
//!
//! Recognition runs in two stages. The extractor walks each C# file once and
//! records provisional sites in its fragment (`sites`): calls, assignments,
//! definition base types and attributes whose receiver the walk can type as a
//! catalogue type. At graph rebuild, `assemble` keeps a site only when the
//! file's own usings, its project's global usings and the SDK's implicit
//! usings qualify every written name to the catalogue entry it was matched
//! against, and no repository declaration shadows it. It then resolves
//! constants and configuration values and builds each transport scope. The
//! kept facts are written to `graph.json` and exported as the
//! `transport-address` family.

mod assemble;
mod catalogue;
mod configuration;
mod constants;
mod fact_types;
mod inputs;
mod qualify;
mod scope;
mod site_types;
mod sites;
mod sites_call;
mod syntax;
mod values;

pub(crate) use assemble::assemble;
pub use fact_types::{
    ConfigValue, FactProperty, FactType, FieldRef, TransportFact, TransportScope, ValueObj,
};
pub(crate) use inputs::{discover, sidecar_differs, write_sidecar};
pub use inputs::{SettingsFile, TransportInputs, UnitInputs};
pub use site_types::{Parsed, Piece, RawValue, SiteFormatter, SiteType, SiteValue, TransportSite};
pub use sites::sites;

/// One catalogue entry as the catalogue declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueEntry {
    /// The fully qualified type, or the declaring type and member name.
    pub name: String,
    /// `type` or `member`.
    pub kind: &'static str,
    /// The role word.
    pub role: &'static str,
}

/// Every type and every member overload's declaring name in the catalogue,
/// in declaration order. A member with overloads in several declaring types
/// has one entry per declaring type.
pub fn catalogue_entries() -> Vec<CatalogueEntry> {
    let mut entries: Vec<CatalogueEntry> = catalogue::TYPES
        .iter()
        .map(|t| CatalogueEntry {
            name: t.name.to_string(),
            kind: "type",
            role: t.role.word(),
        })
        .collect();
    for member in catalogue::members() {
        for overload in member.overloads {
            let name = format!("{}.{}", overload.decl, member.name);
            if !entries.iter().any(|e| e.name == name) {
                entries.push(CatalogueEntry {
                    name,
                    kind: "member",
                    role: member.role.word(),
                });
            }
        }
    }
    entries
}

/// The closed role word list a catalogue entry's role comes from.
pub fn role_words() -> &'static [&'static str] {
    &catalogue::ROLE_WORDS
}

/// The closed list of type role words a fact's `types` can carry.
pub fn type_role_words() -> &'static [&'static str] {
    &catalogue::TYPE_ROLE_WORDS
}

/// The closed list of reasons an unknown value can carry.
pub fn unknown_reasons() -> &'static [&'static str] {
    &values::REASONS
}
