//! Per-file views of the shared name maps on `PreResolvedGlobals`.

use crate::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::pre_globals::PreResolvedGlobals;

/// One of the ext name maps (`classes`, `aliases`, …) as seen by one file: lookups fall
/// through to the ext map unless the file shadowed or removed the name, and writes stay
/// in the file's own layer. Replaces copying the ext map into every analysis — on a
/// large workspace ~30k classes, several MB per analysis.
#[derive(Clone)]
pub struct LayeredMap<V: 'static> {
    ext: Arc<PreResolvedGlobals>,
    ext_map: fn(&PreResolvedGlobals) -> &HashMap<String, V>,
    /// The file's entries; `None` hides the ext entry of that name.
    local: HashMap<String, Option<V>>,
}

impl<V> LayeredMap<V> {
    pub fn new(ext: Arc<PreResolvedGlobals>, ext_map: fn(&PreResolvedGlobals) -> &HashMap<String, V>) -> Self {
        Self { ext, ext_map, local: HashMap::default() }
    }

    fn ext_map(&self) -> &HashMap<String, V> {
        (self.ext_map)(&self.ext)
    }

    pub fn get(&self, name: &str) -> Option<&V> {
        match self.local.get(name) {
            Some(entry) => entry.as_ref(),
            None => self.ext_map().get(name),
        }
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn insert(&mut self, name: String, value: V) {
        self.local.insert(name, Some(value));
    }

    pub fn remove(&mut self, name: &str) {
        if self.ext_map().contains_key(name) {
            self.local.insert(name.to_string(), None);
        } else {
            self.local.remove(name);
        }
    }

    /// Every visible entry: the file's own, then the ext entries it doesn't shadow.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        let local = self.local.iter().filter_map(|(name, v)| v.as_ref().map(|v| (name, v)));
        let ext = self.ext_map().iter().filter(|(name, _)| !self.local.contains_key(name.as_str()));
        local.chain(ext)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.iter().map(|(name, _)| name)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }

    // No `len()`: unlike the `HashMap::len()` it would mimic, it could only be an
    // `iter().count()` walk of the whole ext map. `is_empty` short-circuits instead.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

impl<V: fmt::Debug> fmt::Debug for LayeredMap<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The ext layer is shared by every analysis; only the file's layer is its own.
        f.debug_struct("LayeredMap").field("local", &self.local).finish_non_exhaustive()
    }
}

/// Read-only name lookup over a plain ext map or a [`LayeredMap`], for helpers shared
/// by the ext build and per-file analysis.
pub trait NameLookup<V> {
    fn lookup(&self, name: &str) -> Option<&V>;
}

impl<V> NameLookup<V> for HashMap<String, V> {
    fn lookup(&self, name: &str) -> Option<&V> {
        self.get(name)
    }
}

impl<V> NameLookup<V> for LayeredMap<V> {
    fn lookup(&self, name: &str) -> Option<&V> {
        self.get(name)
    }
}
