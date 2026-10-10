//! Copy-on-write arena storage for `PreResolvedGlobals`.

use std::fmt;
use std::ops::{Index, IndexMut};
use std::sync::Arc;

use crate::collections::HashMap;

/// An IR arena whose leading `base` entries are shared, read-only, by every clone.
///
/// A workspace build starts as a clone of the built-in stub database and then edits a
/// few stub entries and appends its own. Cloning a plain `Vec` copied the whole stub
/// arena into every workspace build, so the server held the stub IR twice (once as the
/// pristine rebuild base, once inside the live globals). Here a clone shares `base`;
/// writing to a base entry copies just that entry into `edited`, and new entries go to
/// `tail`. Indices are unchanged: `0..base.len()` is the base, the rest is the tail.
pub struct LayeredVec<T> {
    /// A slice, not a `Vec`: its length lives in the handle, sparing every read a
    /// pointer hop.
    base: Arc<[T]>,
    /// One bit per base entry, set when it has a private copy in `edited`. Empty
    /// until the first base entry is written. A bitset keeps the check every read
    /// pays small enough to stay in L1.
    edited_bits: Vec<u64>,
    /// Base index → index of its private copy in `edited`.
    edited_slot: HashMap<usize, usize>,
    edited: Vec<T>,
    tail: Vec<T>,
}

impl<T> LayeredVec<T> {
    pub fn new() -> Self {
        Self::from(Vec::new())
    }

    pub fn len(&self) -> usize {
        self.base.len() + self.tail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn push(&mut self, value: T) {
        self.tail.push(value);
    }

    pub fn get(&self, idx: usize) -> Option<&T> {
        (idx < self.len()).then(|| &self[idx])
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.base.iter().enumerate()
            .map(|(i, value)| if self.is_edited(i) { &self.edited[self.edited_slot[&i]] } else { value })
            .chain(self.tail.iter())
    }

    /// Make every entry part of the shared base, so later clones copy nothing. Only
    /// moves storage when nothing is shared yet (a freshly built arena); otherwise the
    /// arena is left as is.
    pub fn share(&mut self) {
        if self.base.is_empty() {
            self.base = Arc::from(std::mem::take(&mut self.tail));
        }
    }

    /// Number of shared base entries that have a private, edited copy.
    pub fn edited_len(&self) -> usize {
        self.edited.len()
    }

    #[inline]
    fn is_edited(&self, base_idx: usize) -> bool {
        self.edited_bits.get(base_idx / 64).is_some_and(|word| word >> (base_idx % 64) & 1 != 0)
    }
}

impl<T> Default for LayeredVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<Vec<T>> for LayeredVec<T> {
    fn from(base: Vec<T>) -> Self {
        LayeredVec {
            base: Arc::from(base),
            edited_bits: Vec::new(),
            edited_slot: HashMap::default(),
            edited: Vec::new(),
            tail: Vec::new(),
        }
    }
}

impl<T> Index<usize> for LayeredVec<T> {
    type Output = T;

    #[inline]
    fn index(&self, idx: usize) -> &T {
        let base_len = self.base.len();
        if idx >= base_len {
            &self.tail[idx - base_len]
        } else if self.is_edited(idx) {
            &self.edited[self.edited_slot[&idx]]
        } else {
            &self.base[idx]
        }
    }
}

impl<T: Clone> IndexMut<usize> for LayeredVec<T> {
    #[inline]
    fn index_mut(&mut self, idx: usize) -> &mut T {
        let base_len = self.base.len();
        if idx >= base_len {
            return &mut self.tail[idx - base_len];
        }
        if self.is_edited(idx) {
            return &mut self.edited[self.edited_slot[&idx]];
        }
        // Not shared (yet): write in place.
        if Arc::get_mut(&mut self.base).is_some() {
            return &mut Arc::get_mut(&mut self.base).expect("base is unshared")[idx];
        }
        if self.edited_bits.is_empty() {
            self.edited_bits = vec![0; base_len.div_ceil(64)];
        }
        self.edited_bits[idx / 64] |= 1 << (idx % 64);
        self.edited_slot.insert(idx, self.edited.len());
        self.edited.push(self.base[idx].clone());
        self.edited.last_mut().expect("just pushed")
    }
}

impl<T: Clone> Clone for LayeredVec<T> {
    fn clone(&self) -> Self {
        LayeredVec {
            base: Arc::clone(&self.base),
            edited_bits: self.edited_bits.clone(),
            edited_slot: self.edited_slot.clone(),
            edited: self.edited.clone(),
            tail: self.tail.clone(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for LayeredVec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

// Serialized exactly like the `Vec<T>` it replaced (the blob is positional bincode);
// a deserialized arena is all shared base.
impl<T: serde::Serialize> serde::Serialize for LayeredVec<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for value in self.iter() {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for LayeredVec<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(LayeredVec::from)
    }
}

#[cfg(test)]
mod tests {
    use super::LayeredVec;

    #[test]
    fn clone_shares_base_and_copies_on_write() {
        let mut base: LayeredVec<String> = LayeredVec::from(vec!["a".to_string(), "b".to_string()]);
        let mut layer = base.clone();
        layer[1].push('!');
        layer.push("c".to_string());
        assert_eq!(layer.iter().cloned().collect::<Vec<_>>(), ["a", "b!", "c"]);
        assert_eq!(layer.edited_len(), 1);
        // The original is untouched by the clone's writes.
        assert_eq!(base.iter().cloned().collect::<Vec<_>>(), ["a", "b"]);
        base[0].push('?');
        assert_eq!(layer[0], "a");
        assert_eq!(base.edited_len(), 1);
        // Once nothing else shares the base, writes land in place.
        drop(layer);
        let mut sole: LayeredVec<String> = LayeredVec::from(vec!["x".to_string()]);
        sole[0].push('!');
        assert_eq!(sole.edited_len(), 0);
        assert_eq!(sole[0], "x!");
        assert_eq!(base.get(2), None);
    }

    #[test]
    fn serializes_like_a_vec() {
        let mut layer: LayeredVec<u32> = LayeredVec::from(vec![1, 2]);
        layer[0] = 10;
        layer.push(3);
        let bytes = bincode::serialize(&layer).unwrap();
        assert_eq!(bytes, bincode::serialize(&vec![10u32, 2, 3]).unwrap());
        let back: LayeredVec<u32> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.iter().copied().collect::<Vec<_>>(), [10, 2, 3]);
    }

    #[test]
    fn share_moves_a_fresh_arena_into_the_base() {
        let mut fresh: LayeredVec<u32> = LayeredVec::new();
        fresh.push(1);
        fresh.push(2);
        fresh.share();
        let mut layer = fresh.clone();
        layer[1] = 20;
        assert_eq!(fresh[1], 2);
        assert_eq!(layer[1], 20);
        assert_eq!(layer.edited_len(), 1);
    }
}
