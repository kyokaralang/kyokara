//! Runtime value representation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use kyokara_hir_def::body::Body;
use kyokara_hir_def::expr::{ExprIdx, PatIdx};
use kyokara_hir_def::item_tree::{FnItemIdx, TypeItemIdx};
use kyokara_hir_def::name::Name;
use kyokara_intern::Interner;

use crate::env::Env;
use crate::error::RuntimeError;
use crate::intrinsics::IntrinsicFn;

/// Map key — only types that are naturally hashable.
///
/// Mirrors Rust's own constraint: `HashMap<K, V>` requires `K: Hash + Eq`.
/// Floats, functions, lists, maps, ADTs, and records are not valid keys.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MapKey {
    Int(i64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
}

impl MapKey {
    /// Convert a `Value` to a `MapKey`, rejecting unhashable types.
    pub fn from_value(v: &Value) -> Result<Self, RuntimeError> {
        match v {
            Value::Int(n) => Ok(MapKey::Int(*n)),
            Value::String(s) => Ok(MapKey::String(s.clone())),
            Value::Char(c) => Ok(MapKey::Char(*c)),
            Value::Bool(b) => Ok(MapKey::Bool(*b)),
            Value::Unit => Ok(MapKey::Unit),
            _ => Err(RuntimeError::TypeError(
                "unhashable type used as map key (only Int, String, Char, Bool, Unit are allowed)"
                    .into(),
            )),
        }
    }

