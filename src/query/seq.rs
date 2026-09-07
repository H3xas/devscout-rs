use std::collections::{HashMap, HashSet};

// ============================================================================
// Order-preserving scratch structures (private -- see module header).
// ============================================================================

/// Insertion-order-preserving set: `insert` is a no-op when the value is
/// already present (first insertion wins the position). Public because it
/// appears in [`ImpactWalkResult`]'s public fields.
#[derive(Debug, Clone)]
pub struct SeqSet<T: Eq + std::hash::Hash + Clone> {
    order: Vec<T>,
    seen: HashSet<T>,
}

impl<T: Eq + std::hash::Hash + Clone> SeqSet<T> {
    /// Creates an empty insertion-ordered set.
    pub fn new() -> Self {
        Self {
            order: Vec::new(),
            seen: HashSet::new(),
        }
    }
    /// Inserts `value` if it is not already present.
    pub fn insert(&mut self, value: T) {
        if self.seen.insert(value.clone()) {
            self.order.push(value);
        }
    }
    /// Returns whether `value` is present.
    pub fn contains(&self, value: &T) -> bool {
        self.seen.contains(value)
    }
    /// Returns the number of values.
    pub fn len(&self) -> usize {
        self.order.len()
    }
    /// Iterates over values in insertion order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.order.iter()
    }
    /// Consumes the set and returns its values in insertion order.
    pub fn into_vec(self) -> Vec<T> {
        self.order
    }
}

impl<T: Eq + std::hash::Hash + Clone> Default for SeqSet<T> {
    fn default() -> Self {
        SeqSet::new()
    }
}

/// Insertion-order-preserving `String`-keyed map with a mutable
/// get-or-insert-default accessor. Same shape as `graph::OrderedMap`, kept as
/// a separate small type here because that one exposes no `get_mut`/entry-style
/// API. Public for the same reason as [`SeqSet`] -- appears in
/// [`ImpactWalkResult::visited`].
#[derive(Debug, Clone)]
pub struct SeqMap<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> SeqMap<V> {
    /// Creates an empty insertion-ordered map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
    /// Returns whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }
    /// Inserts or replaces the value for `key` while preserving key order.
    pub fn insert(&mut self, key: String, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }
    /// Iterates over entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
    /// Iterates over keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }
}

impl<V: Default> SeqMap<V> {
    pub(super) fn get_or_insert_default(&mut self, key: &str) -> &mut V {
        if let Some(&i) = self.index.get(key) {
            return &mut self.entries[i].1;
        }
        self.index.insert(key.to_string(), self.entries.len());
        self.entries.push((key.to_string(), V::default()));
        let last = self.entries.len() - 1;
        &mut self.entries[last].1
    }
}

pub(super) fn push_ordered_unique(map: &mut HashMap<String, Vec<usize>>, key: &str, value: usize) {
    let v = map.entry(key.to_string()).or_default();
    if !v.contains(&value) {
        v.push(value);
    }
}
