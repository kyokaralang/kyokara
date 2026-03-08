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
pub struct MapValue {
    entries: Vec<MapEntry>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PartialEq for MapValue {
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

impl Eq for MapValue {}

impl MapValue {
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

    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_primitive_indexmap(entries: IndexMap<MapKey, Value>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(key, value)| MapEntry {
                hash: primitive_map_key_hash(&key),
                key: key.to_value(),
                value,
            })
            .collect();
        Self::from_entries(entries)
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

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn key_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.key.clone())
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.value.clone())
    }

    pub fn entries(&self) -> &[MapEntry] {
        &self.entries
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetEntry {
    pub hash: i64,
    pub value: Value,
}

#[derive(Debug, Clone, Default)]
pub struct SetValue {
    entries: Vec<SetEntry>,
    buckets: HashMap<i64, Vec<usize>>,
}

impl PartialEq for SetValue {
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

impl Eq for SetValue {}

impl SetValue {
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

    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_primitive_indexset(entries: IndexSet<MapKey>) -> Self {
        let entries = entries
            .into_iter()
            .map(|value| SetEntry {
                hash: primitive_map_key_hash(&value),
                value: value.to_value(),
            })
            .collect();
        Self::from_entries(entries)
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

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn value_at(&self, idx: usize) -> Option<Value> {
        self.entries.get(idx).map(|entry| entry.value.clone())
    }

    pub fn entries(&self) -> &[SetEntry] {
        &self.entries
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

/// Mutable list runtime storage.
///
/// Aliases share the outer `RefCell`, so mutation is visible across aliases.
/// Sequence pipelines snapshot the current inner `Rc<Vec<_>>` cheaply.
#[derive(Debug, Clone)]
pub struct MutableListValue {
    items: Rc<RefCell<Rc<Vec<Value>>>>,
}

impl MutableListValue {
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: Rc::new(RefCell::new(Rc::new(items))),
        }
    }

    pub fn snapshot(&self) -> Rc<Vec<Value>> {
        self.items.borrow().clone()
    }

    pub fn len(&self) -> usize {
        self.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }

    pub fn get_cloned(&self, idx: usize) -> Option<Value> {
        self.items.borrow().get(idx).cloned()
    }

    pub fn last_cloned(&self) -> Option<Value> {
        self.items.borrow().last().cloned()
    }

    pub fn push(&self, value: Value) {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).push(value);
    }

    pub fn pop(&self) -> Option<Value> {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).pop()
    }

    pub fn extend<I>(&self, values: I)
    where
        I: IntoIterator<Item = Value>,
    {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items).extend(values);
    }

    pub fn set(&self, idx: usize, value: Value) {
        let mut items = self.items.borrow_mut();
        Rc::make_mut(&mut *items)[idx] = value;
    }

    pub fn shares_alias_storage_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }

    #[cfg(test)]
    pub fn current_backing_ptr(&self) -> *const Vec<Value> {
        let items = self.items.borrow();
        Rc::as_ptr(&*items)
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
    MutableMap(Rc<RefCell<MapValue>>),
    MutableSet(Rc<RefCell<SetValue>>),
    MutableBitSet(MutableBitSetValue),
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
            (Value::MutableList(a), Value::MutableList(b)) => {
                let a_items = a.items.borrow();
                let b_items = b.items.borrow();
                *a_items == *b_items
            }
            (Value::MutablePriorityQueue(a), Value::MutablePriorityQueue(b)) => {
                *a.borrow() == *b.borrow()
            }
            (Value::MutableMap(a), Value::MutableMap(b)) => *a.borrow() == *b.borrow(),
            (Value::MutableSet(a), Value::MutableSet(b)) => *a.borrow() == *b.borrow(),
            (Value::MutableBitSet(a), Value::MutableBitSet(b)) => a.snapshot() == b.snapshot(),
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
        Value::MutableMap(Rc::new(RefCell::new(MapValue::from_primitive_indexmap(
            entries,
        ))))
    }

    pub fn mutable_set(entries: IndexSet<MapKey>) -> Self {
        Value::MutableSet(Rc::new(RefCell::new(SetValue::from_primitive_indexset(
            entries,
        ))))
    }

    pub fn mutable_bitset(size_bits: usize) -> Self {
        Value::MutableBitSet(MutableBitSetValue::new(size_bits))
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
                let fs: Vec<String> = entries
                    .borrow()
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
                let fs: Vec<String> = entries
                    .borrow()
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
}