    /// Convert back to a `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            MapKey::Int(n) => Value::Int(*n),
            MapKey::String(s) => Value::String(s.clone()),
            MapKey::Char(c) => Value::Char(*c),
            MapKey::Bool(b) => Value::Bool(*b),
            MapKey::Unit => Value::Unit,
        }
    }

    pub fn display(&self, _interner: &Interner) -> String {
        self.to_value().display(_interner)
    }

    pub fn primitive_hash(&self) -> i64 {
        primitive_map_key_hash(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub hash: i64,
    pub key: Value,
    pub value: Value,
}

#[derive(Debug, Clone, Default)]
struct GenericMapValue {
    entries: Vec<MapEntry>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PartialEq for GenericMapValue {
    fn eq(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }

        self.entries.iter().all(|entry| {
            other
                .entries
                .iter()
                .find(|candidate| candidate.hash == entry.hash && candidate.key == entry.key)
                .is_some_and(|candidate| candidate.value == entry.value)
        })
    }
}

impl Eq for GenericMapValue {}

impl GenericMapValue {
    fn push_bucket_index(&mut self, hash: i64, idx: usize) {
        self.buckets.entry(hash).or_default().push(idx);
    }

    fn remove_bucket_index(&mut self, hash: i64, idx: usize) {
        let mut remove_bucket = false;
        if let Some(indices) = self.buckets.get_mut(&hash) {
            if let Some(pos) = indices.iter().position(|&candidate| candidate == idx) {
                indices.remove(pos);
            }
            remove_bucket = indices.is_empty();
        }
        if remove_bucket {
            self.buckets.remove(&hash);
        }
    }

    fn shift_bucket_indices_after_removal(&mut self, removed_idx: usize) {
        self.buckets.retain(|_, indices| {
            for idx in indices.iter_mut() {
                if *idx > removed_idx {
                    *idx -= 1;
                }
            }
            !indices.is_empty()
        });
    }

    pub fn from_entries(entries: Vec<MapEntry>) -> Self {
        let mut buckets: HashMap<i64, Vec<usize>> = HashMap::new();
        for (idx, entry) in entries.iter().enumerate() {
            buckets.entry(entry.hash).or_default().push(idx);
        }
        Self { entries, buckets }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn key_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.key.clone())
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.value.clone())
    }

    pub fn find_index_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Option<usize>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        let Some(indices) = self.buckets.get(&hash) else {
            return Ok(None);
        };
        for &idx in indices {
            let entry = &self.entries[idx];
            if eq(&entry.key, key)? {
                return Ok(Some(idx));
            }
        }
        Ok(None)
    }

    pub fn get_cloned_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Option<Value>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        Ok(self
            .find_index_with(hash, key, eq)?
            .map(|idx| self.entries[idx].value.clone()))
    }

    pub fn contains_with<E>(&self, hash: i64, key: &Value, eq: &mut E) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        Ok(self.find_index_with(hash, key, eq)?.is_some())
    }

    pub fn insert_persistent_with<E>(
        &self,
        hash: i64,
        key: Value,
        value: Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        let mut entries = self.entries.clone();
        if let Some(idx) = self.find_index_with(hash, &key, eq)? {
            if entries[idx].value == value {
                return Ok(self.clone());
            }
            entries[idx].value = value;
            return Ok(Self::from_entries(entries));
        }
        entries.push(MapEntry { hash, key, value });
        Ok(Self::from_entries(entries))
    }

    pub fn remove_persistent_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        let Some(idx) = self.find_index_with(hash, key, eq)? else {
            return Ok(self.clone());
        };
        let mut entries = self.entries.clone();
        entries.remove(idx);
        Ok(Self::from_entries(entries))
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        key: Value,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Some(idx) = self.find_index_with(hash, &key, eq)? {
            self.entries[idx].value = value;
        } else {
            let idx = self.entries.len();
            self.entries.push(MapEntry { hash, key, value });
            self.push_bucket_index(hash, idx);
        }
        Ok(())
    }

    pub fn remove_with<E>(&mut self, hash: i64, key: &Value, eq: &mut E) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Some(idx) = self.find_index_with(hash, key, eq)? {
            let hash = self.entries[idx].hash;
            self.entries.remove(idx);
            self.remove_bucket_index(hash, idx);
            self.shift_bucket_indices_after_removal(idx);
        }
        Ok(())
    }

    pub fn get(&self, key: &MapKey) -> Option<&Value> {
        let hash = key.primitive_hash();
        let key_value = key.to_value();
        self.buckets.get(&hash).and_then(|indices| {
            indices.iter().find_map(|&idx| {
                let entry = &self.entries[idx];
                (entry.key == key_value).then_some(&entry.value)
            })
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PrimitivePersistentMapValue {
    entries: IndexMap<MapKey, Value>,
}

impl PrimitivePersistentMapValue {
    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }

    fn from_indexmap(entries: IndexMap<MapKey, Value>) -> Self {
        Self { entries }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn key_at(&self, idx: usize) -> Option<Value> {
        self.entries.get_index(idx).map(|(key, _)| key.to_value())
    }

    fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get_index(idx).map(|(_, value)| value.clone())
    }

    fn snapshot_entries(&self) -> Vec<MapEntry> {
        self.entries
            .iter()
            .map(|(key, value)| MapEntry {
                hash: key.primitive_hash(),
                key: key.to_value(),
                value: value.clone(),
            })
            .collect()
    }

    #[cfg(test)]
    fn snapshot(&self) -> MapValue {
        MapValue::from_primitive_storage(self.clone())
    }

    fn to_generic(&self) -> GenericMapValue {
        GenericMapValue::from_entries(self.snapshot_entries())
    }

    fn find_index(&self, key: &MapKey) -> Option<usize> {
        self.entries.get_index_of(key)
    }

    fn get_cloned(&self, key: &MapKey) -> Option<Value> {
        self.entries.get(key).cloned()
    }

    fn contains(&self, key: &MapKey) -> bool {
        self.entries.contains_key(key)
    }

    fn insert_persistent(&self, key: MapKey, value: Value) -> Self {
        let mut entries = self.entries.clone();
        if entries.get(&key).is_some_and(|current| current == &value) {
            return self.clone();
        }
        entries.insert(key, value);
        Self { entries }
    }

    fn remove_persistent(&self, key: &MapKey) -> Self {
        if !self.entries.contains_key(key) {
            return self.clone();
        }
        let mut entries = self.entries.clone();
        entries.shift_remove(key);
        Self { entries }
    }

    fn insert(&mut self, key: MapKey, value: Value) {
        self.entries.insert(key, value);
    }

    fn remove(&mut self, key: &MapKey) {
        self.entries.shift_remove(key);
    }
}

#[derive(Debug, Clone)]
enum PersistentMapStorage {
    Primitive(PrimitivePersistentMapValue),
    Generic(GenericMapValue),
}

#[derive(Debug, Clone)]
pub struct MapValue {
    storage: PersistentMapStorage,
}

impl Default for MapValue {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for MapValue {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        let mut eq = |lhs: &Value, rhs: &Value| Ok(lhs == rhs);
        self.snapshot_entries().into_iter().all(|entry| {
            other
                .get_cloned_with(entry.hash, &entry.key, &mut eq)
                .ok()
                .flatten()
                .is_some_and(|candidate| candidate == entry.value)
        })
    }
}

impl Eq for MapValue {}

impl MapValue {
    fn from_primitive_storage(entries: PrimitivePersistentMapValue) -> Self {
        Self {
            storage: PersistentMapStorage::Primitive(entries),
        }
    }

    fn from_generic_storage(entries: GenericMapValue) -> Self {
        Self {
            storage: PersistentMapStorage::Generic(entries),
        }
    }

    pub fn new() -> Self {
        Self::from_primitive_storage(PrimitivePersistentMapValue::default())
    }

    pub fn from_primitive_indexmap(entries: IndexMap<MapKey, Value>) -> Self {
        Self::from_primitive_storage(PrimitivePersistentMapValue::from_indexmap(entries))
    }

    pub fn from_entries(entries: Vec<MapEntry>) -> Self {
        Self::from_generic_storage(GenericMapValue::from_entries(entries))
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            PersistentMapStorage::Primitive(entries) => entries.len(),
            PersistentMapStorage::Generic(entries) => entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn key_at(&self, idx: usize) -> Option<Value> {
        match &self.storage {
            PersistentMapStorage::Primitive(entries) => entries.key_at(idx),
            PersistentMapStorage::Generic(entries) => entries.key_at(idx),
        }
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        match &self.storage {
            PersistentMapStorage::Primitive(entries) => entries.value_at(idx),
            PersistentMapStorage::Generic(entries) => entries.value_at(idx),
        }
    }

    pub fn snapshot_entries(&self) -> Vec<MapEntry> {
        match &self.storage {
            PersistentMapStorage::Primitive(entries) => entries.snapshot_entries(),
            PersistentMapStorage::Generic(entries) => entries.entries.clone(),
        }
    }

    pub fn entries(&self) -> Vec<MapEntry> {
        self.snapshot_entries()
    }

    pub fn find_index_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Option<usize>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(match &self.storage {
                PersistentMapStorage::Primitive(entries) => entries.find_index(&primitive),
                PersistentMapStorage::Generic(entries) => entries.find_index_with(hash, key, eq)?,
            });
        }
        match &self.storage {
            PersistentMapStorage::Primitive(_) => Ok(None),
            PersistentMapStorage::Generic(entries) => entries.find_index_with(hash, key, eq),
        }
    }

    pub fn get_cloned_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Option<Value>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(match &self.storage {
                PersistentMapStorage::Primitive(entries) => entries.get_cloned(&primitive),
                PersistentMapStorage::Generic(entries) => entries.get(&primitive).cloned(),
            });
        }
        match &self.storage {
            PersistentMapStorage::Primitive(_) => Ok(None),
            PersistentMapStorage::Generic(entries) => entries.get_cloned_with(hash, key, eq),
        }
    }

    pub fn contains_with<E>(&self, hash: i64, key: &Value, eq: &mut E) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(match &self.storage {
                PersistentMapStorage::Primitive(entries) => entries.contains(&primitive),
                PersistentMapStorage::Generic(entries) => entries.get(&primitive).is_some(),
            });
        }
        match &self.storage {
            PersistentMapStorage::Primitive(_) => Ok(false),
            PersistentMapStorage::Generic(entries) => entries.contains_with(hash, key, eq),
        }
    }

    pub fn insert_persistent_with<E>(
        &self,
        hash: i64,
        key: Value,
        value: Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&key) {
            return Ok(match &self.storage {
                PersistentMapStorage::Primitive(entries) => {
                    Self::from_primitive_storage(entries.insert_persistent(primitive, value))
                }
                PersistentMapStorage::Generic(entries) => {
                    let mut generic = entries.clone();
                    generic.insert_with(hash, key, value, eq)?;
                    Self::from_generic_storage(generic)
                }
            });
        }

        match &self.storage {
            PersistentMapStorage::Primitive(entries) => {
                let mut generic = entries.to_generic();
                generic.insert_with(hash, key, value, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
            PersistentMapStorage::Generic(entries) => {
                let generic = entries.insert_persistent_with(hash, key, value, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
        }
    }

    pub fn remove_persistent_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(match &self.storage {
                PersistentMapStorage::Primitive(entries) => {
                    Self::from_primitive_storage(entries.remove_persistent(&primitive))
                }
                PersistentMapStorage::Generic(entries) => {
                    let generic = entries.remove_persistent_with(hash, key, eq)?;
                    Self::from_generic_storage(generic)
                }
            });
        }

        match &self.storage {
            PersistentMapStorage::Primitive(_) => Ok(self.clone()),
            PersistentMapStorage::Generic(entries) => {
                let generic = entries.remove_persistent_with(hash, key, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
        }
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        key: Value,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&key) {
            match &mut self.storage {
                PersistentMapStorage::Primitive(entries) => entries.insert(primitive, value),
                PersistentMapStorage::Generic(entries) => {
                    entries.insert_with(hash, key, value, eq)?;
                }
            }
            return Ok(());
        }

        match &mut self.storage {
            PersistentMapStorage::Primitive(entries) => {
                let mut generic = entries.to_generic();
                generic.insert_with(hash, key, value, eq)?;
                self.storage = PersistentMapStorage::Generic(generic);
            }
            PersistentMapStorage::Generic(entries) => {
                entries.insert_with(hash, key, value, eq)?;
            }
        }
        Ok(())
    }

    pub fn remove_with<E>(&mut self, hash: i64, key: &Value, eq: &mut E) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            match &mut self.storage {
                PersistentMapStorage::Primitive(entries) => entries.remove(&primitive),
                PersistentMapStorage::Generic(entries) => {
                    entries.remove_with(hash, key, eq)?;
                }
            }
            return Ok(());
        }

        if let PersistentMapStorage::Generic(entries) = &mut self.storage {
            entries.remove_with(hash, key, eq)?;
        }
        Ok(())
    }

    pub fn get(&self, key: &MapKey) -> Option<&Value> {
        match &self.storage {
            PersistentMapStorage::Primitive(entries) => entries.entries.get(key),
            PersistentMapStorage::Generic(entries) => entries.get(key),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEntry {
    pub hash: i64,
    pub value: Value,
}

#[derive(Debug, Clone, Default)]
struct GenericSetValue {
    entries: Vec<SetEntry>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PartialEq for GenericSetValue {
    fn eq(&self, other: &Self) -> bool {
        if self.entries.len() != other.entries.len() {
            return false;
        }

        self.entries.iter().all(|entry| {
            other
                .entries
                .iter()
                .any(|candidate| candidate.hash == entry.hash && candidate.value == entry.value)
        })
    }
}

impl Eq for GenericSetValue {}

impl GenericSetValue {
    fn push_bucket_index(&mut self, hash: i64, idx: usize) {
        self.buckets.entry(hash).or_default().push(idx);
    }

    fn remove_bucket_index(&mut self, hash: i64, idx: usize) {
        let mut remove_bucket = false;
        if let Some(indices) = self.buckets.get_mut(&hash) {
            if let Some(pos) = indices.iter().position(|&candidate| candidate == idx) {
                indices.remove(pos);
            }
            remove_bucket = indices.is_empty();
        }
        if remove_bucket {
            self.buckets.remove(&hash);
        }
    }

    fn shift_bucket_indices_after_removal(&mut self, removed_idx: usize) {
        self.buckets.retain(|_, indices| {
            for idx in indices.iter_mut() {
                if *idx > removed_idx {
                    *idx -= 1;
                }
            }
            !indices.is_empty()
        });
    }

    pub fn from_entries(entries: Vec<SetEntry>) -> Self {
        let mut buckets: HashMap<i64, Vec<usize>> = HashMap::new();
        for (idx, entry) in entries.iter().enumerate() {
            buckets.entry(entry.hash).or_default().push(idx);
        }
        Self { entries, buckets }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.value.clone())
    }

    pub fn find_index_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<Option<usize>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        let Some(indices) = self.buckets.get(&hash) else {
            return Ok(None);
        };
        for &idx in indices {
            let entry = &self.entries[idx];
            if eq(&entry.value, value)? {
                return Ok(Some(idx));
            }
        }
        Ok(None)
    }

    pub fn contains_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        Ok(self.find_index_with(hash, value, eq)?.is_some())
    }

    pub fn insert_persistent_with<E>(
        &self,
        hash: i64,
        value: Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if self.find_index_with(hash, &value, eq)?.is_some() {
            return Ok(self.clone());
        }
        let mut entries = self.entries.clone();
        entries.push(SetEntry { hash, value });
        Ok(Self::from_entries(entries))
    }

    pub fn remove_persistent_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        let Some(idx) = self.find_index_with(hash, value, eq)? else {
            return Ok(self.clone());
        };
        let mut entries = self.entries.clone();
        entries.remove(idx);
        Ok(Self::from_entries(entries))
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if self.find_index_with(hash, &value, eq)?.is_none() {
            let idx = self.entries.len();
            self.entries.push(SetEntry { hash, value });
            self.push_bucket_index(hash, idx);
        }
        Ok(())
    }

    pub fn remove_with<E>(
        &mut self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Some(idx) = self.find_index_with(hash, value, eq)? {
            let hash = self.entries[idx].hash;
            self.entries.remove(idx);
            self.remove_bucket_index(hash, idx);
            self.shift_bucket_indices_after_removal(idx);
        }
        Ok(())
    }

    pub fn contains(&self, key: &MapKey) -> bool {
        let hash = key.primitive_hash();
        let key_value = key.to_value();
        self.buckets.get(&hash).is_some_and(|indices| {
            indices
                .iter()
                .any(|&idx| self.entries[idx].value == key_value)
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PrimitivePersistentSetValue {
    entries: IndexSet<MapKey>,
}

impl PrimitivePersistentSetValue {
    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }

    fn from_indexset(entries: IndexSet<MapKey>) -> Self {
        Self { entries }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get_index(idx).map(MapKey::to_value)
    }

    fn snapshot_entries(&self) -> Vec<SetEntry> {
        self.entries
            .iter()
            .map(|key| SetEntry {
                hash: key.primitive_hash(),
                value: key.to_value(),
            })
            .collect()
    }

    #[cfg(test)]
    fn snapshot(&self) -> SetValue {
        SetValue::from_primitive_storage(self.clone())
    }

    fn to_generic(&self) -> GenericSetValue {
        GenericSetValue::from_entries(self.snapshot_entries())
    }

    fn find_index(&self, key: &MapKey) -> Option<usize> {
        self.entries.get_index_of(key)
    }

    fn contains(&self, key: &MapKey) -> bool {
        self.entries.contains(key)
    }

    fn insert_persistent(&self, key: MapKey) -> Self {
        if self.entries.contains(&key) {
            return self.clone();
        }
        let mut entries = self.entries.clone();
        entries.insert(key);
        Self { entries }
    }

    fn remove_persistent(&self, key: &MapKey) -> Self {
        if !self.entries.contains(key) {
            return self.clone();
        }
        let mut entries = self.entries.clone();
        entries.shift_remove(key);
        Self { entries }
    }

    fn insert(&mut self, key: MapKey) {
        self.entries.insert(key);
    }

    fn remove(&mut self, key: &MapKey) {
        self.entries.shift_remove(key);
    }
}

#[derive(Debug, Clone)]
enum PersistentSetStorage {
    Primitive(PrimitivePersistentSetValue),
    Generic(GenericSetValue),
}

#[derive(Debug, Clone)]
pub struct SetValue {
    storage: PersistentSetStorage,
}

impl Default for SetValue {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for SetValue {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        let mut eq = |lhs: &Value, rhs: &Value| Ok(lhs == rhs);
        self.snapshot_entries().into_iter().all(|entry| {
            other
                .contains_with(entry.hash, &entry.value, &mut eq)
                .ok()
                .unwrap_or(false)
        })
    }
}

impl Eq for SetValue {}

impl SetValue {
    fn from_primitive_storage(entries: PrimitivePersistentSetValue) -> Self {
        Self {
            storage: PersistentSetStorage::Primitive(entries),
        }
    }

    fn from_generic_storage(entries: GenericSetValue) -> Self {
        Self {
            storage: PersistentSetStorage::Generic(entries),
        }
    }

    pub fn new() -> Self {
        Self::from_primitive_storage(PrimitivePersistentSetValue::default())
    }

    pub fn from_primitive_indexset(entries: IndexSet<MapKey>) -> Self {
        Self::from_primitive_storage(PrimitivePersistentSetValue::from_indexset(entries))
    }

    pub fn from_entries(entries: Vec<SetEntry>) -> Self {
        Self::from_generic_storage(GenericSetValue::from_entries(entries))
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            PersistentSetStorage::Primitive(entries) => entries.len(),
            PersistentSetStorage::Generic(entries) => entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        match &self.storage {
            PersistentSetStorage::Primitive(entries) => entries.value_at(idx),
            PersistentSetStorage::Generic(entries) => entries.value_at(idx),
        }
    }

    pub fn snapshot_entries(&self) -> Vec<SetEntry> {
        match &self.storage {
            PersistentSetStorage::Primitive(entries) => entries.snapshot_entries(),
            PersistentSetStorage::Generic(entries) => entries.entries.clone(),
        }
    }

    pub fn entries(&self) -> Vec<SetEntry> {
        self.snapshot_entries()
    }

    pub fn find_index_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<Option<usize>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            return Ok(match &self.storage {
                PersistentSetStorage::Primitive(entries) => entries.find_index(&primitive),
                PersistentSetStorage::Generic(entries) => {
                    entries.find_index_with(hash, value, eq)?
                }
            });
        }
        match &self.storage {
            PersistentSetStorage::Primitive(_) => Ok(None),
            PersistentSetStorage::Generic(entries) => entries.find_index_with(hash, value, eq),
        }
    }

    pub fn contains_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            return Ok(match &self.storage {
                PersistentSetStorage::Primitive(entries) => entries.contains(&primitive),
                PersistentSetStorage::Generic(entries) => entries.contains(&primitive),
            });
        }
        match &self.storage {
            PersistentSetStorage::Primitive(_) => Ok(false),
            PersistentSetStorage::Generic(entries) => entries.contains_with(hash, value, eq),
        }
    }

    pub fn insert_persistent_with<E>(
        &self,
        hash: i64,
        value: Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&value) {
            return Ok(match &self.storage {
                PersistentSetStorage::Primitive(entries) => {
                    Self::from_primitive_storage(entries.insert_persistent(primitive))
                }
                PersistentSetStorage::Generic(entries) => {
                    let generic = entries.insert_persistent_with(hash, value, eq)?;
                    Self::from_generic_storage(generic)
                }
            });
        }

        match &self.storage {
            PersistentSetStorage::Primitive(entries) => {
                let mut generic = entries.to_generic();
                generic.insert_with(hash, value, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
            PersistentSetStorage::Generic(entries) => {
                let generic = entries.insert_persistent_with(hash, value, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
        }
    }

    pub fn remove_persistent_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<Self, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            return Ok(match &self.storage {
                PersistentSetStorage::Primitive(entries) => {
                    Self::from_primitive_storage(entries.remove_persistent(&primitive))
                }
                PersistentSetStorage::Generic(entries) => {
                    let generic = entries.remove_persistent_with(hash, value, eq)?;
                    Self::from_generic_storage(generic)
                }
            });
        }

        match &self.storage {
            PersistentSetStorage::Primitive(_) => Ok(self.clone()),
            PersistentSetStorage::Generic(entries) => {
                let generic = entries.remove_persistent_with(hash, value, eq)?;
                Ok(Self::from_generic_storage(generic))
            }
        }
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&value) {
            match &mut self.storage {
                PersistentSetStorage::Primitive(entries) => entries.insert(primitive),
                PersistentSetStorage::Generic(entries) => entries.insert_with(hash, value, eq)?,
            }
            return Ok(());
        }

        match &mut self.storage {
            PersistentSetStorage::Primitive(entries) => {
                let mut generic = entries.to_generic();
                generic.insert_with(hash, value, eq)?;
                self.storage = PersistentSetStorage::Generic(generic);
            }
            PersistentSetStorage::Generic(entries) => entries.insert_with(hash, value, eq)?,
        }
        Ok(())
    }

    pub fn remove_with<E>(
        &mut self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            match &mut self.storage {
                PersistentSetStorage::Primitive(entries) => entries.remove(&primitive),
                PersistentSetStorage::Generic(entries) => entries.remove_with(hash, value, eq)?,
            }
            return Ok(());
        }

        if let PersistentSetStorage::Generic(entries) = &mut self.storage {
            entries.remove_with(hash, value, eq)?;
        }
        Ok(())
    }

    pub fn contains(&self, key: &MapKey) -> bool {
        match &self.storage {
            PersistentSetStorage::Primitive(entries) => entries.contains(key),
            PersistentSetStorage::Generic(entries) => entries.contains(key),
        }
    }
}

