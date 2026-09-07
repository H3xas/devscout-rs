use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ---------------------------------------------------------------------------
// Insertion-ordered string-keyed map -- backs fragments.json's `{rel:
// {mtime, fragment}}` and fragments-index.json's `{rel: mtime}`. A HashMap
// has no defined iteration order and serde_json's default Map serializer
// (the `preserve_order` feature is not enabled) sorts by key; neither
// matches the required file-walk-order construction. Built on serde's own
// SerializeMap/MapAccess traits,
// not a hand-rolled JSON writer -- serde_json still owns the actual byte
// encoding.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
/// Represents `OrderedMap`.
pub struct OrderedMap<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> OrderedMap<V> {
    /// Creates an empty insertion-ordered map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    /// Insert, or overwrite in place when the key already exists --
    /// preserves that key's ORIGINAL position, like assigning to an existing
    /// key on a JSON object.
    pub fn insert(&mut self, key: String, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    /// Returns the value associated with `key`, if present.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates over entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: Serialize> Serialize for OrderedMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for OrderedMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OrderedMapVisitor<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for OrderedMapVisitor<V> {
            type Value = OrderedMap<V>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = OrderedMap::new();
                while let Some((k, v)) = map.next_entry::<String, V>()? {
                    out.insert(k, v);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_map(OrderedMapVisitor(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// `stats.ambiguous_pct` -- see module header. Stored as tenths (an i64) so
// the "is this whole?" branch is exact integer arithmetic, never a float
// epsilon comparison.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents `Percent1`.
pub struct Percent1(pub i64);

impl Percent1 {
    /// Returns a zero percentage value.
    pub fn zero() -> Self {
        Percent1(0)
    }

    /// `ambiguous_pct` as `round((ambiguous / attempts) * 1000) / 10`, or 0
    /// when there are no attempts. Takes the pre-division inputs so the
    /// rounding happens exactly once.
    pub fn from_ratio(numerator: usize, denominator: usize) -> Self {
        if denominator == 0 {
            return Percent1::zero();
        }
        let raw = (numerator as f64 / denominator as f64) * 1000.0;
        Percent1(raw.round() as i64)
    }
}

impl Serialize for Percent1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0 % 10 == 0 {
            serializer.serialize_i64(self.0 / 10)
        } else {
            serializer.serialize_f64(self.0 as f64 / 10.0)
        }
    }
}

impl<'de> Deserialize<'de> for Percent1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(deserializer)?;
        Ok(Percent1((v * 10.0).round() as i64))
    }
}
