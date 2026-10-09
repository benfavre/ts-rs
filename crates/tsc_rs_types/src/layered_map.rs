//! A name-keyed registry in two layers: a shared, immutable base and a
//! private overlay.
//!
//! The project-wide registries (classes, interfaces, type aliases, enums)
//! are built once by the donor checker and then handed to one checker per
//! file. They used to be `Arc<FxHashMap>`s written through `Arc::make_mut`,
//! so the first file-local write — registering the file's own interface,
//! merging an augmentation — deep-copied the whole project's map, and the
//! checker's drop freed it again: about a third of a project check's CPU.
//!
//! Here a clone shares the base and copies only the overlay (empty after
//! [`LayeredMap::freeze`]); a write goes to the overlay, and changing a base
//! entry copies that one entry. Entries are never removed.

use std::sync::Arc;

use rustc_hash::FxHashMap;

#[derive(Debug)]
pub(crate) struct LayeredMap<V> {
    base: Arc<FxHashMap<String, V>>,
    overlay: FxHashMap<String, V>,
}

impl<V> Default for LayeredMap<V> {
    fn default() -> Self {
        Self {
            base: Arc::default(),
            overlay: FxHashMap::default(),
        }
    }
}

impl<V: Clone> Clone for LayeredMap<V> {
    fn clone(&self) -> Self {
        Self {
            base: Arc::clone(&self.base),
            overlay: self.overlay.clone(),
        }
    }
}

impl<V> LayeredMap<V> {
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.overlay.contains_key(key) || self.base.contains_key(key)
    }

    pub(crate) fn get(&self, key: &str) -> Option<&V> {
        self.overlay.get(key).or_else(|| self.base.get(key))
    }

    /// Every entry once: the overlay's, then the base's that it does not
    /// shadow.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.overlay.iter().chain(
            self.base
                .iter()
                .filter(|(key, _)| !self.overlay.contains_key(*key)),
        )
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = &String> {
        self.iter().map(|(key, _)| key)
    }

    pub(crate) fn insert(&mut self, key: String, value: V) {
        self.overlay.insert(key, value);
    }

    /// True when both maps read the same shared base (tests).
    #[cfg(test)]
    pub(crate) fn shares_base_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.base, &other.base)
    }
}

impl<V> std::ops::Index<&str> for LayeredMap<V> {
    type Output = V;

    fn index(&self, key: &str) -> &V {
        self.get(key).expect("no entry found for key")
    }
}

impl<V: Clone> LayeredMap<V> {
    /// Mutable access to an entry; a base entry is first copied into the
    /// overlay.
    pub(crate) fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        if !self.overlay.contains_key(key) {
            let value = self.base.get(key)?.clone();
            self.overlay.insert(key.to_string(), value);
        }
        self.overlay.get_mut(key)
    }

    /// `entry(key).or_insert_with(make)`.
    pub(crate) fn get_or_insert_with(&mut self, key: String, make: impl FnOnce() -> V) -> &mut V {
        if !self.overlay.contains_key(&key) {
            let value = match self.base.get(&key) {
                Some(value) => value.clone(),
                None => make(),
            };
            return self.overlay.entry(key).or_insert(value);
        }
        self.overlay.get_mut(&key).expect("checked above")
    }

    /// Moves the overlay into the base, so clones made afterwards share
    /// everything. Call when a checker is done being built and is about to
    /// be cloned per file.
    pub(crate) fn freeze(&mut self) {
        if self.overlay.is_empty() {
            return;
        }
        let overlay = std::mem::take(&mut self.overlay);
        if self.base.is_empty() {
            self.base = Arc::new(overlay);
        } else {
            Arc::make_mut(&mut self.base).extend(overlay);
        }
    }
}

/// An append-only arena in the same two layers: indices below the base's
/// length read the shared base, the rest a private tail. A clone shares
/// the base and copies only the tail.
#[derive(Debug)]
pub(crate) struct LayeredArena<T> {
    base: Arc<Vec<T>>,
    tail: Vec<T>,
}

impl<T: Clone> Clone for LayeredArena<T> {
    fn clone(&self) -> Self {
        Self {
            base: Arc::clone(&self.base),
            tail: self.tail.clone(),
        }
    }
}

impl<T> LayeredArena<T> {
    pub(crate) fn new(items: Vec<T>) -> Self {
        Self {
            base: Arc::default(),
            tail: items,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.base.len() + self.tail.len()
    }

    pub(crate) fn push(&mut self, item: T) {
        self.tail.push(item);
    }
}

impl<T: Clone> LayeredArena<T> {
    /// Moves the tail into the base (indices are unchanged). See
    /// [`LayeredMap::freeze`].
    pub(crate) fn freeze(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        let tail = std::mem::take(&mut self.tail);
        if self.base.is_empty() {
            self.base = Arc::new(tail);
        } else {
            Arc::make_mut(&mut self.base).extend(tail);
        }
    }
}

impl<T> std::ops::Index<usize> for LayeredArena<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        match index.checked_sub(self.base.len()) {
            Some(local) => &self.tail[local],
            None => &self.base[index],
        }
    }
}