fn primitive_map_key_hash(key: &MapKey) -> i64 {
    match key {
        MapKey::Int(n) => *n,
        MapKey::String(s) => {
            let mut hash = 0i64;
            for byte in s.as_bytes() {
                hash = hash.wrapping_mul(31).wrapping_add(i64::from(*byte));
            }
            hash
        }
        MapKey::Char(c) => i64::from(u32::from(*c)),
        MapKey::Bool(b) => i64::from(*b),
        MapKey::Unit => 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeSlot {
    Empty,
    Tombstone,
    Occupied(usize),
}

fn slot_count_for_min_live(min_live: usize) -> usize {
    if min_live == 0 {
        return 0;
    }
    let min_slots = min_live.saturating_mul(10).div_ceil(7);
    min_slots.max(8).next_power_of_two()
}

fn initial_slot_count_from_hint(capacity_hint: usize) -> usize {
    slot_count_for_min_live(capacity_hint)
}

fn probe_index(hash: i64, slot_count: usize) -> usize {
    debug_assert!(slot_count.is_power_of_two());
    (hash as u64 as usize) & (slot_count - 1)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrimitiveMapEntry {
    hash: i64,
    key: MapKey,
    value: Value,
    live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PrimitiveSetEntry {
    hash: i64,
    key: MapKey,
    live: bool,
}

#[derive(Debug, Clone, Default)]
struct PrimitiveMutableMapValue {
    entries: Vec<PrimitiveMapEntry>,
    slots: Vec<ProbeSlot>,
    live_len: usize,
    tombstones: usize,
}

impl PrimitiveMutableMapValue {
    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }

    fn with_capacity(capacity_hint: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity_hint),
            slots: vec![ProbeSlot::Empty; initial_slot_count_from_hint(capacity_hint)],
            live_len: 0,
            tombstones: 0,
        }
    }

    fn from_indexmap(entries: IndexMap<MapKey, Value>) -> Self {
        let mut out = Self::with_capacity(entries.len());
        for (key, value) in entries {
            out.insert(key, value);
        }
        out
    }

    fn len(&self) -> usize {
        self.live_len
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.live_len == 0
    }

    fn snapshot(&self) -> MapValue {
        let mut entries = IndexMap::with_capacity(self.live_len);
        for entry in &self.entries {
            if entry.live {
                entries.insert(entry.key.clone(), entry.value.clone());
            }
        }
        MapValue::from_primitive_indexmap(entries)
    }

    fn get_cloned(&self, key: &MapKey) -> Option<Value> {
        let (_, entry_idx) = self.lookup(key)?;
        Some(self.entries[entry_idx].value.clone())
    }

    fn contains(&self, key: &MapKey) -> bool {
        self.lookup(key).is_some()
    }

    fn insert(&mut self, key: MapKey, value: Value) {
        self.ensure_ready_for_insert();
        match self.find_slot(&key) {
            ProbeSlot::Occupied(entry_idx) => {
                self.entries[entry_idx].value = value;
            }
            ProbeSlot::Empty | ProbeSlot::Tombstone => {
                let slot_idx = self.find_slot_index_for_insert(&key);
                let entry_idx = self.entries.len();
                self.entries.push(PrimitiveMapEntry {
                    hash: key.primitive_hash(),
                    key,
                    value,
                    live: true,
                });
                if matches!(self.slots[slot_idx], ProbeSlot::Tombstone) {
                    self.tombstones = self.tombstones.saturating_sub(1);
                }
                self.slots[slot_idx] = ProbeSlot::Occupied(entry_idx);
                self.live_len += 1;
            }
        }
    }

    fn remove(&mut self, key: &MapKey) {
        let Some((slot_idx, entry_idx)) = self.lookup(key) else {
            return;
        };
        self.entries[entry_idx].live = false;
        self.slots[slot_idx] = ProbeSlot::Tombstone;
        self.live_len = self.live_len.saturating_sub(1);
        self.tombstones += 1;
        if !self.slots.is_empty() && self.tombstones > self.live_len {
            self.rehash(self.slots.len());
        }
    }

    fn lookup(&self, key: &MapKey) -> Option<(usize, usize)> {
        if self.slots.is_empty() {
            return None;
        }
        let hash = key.primitive_hash();
        let mut idx = probe_index(hash, self.slots.len());
        loop {
            match self.slots[idx] {
                ProbeSlot::Empty => return None,
                ProbeSlot::Tombstone => {}
                ProbeSlot::Occupied(entry_idx) => {
                    let entry = &self.entries[entry_idx];
                    if entry.live && entry.hash == hash && entry.key == *key {
                        return Some((idx, entry_idx));
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    fn find_slot(&self, key: &MapKey) -> ProbeSlot {
        self.lookup(key)
            .map(|(_, entry_idx)| ProbeSlot::Occupied(entry_idx))
            .unwrap_or_else(|| {
                if self.slots.is_empty() {
                    ProbeSlot::Empty
                } else {
                    ProbeSlot::Tombstone
                }
            })
    }

    fn find_slot_index_for_insert(&self, key: &MapKey) -> usize {
        let hash = key.primitive_hash();
        let mut idx = probe_index(hash, self.slots.len());
        let mut first_tombstone = None;
        loop {
            match self.slots[idx] {
                ProbeSlot::Empty => return first_tombstone.unwrap_or(idx),
                ProbeSlot::Tombstone => {
                    first_tombstone.get_or_insert(idx);
                }
                ProbeSlot::Occupied(entry_idx) => {
                    let entry = &self.entries[entry_idx];
                    if entry.live && entry.hash == hash && entry.key == *key {
                        return idx;
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    fn ensure_ready_for_insert(&mut self) {
        if self.slots.is_empty() {
            self.rehash(initial_slot_count_from_hint(self.entries.capacity().max(1)));
            return;
        }
        if self.tombstones > self.live_len {
            self.rehash(self.slots.len());
        }
        if (self.live_len + self.tombstones) * 10 >= self.slots.len() * 7 {
            let next_slots = (self.slots.len() * 2)
                .max(slot_count_for_min_live(self.live_len.saturating_add(1)));
            self.rehash(next_slots);
        }
    }

    fn rehash(&mut self, slot_count: usize) {
        let live_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.live)
            .cloned()
            .collect();
        let slot_count = slot_count
            .max(initial_slot_count_from_hint(live_entries.len()))
            .max(if live_entries.is_empty() { 0 } else { 8 });
        self.entries = live_entries;
        self.live_len = self.entries.len();
        self.tombstones = 0;
        self.slots = vec![ProbeSlot::Empty; slot_count];
        if self.slots.is_empty() {
            return;
        }
        for (entry_idx, entry) in self.entries.iter().enumerate() {
            let mut idx = probe_index(entry.hash, self.slots.len());
            loop {
                if matches!(self.slots[idx], ProbeSlot::Empty) {
                    self.slots[idx] = ProbeSlot::Occupied(entry_idx);
                    break;
                }
                idx = (idx + 1) & (self.slots.len() - 1);
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PrimitiveMutableSetValue {
    entries: Vec<PrimitiveSetEntry>,
    slots: Vec<ProbeSlot>,
    live_len: usize,
    tombstones: usize,
}

impl PrimitiveMutableSetValue {
    #[cfg(test)]
    fn new() -> Self {
        Self::default()
    }

    fn with_capacity(capacity_hint: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity_hint),
            slots: vec![ProbeSlot::Empty; initial_slot_count_from_hint(capacity_hint)],
            live_len: 0,
            tombstones: 0,
        }
    }

    fn from_indexset(entries: IndexSet<MapKey>) -> Self {
        let mut out = Self::with_capacity(entries.len());
        for key in entries {
            out.insert(key);
        }
        out
    }

    fn len(&self) -> usize {
        self.live_len
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.live_len == 0
    }

    fn snapshot(&self) -> SetValue {
        let mut entries = IndexSet::with_capacity(self.live_len);
        for entry in &self.entries {
            if entry.live {
                entries.insert(entry.key.clone());
            }
        }
        SetValue::from_primitive_indexset(entries)
    }

    fn contains(&self, key: &MapKey) -> bool {
        self.lookup(key).is_some()
    }

    fn insert(&mut self, key: MapKey) {
        self.ensure_ready_for_insert();
        if self.lookup(&key).is_some() {
            return;
        }
        let slot_idx = self.find_slot_index_for_insert(&key);
        let entry_idx = self.entries.len();
        self.entries.push(PrimitiveSetEntry {
            hash: key.primitive_hash(),
            key,
            live: true,
        });
        if matches!(self.slots[slot_idx], ProbeSlot::Tombstone) {
            self.tombstones = self.tombstones.saturating_sub(1);
        }
        self.slots[slot_idx] = ProbeSlot::Occupied(entry_idx);
        self.live_len += 1;
    }

    fn remove(&mut self, key: &MapKey) {
        let Some((slot_idx, entry_idx)) = self.lookup(key) else {
            return;
        };
        self.entries[entry_idx].live = false;
        self.slots[slot_idx] = ProbeSlot::Tombstone;
        self.live_len = self.live_len.saturating_sub(1);
        self.tombstones += 1;
        if !self.slots.is_empty() && self.tombstones > self.live_len {
            self.rehash(self.slots.len());
        }
    }

    fn lookup(&self, key: &MapKey) -> Option<(usize, usize)> {
        if self.slots.is_empty() {
            return None;
        }
        let hash = key.primitive_hash();
        let mut idx = probe_index(hash, self.slots.len());
        loop {
            match self.slots[idx] {
                ProbeSlot::Empty => return None,
                ProbeSlot::Tombstone => {}
                ProbeSlot::Occupied(entry_idx) => {
                    let entry = &self.entries[entry_idx];
                    if entry.live && entry.hash == hash && entry.key == *key {
                        return Some((idx, entry_idx));
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    fn find_slot_index_for_insert(&self, key: &MapKey) -> usize {
        let hash = key.primitive_hash();
        let mut idx = probe_index(hash, self.slots.len());
        let mut first_tombstone = None;
        loop {
            match self.slots[idx] {
                ProbeSlot::Empty => return first_tombstone.unwrap_or(idx),
                ProbeSlot::Tombstone => {
                    first_tombstone.get_or_insert(idx);
                }
                ProbeSlot::Occupied(entry_idx) => {
                    let entry = &self.entries[entry_idx];
                    if entry.live && entry.hash == hash && entry.key == *key {
                        return idx;
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    fn ensure_ready_for_insert(&mut self) {
        if self.slots.is_empty() {
            self.rehash(initial_slot_count_from_hint(self.entries.capacity().max(1)));
            return;
        }
        if self.tombstones > self.live_len {
            self.rehash(self.slots.len());
        }
        if (self.live_len + self.tombstones) * 10 >= self.slots.len() * 7 {
            let next_slots = (self.slots.len() * 2)
                .max(slot_count_for_min_live(self.live_len.saturating_add(1)));
            self.rehash(next_slots);
        }
    }

    fn rehash(&mut self, slot_count: usize) {
        let live_entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.live)
            .cloned()
            .collect();
        let slot_count = slot_count
            .max(initial_slot_count_from_hint(live_entries.len()))
            .max(if live_entries.is_empty() { 0 } else { 8 });
        self.entries = live_entries;
        self.live_len = self.entries.len();
        self.tombstones = 0;
        self.slots = vec![ProbeSlot::Empty; slot_count];
        if self.slots.is_empty() {
            return;
        }
        for (entry_idx, entry) in self.entries.iter().enumerate() {
            let mut idx = probe_index(entry.hash, self.slots.len());
            loop {
                if matches!(self.slots[idx], ProbeSlot::Empty) {
                    self.slots[idx] = ProbeSlot::Occupied(entry_idx);
                    break;
                }
                idx = (idx + 1) & (self.slots.len() - 1);
            }
        }
    }
}

#[derive(Debug, Clone)]
enum MutableMapStorage {
    Empty { capacity_hint: usize },
    Primitive(PrimitiveMutableMapValue),
    Generic(MapValue),
}

#[derive(Debug, Clone)]
pub struct MutableMapValue {
    storage: MutableMapStorage,
}

impl Default for MutableMapValue {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for MutableMapValue {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot() == other.snapshot()
    }
}

impl Eq for MutableMapValue {}

impl MutableMapValue {
    pub fn new() -> Self {
        Self {
            storage: MutableMapStorage::Empty { capacity_hint: 0 },
        }
    }

    pub fn with_capacity(capacity_hint: usize) -> Self {
        Self {
            storage: MutableMapStorage::Empty { capacity_hint },
        }
    }

    pub fn from_primitive_indexmap(entries: IndexMap<MapKey, Value>) -> Self {
        Self {
            storage: MutableMapStorage::Primitive(PrimitiveMutableMapValue::from_indexmap(entries)),
        }
    }

    pub fn from_map(entries: MapValue) -> Self {
        match &entries.storage {
            PersistentMapStorage::Primitive(primitive) => {
                Self::from_primitive_indexmap(primitive.entries.clone())
            }
            PersistentMapStorage::Generic(_) => Self {
                storage: MutableMapStorage::Generic(entries),
            },
        }
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            MutableMapStorage::Empty { .. } => 0,
            MutableMapStorage::Primitive(entries) => entries.len(),
            MutableMapStorage::Generic(entries) => entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn snapshot(&self) -> MapValue {
        match &self.storage {
            MutableMapStorage::Empty { .. } => MapValue::new(),
            MutableMapStorage::Primitive(entries) => entries.snapshot(),
            MutableMapStorage::Generic(entries) => entries.clone(),
        }
    }

    pub fn get_cloned_primitive(&self, key: &MapKey) -> Option<Value> {
        match &self.storage {
            MutableMapStorage::Empty { .. } => None,
            MutableMapStorage::Primitive(entries) => entries.get_cloned(key),
            MutableMapStorage::Generic(entries) => entries.get(key).cloned(),
        }
    }

    pub fn contains_primitive(&self, key: &MapKey) -> bool {
        match &self.storage {
            MutableMapStorage::Empty { .. } => false,
            MutableMapStorage::Primitive(entries) => entries.contains(key),
            MutableMapStorage::Generic(entries) => entries.get(key).is_some(),
        }
    }

    pub fn insert_primitive(&mut self, key: MapKey, value: Value) {
        match &mut self.storage {
            MutableMapStorage::Empty { capacity_hint } => {
                let mut entries = PrimitiveMutableMapValue::with_capacity(*capacity_hint);
                entries.insert(key, value);
                self.storage = MutableMapStorage::Primitive(entries);
            }
            MutableMapStorage::Primitive(entries) => entries.insert(key, value),
            MutableMapStorage::Generic(entries) => {
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                let _ = entries.insert_with(hash, key_value, value, &mut |lhs, rhs| Ok(lhs == rhs));
            }
        }
    }

    pub fn remove_primitive(&mut self, key: &MapKey) {
        match &mut self.storage {
            MutableMapStorage::Empty { .. } => {}
            MutableMapStorage::Primitive(entries) => entries.remove(key),
            MutableMapStorage::Generic(entries) => {
                let hash = key.primitive_hash();
                let key_value = key.to_value();
                let _ = entries.remove_with(hash, &key_value, &mut |lhs, rhs| Ok(lhs == rhs));
            }
        }
    }

    pub fn get_cloned_with<E>(
        &self,
        hash: i64,
        key: &Value,
        eq: &mut E,
    ) -> Result<Option<Value>, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(self.get_cloned_primitive(&primitive));
        }
        match &self.storage {
            MutableMapStorage::Empty { .. } => Ok(None),
            MutableMapStorage::Primitive(_) => Ok(None),
            MutableMapStorage::Generic(entries) => entries.get_cloned_with(hash, key, eq),
        }
    }

    pub fn contains_with<E>(&self, hash: i64, key: &Value, eq: &mut E) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            return Ok(self.contains_primitive(&primitive));
        }
        match &self.storage {
            MutableMapStorage::Empty { .. } => Ok(false),
            MutableMapStorage::Primitive(_) => Ok(false),
            MutableMapStorage::Generic(entries) => entries.contains_with(hash, key, eq),
        }
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        key: Value,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&key) {
            self.insert_primitive(primitive, value);
            return Ok(());
        }
        match &mut self.storage {
            MutableMapStorage::Empty { .. } => {
                let mut entries = MapValue::new();
                entries.insert_with(hash, key, value, eq)?;
                self.storage = MutableMapStorage::Generic(entries);
            }
            MutableMapStorage::Primitive(entries) => {
                let mut generic = entries.snapshot();
                generic.insert_with(hash, key, value, eq)?;
                self.storage = MutableMapStorage::Generic(generic);
            }
            MutableMapStorage::Generic(entries) => {
                entries.insert_with(hash, key, value, eq)?;
            }
        }
        Ok(())
    }

    pub fn remove_with<E>(&mut self, hash: i64, key: &Value, eq: &mut E) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(key) {
            self.remove_primitive(&primitive);
            return Ok(());
        }
        match &mut self.storage {
            MutableMapStorage::Empty { .. } => {}
            MutableMapStorage::Primitive(_) => {}
            MutableMapStorage::Generic(entries) => entries.remove_with(hash, key, eq)?,
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
enum MutableSetStorage {
    Empty { capacity_hint: usize },
    Primitive(PrimitiveMutableSetValue),
    Generic(SetValue),
}

#[derive(Debug, Clone)]
pub struct MutableSetValue {
    storage: MutableSetStorage,
}

impl Default for MutableSetValue {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for MutableSetValue {
    fn eq(&self, other: &Self) -> bool {
        self.snapshot() == other.snapshot()
    }
}

impl Eq for MutableSetValue {}

impl MutableSetValue {
    pub fn new() -> Self {
        Self {
            storage: MutableSetStorage::Empty { capacity_hint: 0 },
        }
    }

    pub fn with_capacity(capacity_hint: usize) -> Self {
        Self {
            storage: MutableSetStorage::Empty { capacity_hint },
        }
    }

    pub fn from_primitive_indexset(entries: IndexSet<MapKey>) -> Self {
        Self {
            storage: MutableSetStorage::Primitive(PrimitiveMutableSetValue::from_indexset(entries)),
        }
    }

    pub fn from_set(entries: SetValue) -> Self {
        match &entries.storage {
            PersistentSetStorage::Primitive(primitive) => {
                Self::from_primitive_indexset(primitive.entries.clone())
            }
            PersistentSetStorage::Generic(_) => Self {
                storage: MutableSetStorage::Generic(entries),
            },
        }
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            MutableSetStorage::Empty { .. } => 0,
            MutableSetStorage::Primitive(entries) => entries.len(),
            MutableSetStorage::Generic(entries) => entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn snapshot(&self) -> SetValue {
        match &self.storage {
            MutableSetStorage::Empty { .. } => SetValue::new(),
            MutableSetStorage::Primitive(entries) => entries.snapshot(),
            MutableSetStorage::Generic(entries) => entries.clone(),
        }
    }

    pub fn contains_primitive(&self, key: &MapKey) -> bool {
        match &self.storage {
            MutableSetStorage::Empty { .. } => false,
            MutableSetStorage::Primitive(entries) => entries.contains(key),
            MutableSetStorage::Generic(entries) => entries.contains(key),
        }
    }

    pub fn insert_primitive(&mut self, key: MapKey) {
        match &mut self.storage {
            MutableSetStorage::Empty { capacity_hint } => {
                let mut entries = PrimitiveMutableSetValue::with_capacity(*capacity_hint);
                entries.insert(key);
                self.storage = MutableSetStorage::Primitive(entries);
            }
            MutableSetStorage::Primitive(entries) => entries.insert(key),
            MutableSetStorage::Generic(entries) => {
                let hash = key.primitive_hash();
                let value = key.to_value();
                let _ = entries.insert_with(hash, value, &mut |lhs, rhs| Ok(lhs == rhs));
            }
        }
    }

    pub fn remove_primitive(&mut self, key: &MapKey) {
        match &mut self.storage {
            MutableSetStorage::Empty { .. } => {}
            MutableSetStorage::Primitive(entries) => entries.remove(key),
            MutableSetStorage::Generic(entries) => {
                let hash = key.primitive_hash();
                let value = key.to_value();
                let _ = entries.remove_with(hash, &value, &mut |lhs, rhs| Ok(lhs == rhs));
            }
        }
    }

    pub fn contains_with<E>(
        &self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<bool, RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            return Ok(self.contains_primitive(&primitive));
        }
        match &self.storage {
            MutableSetStorage::Empty { .. } => Ok(false),
            MutableSetStorage::Primitive(_) => Ok(false),
            MutableSetStorage::Generic(entries) => entries.contains_with(hash, value, eq),
        }
    }

    pub fn insert_with<E>(
        &mut self,
        hash: i64,
        value: Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(&value) {
            self.insert_primitive(primitive);
            return Ok(());
        }
        match &mut self.storage {
            MutableSetStorage::Empty { .. } => {
                let mut entries = SetValue::new();
                entries.insert_with(hash, value, eq)?;
                self.storage = MutableSetStorage::Generic(entries);
            }
            MutableSetStorage::Primitive(entries) => {
                let mut generic = entries.snapshot();
                generic.insert_with(hash, value, eq)?;
                self.storage = MutableSetStorage::Generic(generic);
            }
            MutableSetStorage::Generic(entries) => {
                entries.insert_with(hash, value, eq)?;
            }
        }
        Ok(())
    }

    pub fn remove_with<E>(
        &mut self,
        hash: i64,
        value: &Value,
        eq: &mut E,
    ) -> Result<(), RuntimeError>
    where
        E: FnMut(&Value, &Value) -> Result<bool, RuntimeError>,
    {
        if let Ok(primitive) = MapKey::from_value(value) {
            self.remove_primitive(&primitive);
            return Ok(());
        }
        match &mut self.storage {
            MutableSetStorage::Empty { .. } => {}
            MutableSetStorage::Primitive(_) => {}
            MutableSetStorage::Generic(entries) => entries.remove_with(hash, value, eq)?,
        }
        Ok(())
    }
}

/// Mutable list runtime storage.
///
/// Aliases share the outer `RefCell`, so mutation is visible across aliases.
/// Sequence pipelines snapshot the current inner `Rc<Vec<_>>` cheaply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct BoolListValue {
    len: usize,
    words: Vec<u64>,
}

impl BoolListValue {
    fn from_values(items: &[Value]) -> Option<Self> {
        let mut out = Self::default();
        for item in items {
            let Value::Bool(value) = item else {
                return None;
            };
            out.push(*value);
        }
        Some(out)
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn get(&self, idx: usize) -> Option<bool> {
        if idx >= self.len {
            return None;
        }
        let word = self.words[idx / 64];
        Some(((word >> (idx % 64)) & 1) != 0)
    }

    fn last(&self) -> Option<bool> {
        self.len.checked_sub(1).and_then(|idx| self.get(idx))
    }

    fn push(&mut self, value: bool) {
        let idx = self.len;
        if idx.is_multiple_of(64) {
            self.words.push(0);
        }
        if value {
            self.words[idx / 64] |= 1u64 << (idx % 64);
        }
        self.len += 1;
    }

    fn extend_bools<I>(&mut self, values: I)
    where
        I: IntoIterator<Item = bool>,
    {
        for value in values {
            self.push(value);
        }
    }

    fn set(&mut self, idx: usize, value: bool) {
        let mask = 1u64 << (idx % 64);
        let word = &mut self.words[idx / 64];
        if value {
            *word |= mask;
        } else {
            *word &= !mask;
        }
    }

    fn pop(&mut self) -> Option<bool> {
        let idx = self.len.checked_sub(1)?;
        let word_idx = idx / 64;
        let mask = 1u64 << (idx % 64);
        let value = (self.words[word_idx] & mask) != 0;
        self.words[word_idx] &= !mask;
        self.len -= 1;
        if self.len.is_multiple_of(64) {
            let _ = self.words.pop();
        }
        Some(value)
    }

    fn to_values(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.len);
        for idx in 0..self.len {
            out.push(Value::Bool(self.get(idx).expect(
                "bool list index should stay in bounds while materializing",
            )));
        }
        out
    }
}

#[derive(Debug, Clone)]
enum MutableListStorage {
    Generic(Rc<Vec<Value>>),
    Bool(BoolListValue),
}

impl MutableListStorage {
    fn from_items(items: Vec<Value>) -> Self {
        if let Some(bools) = BoolListValue::from_values(&items) {
            Self::Bool(bools)
        } else {
            Self::Generic(Rc::new(items))
        }
    }
}

#[derive(Debug, Clone)]
pub struct MutableListValue {
    items: Rc<RefCell<MutableListStorage>>,
}

impl MutableListValue {
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: Rc::new(RefCell::new(MutableListStorage::from_items(items))),
        }
    }

    pub fn snapshot(&self) -> Rc<Vec<Value>> {
        match &*self.items.borrow() {
            MutableListStorage::Generic(items) => items.clone(),
            MutableListStorage::Bool(items) => Rc::new(items.to_values()),
        }
    }

    pub fn len(&self) -> usize {
        match &*self.items.borrow() {
            MutableListStorage::Generic(items) => items.len(),
            MutableListStorage::Bool(items) => items.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &*self.items.borrow() {
            MutableListStorage::Generic(items) => items.is_empty(),
            MutableListStorage::Bool(items) => items.is_empty(),
        }
    }

    pub fn get_cloned(&self, idx: usize) -> Option<Value> {
        match &*self.items.borrow() {
            MutableListStorage::Generic(items) => items.get(idx).cloned(),
            MutableListStorage::Bool(items) => items.get(idx).map(Value::Bool),
        }
    }

    pub fn last_cloned(&self) -> Option<Value> {
        match &*self.items.borrow() {
            MutableListStorage::Generic(items) => items.last().cloned(),
            MutableListStorage::Bool(items) => items.last().map(Value::Bool),
        }
    }

    pub fn push(&self, value: Value) {
        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => Rc::make_mut(items).push(value),
            MutableListStorage::Bool(items) => match value {
                Value::Bool(value) => items.push(value),
                other => {
                    let mut generic = items.to_values();
                    generic.push(other);
                    *storage = MutableListStorage::Generic(Rc::new(generic));
                }
            },
        }
    }

    pub fn insert(&self, idx: usize, value: Value) {
        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => Rc::make_mut(items).insert(idx, value),
            MutableListStorage::Bool(items) => {
                let mut generic = items.to_values();
                generic.insert(idx, value);
                *storage = MutableListStorage::Generic(Rc::new(generic));
            }
        }
    }

    pub fn pop(&self) -> Option<Value> {
        let mut items = self.items.borrow_mut();
        match &mut *items {
            MutableListStorage::Generic(items) => Rc::make_mut(items).pop(),
            MutableListStorage::Bool(items) => items.pop().map(Value::Bool),
        }
    }

    pub fn extend<I>(&self, values: I)
    where
        I: IntoIterator<Item = Value>,
    {
        let values: Vec<_> = values.into_iter().collect();
        if values.is_empty() {
            return;
        }

        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => Rc::make_mut(items).extend(values),
            MutableListStorage::Bool(items) => {
                if values.iter().all(|value| matches!(value, Value::Bool(_))) {
                    items.extend_bools(values.into_iter().map(|value| match value {
                        Value::Bool(value) => value,
                        _ => unreachable!("checked all values are bools"),
                    }));
                } else {
                    let mut generic = items.to_values();
                    generic.extend(values);
                    *storage = MutableListStorage::Generic(Rc::new(generic));
                }
            }
        }
    }

    pub fn set(&self, idx: usize, value: Value) {
        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => Rc::make_mut(items)[idx] = value,
            MutableListStorage::Bool(items) => match value {
                Value::Bool(value) => items.set(idx, value),
                other => {
                    let mut generic = items.to_values();
                    generic[idx] = other;
                    *storage = MutableListStorage::Generic(Rc::new(generic));
                }
            },
        }
    }

    pub fn delete_at(&self, idx: usize) {
        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => {
                Rc::make_mut(items).remove(idx);
            }
            MutableListStorage::Bool(items) => {
                let mut generic = items.to_values();
                generic.remove(idx);
                *storage = MutableListStorage::Generic(Rc::new(generic));
            }
        }
    }

    pub fn remove_at(&self, idx: usize) -> Value {
        let mut storage = self.items.borrow_mut();
        match &mut *storage {
            MutableListStorage::Generic(items) => Rc::make_mut(items).remove(idx),
            MutableListStorage::Bool(items) => {
                let mut generic = items.to_values();
                let removed = generic.remove(idx);
                *storage = MutableListStorage::Generic(Rc::new(generic));
                removed
            }
        }
    }

    pub fn replace_all(&self, values: Vec<Value>) {
        *self.items.borrow_mut() = MutableListStorage::from_items(values);
    }

    pub fn reverse(&self) {
        let mut values = self.snapshot().as_ref().clone();
        values.reverse();
        self.replace_all(values);
    }

    pub fn shares_alias_storage_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }

    #[cfg(test)]
    pub fn current_backing_ptr(&self) -> *const Vec<Value> {
        let items = self.items.borrow();
        match &*items {
            MutableListStorage::Generic(items) => Rc::as_ptr(items),
            MutableListStorage::Bool(_) => {
                panic!("current_backing_ptr only applies to generic list storage")
            }
        }
    }

    #[cfg(test)]
    pub fn uses_bool_storage(&self) -> bool {
        matches!(&*self.items.borrow(), MutableListStorage::Bool(_))
    }
}

fn bitset_word_len(size_bits: usize) -> usize {
    if size_bits == 0 {
        0
    } else {
        size_bits.div_ceil(64)
    }
}

fn bitset_last_word_mask(size_bits: usize) -> u64 {
    let remainder = size_bits % 64;
    if remainder == 0 {
        u64::MAX
    } else {
        (1u64 << remainder) - 1
    }
}

fn bitset_index(size_bits: usize, idx: i64) -> Result<(usize, u64), RuntimeError> {
    let idx = usize::try_from(idx)
        .ok()
        .filter(|&idx| idx < size_bits)
        .ok_or_else(|| RuntimeError::TypeError("bitset index out of bounds".into()))?;
    Ok((idx / 64, 1u64 << (idx % 64)))
}

#[derive(Debug, Clone)]
pub struct BitSetValue {
    size_bits: usize,
    words: Rc<Vec<u64>>,
}

impl PartialEq for BitSetValue {
    fn eq(&self, other: &Self) -> bool {
        self.size_bits == other.size_bits && self.words == other.words
    }
}

impl Eq for BitSetValue {}

impl BitSetValue {
    pub fn new(size_bits: usize) -> Self {
        Self {
            size_bits,
            words: Rc::new(vec![0; bitset_word_len(size_bits)]),
        }
    }

    pub fn size_bits(&self) -> usize {
        self.size_bits
    }

    pub fn words(&self) -> Rc<Vec<u64>> {
        self.words.clone()
    }

    pub fn count(&self) -> usize {
        self.words
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|word| *word == 0)
    }

    pub fn test(&self, idx: i64) -> Result<bool, RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        Ok(self.words[word_idx] & mask != 0)
    }

    pub fn set(&self, idx: i64) -> Result<Self, RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        if self.words[word_idx] & mask != 0 {
            return Ok(self.clone());
        }
        let mut words = self.words.clone();
        Rc::make_mut(&mut words)[word_idx] |= mask;
        Ok(Self {
            size_bits: self.size_bits,
            words,
        })
    }

    pub fn reset(&self, idx: i64) -> Result<Self, RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        if self.words[word_idx] & mask == 0 {
            return Ok(self.clone());
        }
        let mut words = self.words.clone();
        Rc::make_mut(&mut words)[word_idx] &= !mask;
        Ok(Self {
            size_bits: self.size_bits,
            words,
        })
    }

    pub fn flip(&self, idx: i64) -> Result<Self, RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        let mut words = self.words.clone();
        Rc::make_mut(&mut words)[word_idx] ^= mask;
        Ok(Self {
            size_bits: self.size_bits,
            words,
        })
    }

    pub fn union(&self, other: &Self) -> Result<Self, RuntimeError> {
        self.binary_op(other, |lhs, rhs| lhs | rhs)
    }

    pub fn intersection(&self, other: &Self) -> Result<Self, RuntimeError> {
        self.binary_op(other, |lhs, rhs| lhs & rhs)
    }

    pub fn difference(&self, other: &Self) -> Result<Self, RuntimeError> {
        self.binary_op(other, |lhs, rhs| lhs & !rhs)
    }

    pub fn xor(&self, other: &Self) -> Result<Self, RuntimeError> {
        self.binary_op(other, |lhs, rhs| lhs ^ rhs)
    }

    fn binary_op<F>(&self, other: &Self, mut op: F) -> Result<Self, RuntimeError>
    where
        F: FnMut(u64, u64) -> u64,
    {
        if self.size_bits != other.size_bits {
            return Err(RuntimeError::TypeError("bitset size mismatch".into()));
        }

        let mut words = Vec::with_capacity(self.words.len());
        for (&lhs, &rhs) in self.words.iter().zip(other.words.iter()) {
            words.push(op(lhs, rhs));
        }
        if let Some(last) = words.last_mut() {
            *last &= bitset_last_word_mask(self.size_bits);
        }
        Ok(Self {
            size_bits: self.size_bits,
            words: Rc::new(words),
        })
    }

    pub fn shares_word_storage_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.words, &other.words)
    }

    #[cfg(test)]
    pub fn current_words_ptr(&self) -> *const Vec<u64> {
        Rc::as_ptr(&self.words)
    }
}

#[derive(Debug, Clone)]
pub struct MutableBitSetValue {
    size_bits: usize,
    words: Rc<RefCell<Rc<Vec<u64>>>>,
}

impl MutableBitSetValue {
    pub fn new(size_bits: usize) -> Self {
        Self {
            size_bits,
            words: Rc::new(RefCell::new(Rc::new(vec![0; bitset_word_len(size_bits)]))),
        }
    }

    pub fn from_bitset(bitset: &BitSetValue) -> Self {
        Self {
            size_bits: bitset.size_bits,
            words: Rc::new(RefCell::new(bitset.words.clone())),
        }
    }

    pub fn snapshot(&self) -> BitSetValue {
        BitSetValue {
            size_bits: self.size_bits,
            words: self.words.borrow().clone(),
        }
    }

    pub fn size_bits(&self) -> usize {
        self.size_bits
    }

    pub fn count(&self) -> usize {
        self.words
            .borrow()
            .iter()
            .map(|word| word.count_ones() as usize)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.borrow().iter().all(|word| *word == 0)
    }

    pub fn test(&self, idx: i64) -> Result<bool, RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        Ok(self.words.borrow()[word_idx] & mask != 0)
    }

    pub fn set(&self, idx: i64) -> Result<(), RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        let mut words = self.words.borrow_mut();
        let words_mut = Rc::make_mut(&mut *words);
        words_mut[word_idx] |= mask;
        Ok(())
    }

    pub fn reset(&self, idx: i64) -> Result<(), RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        let mut words = self.words.borrow_mut();
        let words_mut = Rc::make_mut(&mut *words);
        words_mut[word_idx] &= !mask;
        Ok(())
    }

    pub fn flip(&self, idx: i64) -> Result<(), RuntimeError> {
        let (word_idx, mask) = bitset_index(self.size_bits, idx)?;
        let mut words = self.words.borrow_mut();
        let words_mut = Rc::make_mut(&mut *words);
        words_mut[word_idx] ^= mask;
        Ok(())
    }

    pub fn union_assign(&self, other: &Self) -> Result<(), RuntimeError> {
        self.binary_assign(other, |lhs, rhs| lhs | rhs)
    }

    pub fn intersection_assign(&self, other: &Self) -> Result<(), RuntimeError> {
        self.binary_assign(other, |lhs, rhs| lhs & rhs)
    }

    pub fn difference_assign(&self, other: &Self) -> Result<(), RuntimeError> {
        self.binary_assign(other, |lhs, rhs| lhs & !rhs)
    }

    pub fn xor_assign(&self, other: &Self) -> Result<(), RuntimeError> {
        self.binary_assign(other, |lhs, rhs| lhs ^ rhs)
    }

    fn binary_assign<F>(&self, other: &Self, mut op: F) -> Result<(), RuntimeError>
    where
        F: FnMut(u64, u64) -> u64,
    {
        if self.size_bits != other.size_bits {
            return Err(RuntimeError::TypeError("bitset size mismatch".into()));
        }

        let other_words = other.words.borrow().clone();
        let mut words = self.words.borrow_mut();
        let words_mut = Rc::make_mut(&mut *words);
        for (lhs, &rhs) in words_mut.iter_mut().zip(other_words.iter()) {
            *lhs = op(*lhs, rhs);
        }
        if let Some(last) = words_mut.last_mut() {
            *last &= bitset_last_word_mask(self.size_bits);
        }
        Ok(())
    }

    pub fn shares_alias_storage_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.words, &other.words)
    }

    #[cfg(test)]
    pub fn current_words_ptr(&self) -> *const Vec<u64> {
        let words = self.words.borrow();
        Rc::as_ptr(&*words)
    }
}

