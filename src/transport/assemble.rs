//! The graph-rebuild stage: keeps or refuses each provisional site, then
//! builds the facts the kept sites state.
//!
//! A site is kept only when its parent is kept, every written catalogue
//! name qualifies to the entry it was matched against, the file imports the
//! namespace of an extension method it calls, and no repository method
//! shadows a chain start. A refused site takes every site below it along.

use std::path::Path;

use super::catalogue::{
    self, Act, Member, Overload, Val, ATTRIBUTES, DEFINITIONS, FORMATTERS, OPTIONS, TRANSPORTS,
    URI_TYPE,
};
use super::configuration::configured_value;
use super::constants::{Constants, Place, Resolved};
use super::fact_types::{FactProperty, FactType, TransportFact, TransportScope, ValueObj};
use super::inputs::TransportInputs;
use super::qualify::{Ctx, Qual};
use super::scope::{self, Part};
use super::site_types::{SiteFormatter, SiteType, TransportSite};
use super::sites::catalogue_type;
use crate::graph::{Fragment, Graph};
use crate::project::ProjectModel;

/// What a recorded site's `member` key names.
#[derive(Clone, Copy)]
enum Entry {
    Call(&'static Member, &'static Overload),
    Definition,
    Attribute(Val),
}

impl Entry {
    fn act(self) -> Act {
        match self {
            Entry::Call(member, _) => member.act,
            Entry::Definition => Act::Pass,
            Entry::Attribute(_) => Act::Entity,
        }
    }
}

fn entry(key: &str) -> Option<Entry> {
    if let Some((member, overload)) = catalogue::member_by_key(key) {
        return Some(Entry::Call(member, overload));
    }
    if DEFINITIONS.iter().any(|(name, _)| *name == key) {
        return Some(Entry::Definition);
    }
    ATTRIBUTES
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|(_, val, _)| Entry::Attribute(*val))
}

/// Builds every transport fact the fragments' kept sites state, in file
/// order and then site order.
pub(crate) fn assemble(
    root: &Path,
    fragments: &[(String, Fragment)],
    graph: &Graph,
    model: Option<&ProjectModel>,
    inputs: &TransportInputs,
) -> Vec<TransportFact> {
    if fragments.iter().all(|(_, f)| f.transport.is_empty()) {
        return Vec::new();
    }
    let ctx = Ctx::new(root, fragments, graph, model, inputs);
    let constants = Constants::new(&ctx);
    let mut facts = Vec::new();
    for (file, fragment) in fragments {
        if fragment.transport.is_empty() {
            continue;
        }
        let file_facts = FileFacts::new(&ctx, &constants, file, &fragment.transport);
        file_facts.emit(&mut facts);
    }
    facts
}

struct FileFacts<'f, 'c, 'a> {
    ctx: &'c Ctx<'a>,
    constants: &'c Constants<'c, 'a>,
    file: &'f str,
    sites: &'f [TransportSite],
    entries: Vec<Option<Entry>>,
    kept: Vec<bool>,
}

impl<'f, 'c, 'a> FileFacts<'f, 'c, 'a> {
    fn new(
        ctx: &'c Ctx<'a>,
        constants: &'c Constants<'c, 'a>,
        file: &'f str,
        sites: &'f [TransportSite],
    ) -> Self {
        let entries: Vec<Option<Entry>> = sites.iter().map(|s| entry(&s.member)).collect();
        let mut facts = FileFacts {
            ctx,
            constants,
            file,
            sites,
            entries,
            kept: Vec::with_capacity(sites.len()),
        };
        for (index, site) in sites.iter().enumerate() {
            let parent_kept = site.parent.is_none_or(|p| p < index && facts.kept[p]);
            let keep = parent_kept && facts.checks(index);
            facts.kept.push(keep);
        }
        facts
    }

    fn checks(&self, index: usize) -> bool {
        let site = &self.sites[index];
        let Some(entry) = self.entries[index] else {
            return false;
        };
        if let Some(written) = &site.written {
            if !self.names(written, catalogue_type(written), site) {
                return false;
            }
        }
        let Entry::Call(member, overload) = entry else {
            return true;
        };
        if overload.extension
            && !self.ctx.imports(
                self.file,
                &site.namespace,
                catalogue::namespace_of(overload.decl),
            )
        {
            return false;
        }
        if member.act == Act::Options && !self.options_type(site).is_some() {
            return false;
        }
        if member.start && self.ctx.shadows(member.name, self.file, &site.namespace) {
            return false;
        }
        match &site.receiver {
            Some(receiver) if member.start => {
                self.ctx
                    .receiver_fits(receiver, overload.on, self.file, &site.namespace)
            }
            _ => true,
        }
    }

