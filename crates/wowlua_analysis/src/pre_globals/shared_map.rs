//! Copy-on-write map storage for `PreResolvedGlobals`.

use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;
use std::sync::Arc;

use crate::collections::HashMap;

/// A map whose `base` entries are shared, read-only, by every clone — the map
/// counterpart of [`super::layered_vec::LayeredVec`]. Entries this layer inserted or
/// edited live in `own` and shadow the base entry of the same key.
pub struct SharedMap<K, V> {
    base: Arc<HashMap<K, V>>,
    own: HashMap<K, V>,
}

impl<K: Hash + Eq, V> SharedMap<K, V> {
    pub fn get<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> Option<&V>
    where K: Borrow<Q> {
        self.own.get(key).or_else(|| self.base.get(key))
    }

    pub fn contains_key<Q: Hash + Eq + ?Sized>(&self, key: &Q) -> bool
    where K: Borrow<Q> {
        self.get(key).is_some()
    }

    pub fn insert(&mut self, key: K, value: V) {
        self.own.insert(key, value);
    }

    /// Every entry: this layer's, then the base entries it doesn't shadow.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.own.iter().chain(self.base.iter().filter(|(k, _)| !self.own.contains_key(*k)))
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// Make every entry part of the shared base, so later clones copy nothing. Only
    /// moves storage when nothing is shared yet (a freshly built map).
    pub(super) fn share(&mut self) {
        if self.base.is_empty() {
            self.base = Arc::new(std::mem::take(&mut self.own));
        }
    }
}

impl<K: Hash + Eq + Clone, V: Clone + Default> SharedMap<K, V> {
    /// The value at `key` for writing, copying the base value (or a default) into
    /// this layer first.
    pub fn entry_or_default(&mut self, key: K) -> &mut V {
        if !self.own.contains_key(&key) {
            let value = self.base.get(&key).cloned().unwrap_or_default();
            self.own.insert(key.clone(), value);
        }
        self.own.get_mut(&key).expect("just inserted")
    }
}

impl<K: Hash + Eq + Clone, V: Clone> SharedMap<K, V> {
    /// Every stored value for writing — including base values another layer's
    /// entries shadow. Unshares the base (the stub generator's path rewrite, which
    /// runs before anything is shared).
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        Arc::make_mut(&mut self.base).values_mut().chain(self.own.values_mut())
    }
}

impl<K, V> Default for SharedMap<K, V> {
    fn default() -> Self {
        SharedMap { base: Arc::new(HashMap::default()), own: HashMap::default() }
    }
}

impl<K: Clone, V: Clone> Clone for SharedMap<K, V> {
    fn clone(&self) -> Self {
        SharedMap { base: Arc::clone(&self.base), own: self.own.clone() }
    }
}

impl<K: Hash + Eq + fmt::Debug, V: fmt::Debug> fmt::Debug for SharedMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

// Serialized exactly like the `HashMap` it replaced; a deserialized map is all base.
impl<K: Hash + Eq + serde::Serialize, V: serde::Serialize> serde::Serialize for SharedMap<K, V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Length up front: bincode can't serialize a map of unknown size.
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.iter() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de, K, V> serde::Deserialize<'de> for SharedMap<K, V>
where K: Hash + Eq + serde::Deserialize<'de>, V: serde::Deserialize<'de> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let base = HashMap::<K, V>::deserialize(deserializer)?;
        Ok(SharedMap { base: Arc::new(base), own: HashMap::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::SharedMap;

    #[test]
    fn clone_shares_base_and_copies_on_write() {
        let mut base: SharedMap<u32, Vec<u32>> = SharedMap::default();
        base.insert(1, vec![1]);
        base.insert(2, vec![2]);
        base.share();
        let mut layer = base.clone();
        layer.entry_or_default(2).push(20);
        layer.entry_or_default(3).push(3);
        assert_eq!(layer.get(&1), Some(&vec![1]));
        assert_eq!(layer.get(&2), Some(&vec![2, 20]));
        assert_eq!(layer.get(&3), Some(&vec![3]));
        assert_eq!(layer.len(), 3);
        assert_eq!(base.get(&2), Some(&vec![2]));
        assert!(!base.contains_key(&3));
    }

    #[test]
    fn serializes_like_a_hash_map() {
        let mut map: SharedMap<u32, u32> = SharedMap::default();
        map.insert(1, 10);
        map.share();
        let mut layer = map.clone();
        layer.insert(1, 11);
        layer.insert(2, 20);
        let bytes = bincode::serialize(&layer).unwrap();
        let plain: crate::collections::HashMap<u32, u32> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(plain.len(), 2);
        assert_eq!(plain[&1], 11);
        let back: SharedMap<u32, u32> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.get(&2), Some(&20));
    }
}