#[derive(Debug, Clone)]
pub struct MutableDequeValue {
    items: Rc<RefCell<Rc<VecDeque<Value>>>>,
}

impl MutableDequeValue {
    pub fn new(items: VecDeque<Value>) -> Self {
        Self {
            items: Rc::new(RefCell::new(Rc::new(items))),
        }
    }

    pub fn snapshot(&self) -> Rc<VecDeque<Value>> {
        self.items.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }

    pub fn push_front(&self, value: Value) {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).push_front(value);
    }

    pub fn push_back(&self, value: Value) {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).push_back(value);
    }

    pub fn pop_front(&self) -> Option<Value> {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).pop_front()
    }

    pub fn pop_back(&self) -> Option<Value> {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).pop_back()
    }
}

/// A runtime value.
///
/// Kept small (32 bytes) by boxing heap-heavy variants behind indirection.
/// This improves cache locality for the common Int/Float/Bool/Unit cases.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Unit,
    Adt {
        type_idx: TypeItemIdx,
        variant: usize,
        fields: Vec<Value>,
    },
    Record {
        fields: Vec<(Name, Value)>,
        /// Optional type index for named record types (used for method resolution).
        type_idx: Option<TypeItemIdx>,
    },
    List(Rc<Vec<Value>>),
    BitSet(BitSetValue),
    MutableList(MutableListValue),
    MutablePriorityQueue(Rc<RefCell<MutablePriorityQueueValue>>),
    MutableMap(Rc<RefCell<MutableMapValue>>),
    MutableSet(Rc<RefCell<MutableSetValue>>),
    MutableBitSet(MutableBitSetValue),
    MutableDeque(MutableDequeValue),
    Deque(Rc<VecDeque<Value>>),
    Seq(Rc<SeqPlan>),
    Map(Rc<MapValue>),
    Set(Rc<SetValue>),
    Fn(Box<FnValue>),
}