    fn names(&self, written: &str, expected: Option<&'static str>, site: &TransportSite) -> bool {
        expected.is_some_and(|ty| {
            self.ctx
                .names_catalogue(written, ty, self.file, &site.namespace, &site.outer_types)
        })
    }

    // The catalogue options type an options binding's type argument names.
    fn options_type(
        &self,
        site: &TransportSite,
    ) -> Option<&'static (&'static str, &'static [(&'static str, Val)])> {
        let written = &site.types.first()?.name;
        let ty = catalogue_type(written)?;
        if !self.names(written, Some(ty), site) {
            return None;
        }
        OPTIONS.iter().find(|(name, _)| *name == ty)
    }

    fn act(&self, index: usize) -> Option<Act> {
        self.entries[index].map(Entry::act)
    }

    fn ancestor(&self, index: usize, act: Act) -> Option<usize> {
        let mut at = self.sites[index].parent;
        while let Some(p) = at {
            if self.act(p) == Some(act) {
                return Some(p);
            }
            at = self.sites[p].parent;
        }
        None
    }

    fn definition_of(&self, index: usize) -> Option<usize> {
        let mut at = self.sites[index].parent;
        while let Some(p) = at {
            if matches!(self.entries[p], Some(Entry::Definition)) {
                return Some(p);
            }
            at = self.sites[p].parent;
        }
        None
    }

    fn kept_with(&self, act: Act) -> impl Iterator<Item = usize> + '_ {
        (0..self.sites.len()).filter(move |i| self.kept[*i] && self.act(*i) == Some(act))
    }

    fn place(&self, index: usize) -> Place<'_> {
        let site = &self.sites[index];
        Place {
            file: self.file,
            ns: &site.namespace,
            outer: &site.outer_types,
        }
    }

    fn value(&self, index: usize, val: Val) -> Option<Resolved> {
        let site = &self.sites[index];
        let recorded = site.values.iter().find(|v| v.role == val.word())?;
        if let Some(wrapper) = &recorded.wrapper {
            if !self.names(wrapper, Some(URI_TYPE), site) {
                return Some(Resolved::unknown("expression"));
            }
        }
        Some(
            self.constants
                .resolve(&recorded.value, val, &self.place(index)),
        )
    }

    fn fact_type(&self, index: usize, written: &SiteType) -> FactType {
        let site = &self.sites[index];
        let resolved =
            match self
                .ctx
                .qualify(&written.name, self.file, &site.namespace, &site.outer_types)
            {
                Qual::Repo(def) => Some(self.ctx.def(def)),
                _ => None,
            };
        FactType {
            role: written.role.clone(),
            name: written.name.clone(),
            resolved: resolved.map(|d| d.id.clone()),
            resolved_file: resolved.map(|d| d.file.clone()),
            line: None,
        }
    }

    fn types(&self, index: usize) -> Vec<FactType> {
        self.sites[index]
            .types
            .iter()
            .map(|t| self.fact_type(index, t))
            .collect()
    }

    fn base(&self, index: usize, kind: &str) -> TransportFact {
        let site = &self.sites[index];
        let bus = self.ancestor(index, Act::Bus);
        TransportFact {
            file: self.file.to_string(),
            line: site.line,
            kind: kind.to_string(),
            member: site.member.clone(),
            bus_file: bus.map(|_| self.file.to_string()),
            bus_line: bus.map(|b| self.sites[b].line),
            transport: None,
            registration: None,
            types: Vec::new(),
            formatter: None,
            formatter_source: None,
            formatter_arguments: Vec::new(),
            filtered: false,
            address: None,
            key: None,
            properties: Vec::new(),
            scope: None,
        }
    }

    fn transport(&self, bus: usize) -> String {
        let own = match self.entries[bus] {
            Some(Entry::Call(member, _)) => member.word,
            _ => None,
        };
        own.or_else(|| {
            self.kept_with(Act::Transport)
                .filter(|t| self.sites[*t].parent == Some(bus))
                .find_map(|t| match self.entries[t] {
                    Some(Entry::Call(member, _)) => member.word,
                    _ => None,
                })
        })
        .unwrap_or("not-stated")
        .to_string()
    }

    // The transport an address under `index` is in: its bus's, or none.
    fn transport_of(&self, index: usize) -> Option<String> {
        let bus = self.ancestor(index, Act::Bus)?;
        Some(self.transport(bus)).filter(|t| t != "not-stated")
    }

    fn host_transport(&self, index: usize) -> Option<String> {
        if let Some(t) = self.transport_of(index) {
            return Some(t);
        }
        let Some(Entry::Call(_, overload)) = self.entries[index] else {
            return None;
        };
        TRANSPORTS
            .iter()
            .find(|(ty, _)| *ty == overload.on)
            .map(|(_, word)| (*word).to_string())
    }

    fn host_scope(&self, index: usize) -> TransportScope {
        let mut parts = Vec::new();
        for val in [Val::HostName, Val::HostUri, Val::VirtualHost, Val::Region] {
            if let Some(resolved) = self.value(index, val) {
                parts.extend(scope::parts_of(&resolved, val));
            }
        }
        scope::from_parts(&parts, self.host_transport(index).as_deref())
    }

    fn bus_scope(&self, bus: usize) -> TransportScope {
        let hosts: Vec<TransportScope> = self
            .kept_with(Act::Host)
            .filter(|h| self.ancestor(*h, Act::Bus) == Some(bus))
            .map(|h| self.host_scope(h))
            .collect();
        let transport = self.transport(bus);
        let transport = Some(transport.as_str()).filter(|t| *t != "not-stated");
        scope::bus_scope(&hosts, transport)
    }

    fn address_fact(&self, index: usize, kind: &str, val: Val) -> TransportFact {
        let mut fact = self.base(index, kind);
        let resolved = self.value(index, val);
        let own: Vec<(Part, ValueObj)> = match (&resolved, val) {
            (Some(r), Val::Address | Val::EntityName) => scope::parts_of(r, val),
            _ => Vec::new(),
        };
        let bus = self.ancestor(index, Act::Bus);
        let bus_scope = bus.map(|b| self.bus_scope(b));
        let transport = self.transport_of(index);
        fact.scope = Some(scope::address_scope(
            &own,
            bus_scope.as_ref(),
            transport.as_deref(),
        ));
        fact.address = resolved.map(|r| r.obj);
        fact
    }

    fn emit(&self, out: &mut Vec<TransportFact>) {
        for index in 0..self.sites.len() {
            if !self.kept[index] {
                continue;
            }
            if let Some(fact) = self.fact(index) {
                out.push(fact);
            }
        }
    }

    fn fact(&self, index: usize) -> Option<TransportFact> {
        let fact = match self.act(index)? {
            Act::Bus => self.bus_fact(index),
            Act::Host => {
                let mut fact = self.base(index, "host");
                fact.scope = Some(self.host_scope(index));
                fact
            }
            Act::Receive => self.receive_fact(index),
            Act::ConfigureEndpoints => self.configure_endpoints_fact(index),
            Act::Registration => {
                let mut fact = self.base(index, "registration");
                fact.registration = self.word(index);
                fact.types = self.types(index);
                fact
            }
            Act::Send => self.address_fact(index, "send-address", Val::Address),
            Act::Request => self.typed_address(index, "request-address", Val::Address),
            Act::Convention => self.typed_address(index, "endpoint-convention", Val::Address),
            Act::Entity => self.entity_fact(index),
            Act::EndpointName => self.endpoint_name_fact(index),
            Act::Options => self.options_fact(index),
            Act::Outbox => {
                let mut fact = self.base(index, "outbox");
                fact.types = self.types(index);
                fact
            }
            Act::Transport
            | Act::Configured
            | Act::Formatter
            | Act::Topology
            | Act::OptionsKey
            | Act::Pass => return None,
        };
        Some(fact)
    }

    fn word(&self, index: usize) -> Option<String> {
        match self.entries[index] {
            Some(Entry::Call(member, _)) => member.word.map(str::to_string),
            _ => None,
        }
    }

    fn bus_fact(&self, index: usize) -> TransportFact {
        let mut fact = self.base(index, "bus");
        fact.transport = Some(self.transport(index));
        fact.types = self.types(index);
        fact.scope = Some(self.bus_scope(index));
        fact
    }

    fn typed_address(&self, index: usize, kind: &str, val: Val) -> TransportFact {
        let mut fact = self.address_fact(index, kind, val);
        fact.types = self.types(index);
        fact
    }

    fn receive_fact(&self, index: usize) -> TransportFact {
        let mut fact = self.address_fact(index, "receive-endpoint", Val::Name);
        for configured in self.kept_with(Act::Configured) {
            if self.ancestor(configured, Act::Receive) != Some(index) {
                continue;
            }
            for written in &self.sites[configured].types {
                let mut item = self.fact_type(configured, written);
                item.line = Some(self.sites[configured].line);
                fact.types.push(item);
            }
        }
        fact
    }

    fn entity_fact(&self, index: usize) -> TransportFact {
        let val = match self.entries[index] {
            Some(Entry::Attribute(val)) => val,
            _ => Val::EntityName,
        };
        let mut fact = self.address_fact(index, "entity-name", val);
        fact.types = match self.entries[index] {
            Some(Entry::Attribute(_)) => self.types(index),
            _ => self
                .ancestor(index, Act::Topology)
                .map(|t| self.types(t))
                .unwrap_or_default(),
        };
        fact
    }

    fn endpoint_name_fact(&self, index: usize) -> TransportFact {
        let mut fact = self.address_fact(index, "endpoint-name", Val::Name);
        fact.types = self
            .definition_of(index)
            .map(|d| self.types(d))
            .unwrap_or_default();
        fact
    }

    fn formatter_word(&self, index: usize, formatter: &SiteFormatter) -> String {
        let site = &self.sites[index];
        let word = formatter.written.as_ref().and_then(|written| {
            let ty = catalogue_type(written)?;
            self.ctx
                .names_catalogue(written, ty, self.file, &site.namespace, &site.outer_types)
                .then(|| FORMATTERS.iter().find(|(name, _)| *name == ty))
                .flatten()
                .map(|(_, word)| *word)
        });
        word.unwrap_or("unknown").to_string()
    }

    fn formatter_arguments(&self, index: usize, formatter: &SiteFormatter) -> Vec<ValueObj> {
        formatter
            .args
            .iter()
            .map(|raw| {
                self.constants
                    .resolve(raw, Val::FormatterArg, &self.place(index))
                    .obj
            })
            .collect()
    }

    // The formatter in force: the call's own argument, else the last
    // formatter its bus sets, else the default.
    fn formatter_in_force(&self, index: usize) -> (String, &'static str, Vec<ValueObj>) {
        if let Some(formatter) = &self.sites[index].formatter {
            return (
                self.formatter_word(index, formatter),
                "argument",
                self.formatter_arguments(index, formatter),
            );
        }
        let bus = self.ancestor(index, Act::Bus);
        let set = bus.and_then(|b| {
            self.kept_with(Act::Formatter)
                .filter(|f| self.ancestor(*f, Act::Bus) == Some(b))
                .last()
        });
        match set {
            Some(f) => match &self.sites[f].formatter {
                Some(formatter) => (
                    self.formatter_word(f, formatter),
                    "bus",
                    self.formatter_arguments(f, formatter),
                ),
                None => (
                    self.word(f).unwrap_or_else(|| "unknown".to_string()),
                    "bus",
                    Vec::new(),
                ),
            },
            None => ("default".to_string(), "default", Vec::new()),
        }
    }

    fn configure_endpoints_fact(&self, index: usize) -> TransportFact {
        let mut fact = self.base(index, "configure-endpoints");
        let (word, source, arguments) = self.formatter_in_force(index);
        fact.formatter = Some(word);
        fact.formatter_source = Some(source.to_string());
        fact.formatter_arguments = arguments;
        fact.filtered = self.sites[index].filtered;
        fact
    }

    fn settings_files(&self) -> Vec<String> {
        let unit = self
            .ctx
            .model
            .and_then(|m| m.unit_of_file(self.file).map(|u| m.units[u].id.clone()));
        unit.and_then(|id| self.ctx.inputs.units.iter().find(|u| u.unit == id))
            .map(|u| u.settings.iter().map(|s| s.file.clone()).collect())
            .unwrap_or_default()
    }

    fn options_fact(&self, index: usize) -> TransportFact {
        let mut fact = self.base(index, "options-binding");
        fact.types = self.types(index);
        let keyed = self
            .value(index, Val::Key)
            .or_else(|| {
                (0..self.sites.len())
                    .filter(|k| self.kept[*k] && self.sites[*k].parent == Some(index))
                    .find(|k| self.act(*k) == Some(Act::OptionsKey))
                    .and_then(|k| self.value(k, Val::Key))
            })
            .map(|r| r.obj);
        if let (Some(ValueObj::Configuration { key, .. }), Some((_, marked))) =
            (&keyed, self.options_type(&self.sites[index]))
        {
            let files = self.settings_files();
            fact.properties = marked
                .iter()
                .map(|(name, val)| FactProperty {
                    name: (*name).to_string(),
                    role: val.word().to_string(),
                    value: configured_value(self.ctx.root, &files, &format!("{key}:{name}"), *val),
                })
                .collect();
        }
        fact.key = keyed;
        fact
    }
}
