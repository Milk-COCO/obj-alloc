

use std::ops::{Deref, DerefMut};
use field_collex::{Collexetable, ConstUnit, Collex, FieldValue};
use field_collex::collex::{CollexCursor, ModifyError, iter::{Iter as CollexIter, IntoIter as CollexIntoIter}};
use crate::{Id, IdMap, Pair};

#[derive(Debug, Clone)]
#[derive(serde::Serialize)]
#[serde(transparent)]
pub struct OrdIdMap<K,O,T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    #[serde(skip)]
    pub id_map: IdMap<K,T>,
    pub collex: Collex<Pair<K,O>,T>
}

impl<K,E,V> Deref for OrdIdMap<K,E,V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    type Target = Collex<Pair<K,E>,V>;
    fn deref(&self) -> &Self::Target {
        &self.collex
    }
}

impl<K,E,V> DerefMut for OrdIdMap<K,E,V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.collex
    }
}

// ==================== 构造方法（需要 ConstUnit） ====================

impl<K, E, V> Default for OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue + ConstUnit,
 {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, E, V> OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue + ConstUnit,
{
    pub fn new() -> Self {
        Self {
            id_map: IdMap::with_id(),
            collex: Collex::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id_map: IdMap::with_id_capacity(capacity),
            collex: Collex::new(),
        }
    }

    pub fn with_elements(vec: Vec<E>) -> Self {
        let mut id_map = IdMap::with_id();
        let mut collex = Collex::new();
        for elem in vec {
            let v = elem.collexate();
            let id = id_map.insert(v);
            collex.insert(Pair(id, elem)).ok();
        }
        Self { id_map, collex }
    }
}

// ==================== 增删改查 ====================

impl<K, E, V> OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    pub fn insert(&mut self, elem: E) -> Result<K, E> {
        let v = elem.collexate();
        let id = self.id_map.insert(v);
        match self.collex.insert(Pair(id, elem)) {
            Ok(()) => Ok(id),
            Err(pair) => {
                self.id_map.remove(id);
                Err(pair.1)
            }
        }
    }

    pub fn insert_with_id(&mut self, id: K, elem: E) -> Result<Option<E>, E> {
        let v = elem.collexate();
        let old_v = self.id_map.insert_with_id(id, v);

        // 如果替换了旧值，从 collex 中移除旧元素
        let old_elem = if let Some(ref old_v) = old_v {
            self.collex.remove(old_v).ok().map(|pair| pair.1)
        } else {
            None
        };

        match self.collex.insert(Pair(id, elem)) {
            Ok(()) => Ok(old_elem),
            Err(pair) => {
                // 回滚：恢复旧元素
                if let Some(old_elem) = old_elem {
                    let old_v = old_elem.collexate();
                    self.id_map.insert_with_id(id, old_v);
                    self.collex.insert(Pair(id, old_elem)).ok();
                } else {
                    self.id_map.remove(id);
                }
                Err(pair.1)
            }
        }
    }

    pub fn remove(&mut self, id: K) -> Option<E> {
        let v = self.id_map.remove(id)?;
        self.collex.remove(&v).ok().map(|pair| pair.1)
    }

    pub fn get_with_id(&self, id: K) -> Option<&E> {
        let v = self.id_map.get(id)?;
        let pair = self.collex.find_ge(v)?;
        if pair.collexate_ref() == v {
            Some(&pair.1)
        } else {
            None
        }
    }

    pub fn extend(&mut self, vec: Vec<E>) {
        for elem in vec {
            let v = elem.collexate();
            let id = self.id_map.insert(v);
            self.collex.insert(Pair(id, elem)).ok();
        }
    }

    /// 创建用于单调递增时间线查找的游标。
    /// beat 非递减的前提下，每次 `step()` 只需 O(1) 推进。
    pub fn cursor(&self) -> CollexCursor<Pair<K, E>, V> {
        CollexCursor::new(&self.collex)
    }

    /// 从已保存的位置恢复游标（用于跨帧缓存）
    pub fn cursor_from(&self, pos: Option<(usize, usize)>) -> CollexCursor<Pair<K, E>, V> {
        CollexCursor::from_pos(&self.collex, pos)
    }

    pub fn try_extend(&mut self, vec: Vec<E>) -> (Vec<K>, Vec<E>) {
        let mut accepted = Vec::with_capacity(vec.len());
        let mut rejected = Vec::new();
        for elem in vec {
            let v = elem.collexate();
            let id = self.id_map.insert(v);
            match self.collex.insert(Pair(id, elem)) {
                Ok(()) => accepted.push(id),
                Err(pair) => {
                    self.id_map.remove(id);
                    rejected.push(pair.1);
                }
            }
        }
        (accepted, rejected)
    }

    pub fn into_raw_parts(self) -> (IdMap<K,V>, Collex<Pair<K,E>,V>) {
        (self.id_map, self.collex)
    }

    pub fn from_raw_parts(id_map: IdMap<K,V>, collex: Collex<Pair<K,E>,V>) -> Self {
        Self { id_map, collex }
    }

    pub fn modify<F, R>(&mut self, id: K, f: F) -> Result<R, ModifyError<R, E>>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(ModifyError::NotFound)?;

        match self.collex.modify(&v, |pair| {
            let r = f(&mut pair.1);
            (r, pair.collexate())
        }) {
            Ok((r, new_v)) => {
                if new_v != v {
                    self.id_map.remove(id);
                    self.id_map.insert_with_id(id, new_v);
                }
                Ok(r)
            }
            Err(ModifyError::NotFound) => Err(ModifyError::NotFound),
            Err(ModifyError::InsertError((r, _), pair)) => Err(ModifyError::InsertError(r, pair.1)),
        }
    }

    /// 按值修改元素（不同于按 ID 修改，当不知道 ID 时使用）
    pub fn modify_by_val<F, R>(&mut self, value: &V, f: F) -> Result<R, ModifyError<R, E>>
    where
        F: FnOnce(&mut E) -> R,
        V: Copy + PartialEq,
    {
        match self.collex.modify(value, |pair| {
            let r = f(&mut pair.1);
            let new_v = *pair.collexate_ref();
            (r, new_v)
        }) {
            Ok((r, _new_v)) => Ok(r),
            Err(ModifyError::NotFound) => Err(ModifyError::NotFound),
            Err(ModifyError::InsertError((r, _), pair)) => Err(ModifyError::InsertError(r, pair.1)),
        }
    }

    pub fn try_modify<F, R>(&mut self, id: K, f: F) -> Result<R, ()>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(())?;

        match self.collex.try_modify(&v, |pair| {
            let r = f(&mut pair.1);
            (r, pair.collexate())
        }) {
            Ok((r, new_v)) => {
                if new_v != v {
                    self.id_map.remove(id);
                    self.id_map.insert_with_id(id, new_v);
                }
                Ok(r)
            }
            Err(()) => Err(()),
        }
    }
}

pub struct Iter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    inner: CollexIter<'a, Pair<K, O>, T>,
}

impl<'a, K, O, T> Iterator for Iter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    type Item = (K, &'a O);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|pair| (pair.0, &pair.1))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

pub struct IntoIter<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    inner: CollexIntoIter<Pair<K, O>, T>,
}

impl<K, O, T> Iterator for IntoIter<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    type Item = (K, O);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|pair| (pair.0, pair.1))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K, O, T> OrdIdMap<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    pub fn iter(&self) -> Iter<'_, K, O, T> {
        Iter {
            inner: self.collex.iter(),
        }
    }
}

impl<'a, K, O, T> IntoIterator for &'a OrdIdMap<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    type Item = (K, &'a O);
    type IntoIter = Iter<'a, K, O, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<K, O, T> IntoIterator for OrdIdMap<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    type Item = (K, O);
    type IntoIter = IntoIter<K, O, T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.collex.into_iter(),
        }
    }
}