/// Function values — user-defined, lambdas, or intrinsics.
#[derive(Debug, Clone)]
pub enum FnValue {
    User(FnItemIdx),
    Lambda {
        params: Vec<PatIdx>,
        body_expr: ExprIdx,
        body: Rc<Body>,
        env: Env,
    },
    Intrinsic(IntrinsicFn),
    /// ADT constructor with fields (used when a constructor is passed as a value).
    Constructor {
        type_idx: TypeItemIdx,
        variant_idx: usize,
        arity: usize,
    },
}

/// Source generator for a lazy sequence.
#[derive(Debug, Clone)]
pub enum SeqSource {
    Range { start: i64, end: i64 },
    FromList(Rc<Vec<Value>>),
    FromDeque(Rc<VecDeque<Value>>),
    StringSplit { s: Rc<String>, delim: Rc<String> },
    StringLines { s: Rc<String> },
    StringChars { s: Rc<String> },
    MapKeys(Rc<MapValue>),
    MapValues(Rc<MapValue>),
    SetValues(Rc<SetValue>),
    BitSetValues(BitSetValue),
}

/// Lazy, re-iterable sequence plan.
#[derive(Debug, Clone)]
pub enum SeqPlan {
    Source(SeqSource),
    Map {
        input: Rc<SeqPlan>,
        f: Value,
    },
    FlatMap {
        input: Rc<SeqPlan>,
        f: Value,
    },
    Filter {
        input: Rc<SeqPlan>,
        f: Value,
    },
    Scan {
        input: Rc<SeqPlan>,
        init: Value,
        f: Value,
    },
    Unfold {
        seed: Value,
        step: Value,
    },
    Enumerate {
        input: Rc<SeqPlan>,
    },
    Zip {
        left: Rc<SeqPlan>,
        right: Rc<SeqPlan>,
    },
    Chunks {
        input: Rc<SeqPlan>,
        n: i64,
    },
    Windows {
        input: Rc<SeqPlan>,
        n: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityQueueDirection {
    Min,
    Max,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityQueueEntry {
    pub priority: Value,
    pub value: Value,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutablePriorityQueueValue {
    pub direction: PriorityQueueDirection,
    pub next_seq: u64,
    pub entries: Vec<PriorityQueueEntry>,
}

impl MutablePriorityQueueValue {
    pub fn new(direction: PriorityQueueDirection) -> Self {
        Self {
            direction,
            next_seq: 0,
            entries: Vec::new(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Unit, Value::Unit) => true,
            (
                Value::Adt {
                    type_idx: t1,
                    variant: v1,
                    fields: f1,
                },
                Value::Adt {
                    type_idx: t2,
                    variant: v2,
                    fields: f2,
                },
            ) => t1 == t2 && v1 == v2 && f1 == f2,
            (Value::Record { fields: f1, .. }, Value::Record { fields: f2, .. }) => f1 == f2,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::BitSet(a), Value::BitSet(b)) => a == b,
            (Value::MutableList(a), Value::MutableList(b)) => a.snapshot() == b.snapshot(),
            (Value::MutablePriorityQueue(a), Value::MutablePriorityQueue(b)) => {
                *a.borrow() == *b.borrow()
            }
            (Value::MutableMap(a), Value::MutableMap(b)) => *a.borrow() == *b.borrow(),
            (Value::MutableSet(a), Value::MutableSet(b)) => *a.borrow() == *b.borrow(),
            (Value::MutableBitSet(a), Value::MutableBitSet(b)) => a.snapshot() == b.snapshot(),
            (Value::MutableDeque(a), Value::MutableDeque(b)) => a.snapshot() == b.snapshot(),
            (Value::Deque(a), Value::Deque(b)) => a == b,
            // Sequences are lazy plans; do not force-evaluate for equality.
            (Value::Seq(_), Value::Seq(_)) => false,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            // Functions are never equal.
            (Value::Fn(_), Value::Fn(_)) => false,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl Value {
    pub fn list(items: Vec<Value>) -> Self {
        Value::List(Rc::new(items))
    }

    pub fn mutable_list(items: Vec<Value>) -> Self {
        Value::MutableList(MutableListValue::new(items))
    }

    pub fn mutable_priority_queue(direction: PriorityQueueDirection) -> Self {
        Value::MutablePriorityQueue(Rc::new(RefCell::new(MutablePriorityQueueValue::new(
            direction,
        ))))
    }

    pub fn bitset(size_bits: usize) -> Self {
        Value::BitSet(BitSetValue::new(size_bits))
    }

    pub fn mutable_map(entries: IndexMap<MapKey, Value>) -> Self {
        Value::MutableMap(Rc::new(RefCell::new(
            MutableMapValue::from_primitive_indexmap(entries),
        )))
    }

    pub fn mutable_map_with_capacity(capacity: usize) -> Self {
        Value::MutableMap(Rc::new(RefCell::new(MutableMapValue::with_capacity(
            capacity,
        ))))
    }

    pub fn mutable_set(entries: IndexSet<MapKey>) -> Self {
        Value::MutableSet(Rc::new(RefCell::new(
            MutableSetValue::from_primitive_indexset(entries),
        )))
    }

    pub fn mutable_set_with_capacity(capacity: usize) -> Self {
        Value::MutableSet(Rc::new(RefCell::new(MutableSetValue::with_capacity(
            capacity,
        ))))
    }

    pub fn mutable_bitset(size_bits: usize) -> Self {
        Value::MutableBitSet(MutableBitSetValue::new(size_bits))
    }

    pub fn mutable_deque(items: VecDeque<Value>) -> Self {
        Value::MutableDeque(MutableDequeValue::new(items))
    }

    pub fn seq_source(source: SeqSource) -> Self {
        Value::Seq(Rc::new(SeqPlan::Source(source)))
    }

    pub fn deque(items: VecDeque<Value>) -> Self {
        Value::Deque(Rc::new(items))
    }

    pub fn seq_plan(plan: SeqPlan) -> Self {
        Value::Seq(Rc::new(plan))
    }

    pub fn map(entries: IndexMap<MapKey, Value>) -> Self {
        Value::Map(Rc::new(MapValue::from_primitive_indexmap(entries)))
    }

    pub fn set(entries: IndexSet<MapKey>) -> Self {
        Value::Set(Rc::new(SetValue::from_primitive_indexset(entries)))
    }

    pub fn display(&self, interner: &Interner) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Char(c) => c.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Unit => "()".to_string(),
            Value::Adt {
                variant, fields, ..
            } => {
                if fields.is_empty() {
                    format!("<variant {variant}>")
                } else {
                    let fs: Vec<String> = fields.iter().map(|f| f.display(interner)).collect();
                    format!("<variant {variant}>({})", fs.join(", "))
                }
            }
            Value::Record { fields, .. } => {
                let fs: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n.resolve(interner), v.display(interner)))
                    .collect();
                format!("{{ {} }}", fs.join(", "))
            }
            Value::List(items) => {
                let fs: Vec<String> = items.iter().map(|v| v.display(interner)).collect();
                format!("[{}]", fs.join(", "))
            }
            Value::BitSet(bitset) => {
                let fs: Vec<String> = bitset
                    .words()
                    .iter()
                    .enumerate()
                    .flat_map(|(word_idx, word)| {
                        let size_bits = bitset.size_bits();
                        let mut bits = Vec::new();
                        let mut remaining = *word;
                        while remaining != 0 {
                            let bit = remaining.trailing_zeros() as usize;
                            let idx = word_idx * 64 + bit;
                            if idx < size_bits {
                                bits.push(idx.to_string());
                            }
                            remaining &= remaining - 1;
                        }
                        bits
                    })
                    .collect();
                format!(
                    "BitSet(size={}, #{{{}}})",
                    bitset.size_bits(),
                    fs.join(", ")
                )
            }
            Value::MutableList(items) => {
                let snapshot = items.snapshot();
                let fs: Vec<String> = snapshot.iter().map(|v| v.display(interner)).collect();
                format!("MutableList([{}])", fs.join(", "))
            }
            Value::MutablePriorityQueue(queue) => {
                let queue = queue.borrow();
                let direction = match queue.direction {
                    PriorityQueueDirection::Min => "min",
                    PriorityQueueDirection::Max => "max",
                };
                format!(
                    "MutablePriorityQueue(direction={direction}, len={})",
                    queue.entries.len()
                )
            }
            Value::MutableMap(entries) => {
                let snapshot = entries.borrow().snapshot();
                let fs: Vec<String> = snapshot
                    .entries()
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}: {}",
                            entry.key.display(interner),
                            entry.value.display(interner)
                        )
                    })
                    .collect();
                format!("MutableMap({{{}}})", fs.join(", "))
            }
            Value::MutableSet(entries) => {
                let snapshot = entries.borrow().snapshot();
                let fs: Vec<String> = snapshot
                    .entries()
                    .iter()
                    .map(|entry| entry.value.display(interner))
                    .collect();
                format!("MutableSet(#{{{}}})", fs.join(", "))
            }
            Value::MutableBitSet(bitset) => {
                let snapshot = bitset.snapshot();
                Value::BitSet(snapshot)
                    .display(interner)
                    .replace("BitSet", "MutableBitSet")
            }
            Value::MutableDeque(items) => {
                let snapshot = items.snapshot();
                let fs: Vec<String> = snapshot.iter().map(|v| v.display(interner)).collect();
                format!("MutableDeque([{}])", fs.join(", "))
            }
            Value::Deque(items) => {
                let fs: Vec<String> = items.iter().map(|v| v.display(interner)).collect();
                format!("Deque([{}])", fs.join(", "))
            }
            Value::Seq(_) => "<seq>".to_string(),
            Value::Map(entries) => {
                let fs: Vec<String> = entries
                    .entries()
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}: {}",
                            entry.key.display(interner),
                            entry.value.display(interner)
                        )
                    })
                    .collect();
                format!("{{{}}}", fs.join(", "))
            }
            Value::Set(entries) => {
                let fs: Vec<String> = entries
                    .entries()
                    .iter()
                    .map(|entry| entry.value.display(interner))
                    .collect();
                format!("#{{{}}}", fs.join(", "))
            }
            Value::Fn(_) => "<function>".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_values(xs: &[i64]) -> Vec<Value> {
        xs.iter().map(|&n| Value::Int(n)).collect()
    }

    fn assert_mutable_list_matches_model(items: &MutableListValue, expected: &[i64]) {
        let expected_values = int_values(expected);
        assert_eq!(items.len(), expected.len());
        assert_eq!(items.is_empty(), expected.is_empty());
        assert_eq!(items.snapshot().as_ref(), expected_values.as_slice());
        for (idx, expected_value) in expected_values.iter().enumerate() {
            assert_eq!(items.get_cloned(idx), Some(expected_value.clone()));
        }
        assert_eq!(items.get_cloned(expected.len()), None);
        assert_eq!(items.last_cloned(), expected_values.last().cloned());
    }

    #[derive(Clone, Copy, Debug)]
    enum IndexedEditOp {
        Insert { idx: usize, value: i64 },
        DeleteAt { idx: usize },
        RemoveAt { idx: usize },
    }

    fn explore_mutable_list_indexed_edit_model(expected: Vec<i64>, remaining_depth: usize) {
        let items = MutableListValue::new(int_values(&expected));
        assert_mutable_list_matches_model(&items, &expected);

        if remaining_depth == 0 {
            return;
        }

        let mut ops = Vec::new();
        for idx in 0..=expected.len() {
            for value in [0, 1, 2] {
                ops.push(IndexedEditOp::Insert { idx, value });
            }
        }
        for idx in 0..expected.len() {
            ops.push(IndexedEditOp::DeleteAt { idx });
            ops.push(IndexedEditOp::RemoveAt { idx });
        }

        for op in ops {
            let branch_items = MutableListValue::new(int_values(&expected));
            let mut branch_expected = expected.clone();
            match op {
                IndexedEditOp::Insert { idx, value } => {
                    branch_items.insert(idx, Value::Int(value));
                    branch_expected.insert(idx, value);
                }
                IndexedEditOp::DeleteAt { idx } => {
                    branch_items.delete_at(idx);
                    branch_expected.remove(idx);
                }
                IndexedEditOp::RemoveAt { idx } => {
                    let removed = branch_items.remove_at(idx);
                    let expected_removed = branch_expected.remove(idx);
                    assert_eq!(removed, Value::Int(expected_removed));
                }
            }
            assert_mutable_list_matches_model(&branch_items, &branch_expected);
            explore_mutable_list_indexed_edit_model(branch_expected, remaining_depth - 1);
        }
    }

    #[test]
    fn list_clone_shares_storage_for_cow() {
        let original = Value::list(vec![Value::Int(1), Value::Int(2)]);
        let cloned = original.clone();
        let (Value::List(a), Value::List(b)) = (&original, &cloned) else {
            panic!("expected list values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "list clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn bitset_clone_shares_storage_for_cow() {
        let original = Value::bitset(16);
        let cloned = original.clone();
        let (Value::BitSet(a), Value::BitSet(b)) = (&original, &cloned) else {
            panic!("expected bitset values");
        };
        assert!(
            a.shares_word_storage_with(b),
            "bitset clone should share packed word storage before mutation in COW model"
        );
    }

    #[test]
    fn mutable_list_clone_shares_storage() {
        let original = Value::mutable_list(vec![Value::Int(1), Value::Int(2)]);
        let cloned = original.clone();
        let (Value::MutableList(a), Value::MutableList(b)) = (&original, &cloned) else {
            panic!("expected mutable list values");
        };
        assert!(
            a.shares_alias_storage_with(b),
            "mutable list clone should share storage for alias-visible mutation"
        );
    }

    #[test]
    fn mutable_bool_list_uses_specialized_storage_until_promotion() {
        let value = Value::mutable_list(vec![Value::Bool(false), Value::Bool(true)]);
        let Value::MutableList(items) = &value else {
            panic!("expected mutable list value");
        };

        assert!(
            items.uses_bool_storage(),
            "homogeneous bool mutable list should use specialized storage"
        );
        assert_eq!(items.get_cloned(0), Some(Value::Bool(false)));
        assert_eq!(items.get_cloned(1), Some(Value::Bool(true)));

        items.push(Value::Bool(false));
        items.set(1, Value::Bool(false));
        assert!(
            items.uses_bool_storage(),
            "bool-only mutations should stay on specialized storage"
        );
        assert_eq!(
            items.snapshot().as_ref(),
            &vec![Value::Bool(false), Value::Bool(false), Value::Bool(false)]
        );

        items.push(Value::Int(1));
        assert!(
            !items.uses_bool_storage(),
            "first non-bool element should promote to generic storage"
        );
        assert_eq!(
            items.snapshot().as_ref(),
            &vec![
                Value::Bool(false),
                Value::Bool(false),
                Value::Bool(false),
                Value::Int(1)
            ]
        );
    }

    #[test]
    fn mutable_bitset_clone_shares_storage() {
        let original = Value::mutable_bitset(16);
        let cloned = original.clone();
        let (Value::MutableBitSet(a), Value::MutableBitSet(b)) = (&original, &cloned) else {
            panic!("expected mutable bitset values");
        };
        assert!(
            a.shares_alias_storage_with(b),
            "mutable bitset clone should share storage for alias-visible mutation"
        );
    }

    #[test]
    fn mutable_list_snapshot_preserves_old_backing_after_mutation() {
        let value = Value::mutable_list(vec![Value::Int(1), Value::Int(2)]);
        let Value::MutableList(items) = &value else {
            panic!("expected mutable list value");
        };

        let snapshot = items.snapshot();
        let snapshot_ptr = Rc::as_ptr(&snapshot);

        items.set(0, Value::Int(99));

        assert_eq!(snapshot.as_ref(), &vec![Value::Int(1), Value::Int(2)]);
        assert_ne!(
            snapshot_ptr,
            items.current_backing_ptr(),
            "mutation should move aliases to a new current backing while preserving the old snapshot"
        );
    }

    #[test]
    fn mutable_list_snapshot_preserves_old_backing_after_remove_at() {
        let value = Value::mutable_list(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let Value::MutableList(items) = &value else {
            panic!("expected mutable list value");
        };

        let snapshot = items.snapshot();
        let snapshot_ptr = Rc::as_ptr(&snapshot);

        let removed = items.remove_at(1);

        assert_eq!(removed, Value::Int(2));
        assert_eq!(
            snapshot.as_ref(),
            &vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert_eq!(
            items.snapshot().as_ref(),
            &vec![Value::Int(1), Value::Int(3)]
        );
        assert_ne!(
            snapshot_ptr,
            items.current_backing_ptr(),
            "remove_at should move aliases to a new current backing while preserving the old snapshot"
        );
    }

    #[test]
    fn mutable_list_indexed_edits_match_vec_model_for_short_sequences() {
        for initial in [vec![], vec![0], vec![0, 1]] {
            explore_mutable_list_indexed_edit_model(initial, 4);
        }
    }

    #[test]
    fn mutable_bitset_snapshot_preserves_old_backing_after_mutation() {
        let value = Value::mutable_bitset(16);
        let Value::MutableBitSet(items) = &value else {
            panic!("expected mutable bitset value");
        };

        items.set(3).expect("initial set should succeed");
        let snapshot = items.snapshot();
        let snapshot_ptr = snapshot.current_words_ptr();

        items.flip(5).expect("flip should succeed");

        assert!(
            snapshot.test(3).expect("snapshot read should succeed"),
            "snapshot should preserve pre-mutation bits"
        );
        assert!(
            !snapshot.test(5).expect("snapshot read should succeed"),
            "snapshot should not observe later mutation"
        );
        assert_ne!(
            snapshot_ptr,
            items.current_words_ptr(),
            "mutation should move aliases to a new current backing while preserving the old snapshot"
        );
    }

    #[test]
    fn mutable_map_clone_shares_storage() {
        let mut entries = IndexMap::new();
        entries.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::mutable_map(entries);
        let cloned = original.clone();
        let (Value::MutableMap(a), Value::MutableMap(b)) = (&original, &cloned) else {
            panic!("expected mutable map values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "mutable map clone should share storage for alias-visible mutation"
        );
    }

    #[test]
    fn mutable_set_clone_shares_storage() {
        let mut entries = IndexSet::new();
        entries.insert(MapKey::Int(1));
        let original = Value::mutable_set(entries);
        let cloned = original.clone();
        let (Value::MutableSet(a), Value::MutableSet(b)) = (&original, &cloned) else {
            panic!("expected mutable set values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "mutable set clone should share storage for alias-visible mutation"
        );
    }

    #[test]
    fn map_clone_shares_storage_for_cow() {
        let mut m = IndexMap::new();
        m.insert(MapKey::Int(1), Value::Int(10));
        let original = Value::map(m);
        let cloned = original.clone();
        let (Value::Map(a), Value::Map(b)) = (&original, &cloned) else {
            panic!("expected map values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "map clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn deque_clone_shares_storage_for_cow() {
        let mut q = VecDeque::new();
        q.push_back(Value::Int(1));
        q.push_back(Value::Int(2));
        let original = Value::deque(q);
        let cloned = original.clone();
        let (Value::Deque(a), Value::Deque(b)) = (&original, &cloned) else {
            panic!("expected deque values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "deque clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn set_clone_shares_storage_for_cow() {
        let mut s = IndexSet::new();
        s.insert(MapKey::Int(1));
        let original = Value::set(s);
        let cloned = original.clone();
        let (Value::Set(a), Value::Set(b)) = (&original, &cloned) else {
            panic!("expected set values");
        };
        assert!(
            Rc::ptr_eq(a, b),
            "set clone should share storage before mutation in COW model"
        );
    }

    #[test]
    fn mutable_map_bucket_indices_remain_valid_after_middle_removal() {
        let mut map = MapValue::new();
        let mut eq = |left: &Value, right: &Value| Ok(left == right);

        map.insert_with(0, Value::Int(1), Value::Int(10), &mut eq)
            .expect("first insert should succeed");
        map.insert_with(0, Value::Int(2), Value::Int(20), &mut eq)
            .expect("second insert should succeed");
        map.insert_with(0, Value::Int(3), Value::Int(30), &mut eq)
            .expect("third insert should succeed");

        map.remove_with(0, &Value::Int(2), &mut eq)
            .expect("removal should succeed");

        assert_eq!(map.len(), 2);
        assert_eq!(
            map.find_index_with(0, &Value::Int(3), &mut eq)
                .expect("lookup should succeed"),
            Some(1)
        );
        assert_eq!(
            map.get_cloned_with(0, &Value::Int(3), &mut eq)
                .expect("lookup should succeed"),
            Some(Value::Int(30))
        );
        assert!(
            !map.contains_with(0, &Value::Int(2), &mut eq)
                .expect("lookup should succeed")
        );
    }

    #[test]
    fn mutable_set_bucket_indices_remain_valid_after_middle_removal() {
        let mut set = SetValue::new();
        let mut eq = |left: &Value, right: &Value| Ok(left == right);

        set.insert_with(0, Value::Int(1), &mut eq)
            .expect("first insert should succeed");
        set.insert_with(0, Value::Int(2), &mut eq)
            .expect("second insert should succeed");
        set.insert_with(0, Value::Int(3), &mut eq)
            .expect("third insert should succeed");

        set.remove_with(0, &Value::Int(2), &mut eq)
            .expect("removal should succeed");

        assert_eq!(set.len(), 2);
        assert_eq!(
            set.find_index_with(0, &Value::Int(3), &mut eq)
                .expect("lookup should succeed"),
            Some(1)
        );
        assert!(
            set.contains_with(0, &Value::Int(3), &mut eq)
                .expect("lookup should succeed")
        );
        assert!(
            !set.contains_with(0, &Value::Int(2), &mut eq)
                .expect("lookup should succeed")
        );
    }

    #[test]
    fn primitive_mutable_map_remove_reinsert_appends_to_end() {
        let mut map = PrimitiveMutableMapValue::new();
        assert!(map.is_empty(), "new primitive map should start empty");

        map.insert(MapKey::String("b".into()), Value::Int(1));
        map.insert(MapKey::String("a".into()), Value::Int(2));
        map.remove(&MapKey::String("b".into()));
        map.insert(MapKey::String("b".into()), Value::Int(3));

        let snapshot = map.snapshot();
        let keys: Vec<_> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.key.clone())
            .collect();
        assert_eq!(
            keys,
            vec![Value::String("a".into()), Value::String("b".into())]
        );
        assert_eq!(
            snapshot.get(&MapKey::String("b".into())),
            Some(&Value::Int(3))
        );
    }

    #[test]
    fn primitive_mutable_set_remove_reinsert_appends_to_end() {
        let mut set = PrimitiveMutableSetValue::new();
        assert!(set.is_empty(), "new primitive set should start empty");

        set.insert(MapKey::String("b".into()));
        set.insert(MapKey::String("a".into()));
        set.remove(&MapKey::String("b".into()));
        set.insert(MapKey::String("b".into()));

        let snapshot = set.snapshot();
        let values: Vec<_> = snapshot
            .entries()
            .iter()
            .map(|entry| entry.value.clone())
            .collect();
        assert_eq!(
            values,
            vec![Value::String("a".into()), Value::String("b".into())]
        );
        assert!(snapshot.contains(&MapKey::String("b".into())));
    }

    #[test]
    fn primitive_persistent_map_remove_reinsert_appends_to_end() {
        let mut map = PrimitivePersistentMapValue::new();
        assert!(
            map.is_empty(),
            "new primitive persistent map should start empty"
        );

        map.insert(MapKey::String("b".into()), Value::Int(1));
        map.insert(MapKey::String("a".into()), Value::Int(2));
        map.remove(&MapKey::String("b".into()));
        map.insert(MapKey::String("b".into()), Value::Int(3));

        let snapshot = map.snapshot();
        let keys: Vec<_> = snapshot
            .snapshot_entries()
            .into_iter()
            .map(|entry| entry.key)
            .collect();
        assert_eq!(
            keys,
            vec![Value::String("a".into()), Value::String("b".into())]
        );
        assert_eq!(
            snapshot.get(&MapKey::String("b".into())),
            Some(&Value::Int(3))
        );
    }

    #[test]
    fn primitive_persistent_set_remove_reinsert_appends_to_end() {
        let mut set = PrimitivePersistentSetValue::new();
        assert!(
            set.is_empty(),
            "new primitive persistent set should start empty"
        );

        set.insert(MapKey::String("b".into()));
        set.insert(MapKey::String("a".into()));
        set.remove(&MapKey::String("b".into()));
        set.insert(MapKey::String("b".into()));

        let snapshot = set.snapshot();
        let values: Vec<_> = snapshot
            .snapshot_entries()
            .into_iter()
            .map(|entry| entry.value)
            .collect();
        assert_eq!(
            values,
            vec![Value::String("a".into()), Value::String("b".into())]
        );
        assert!(snapshot.contains(&MapKey::String("b".into())));
    }
}
