//! `OrdIdMap`：id ↔ 排序键 的派生索引 + 一个按排序键升序的 [`JumpArray`]。
//!
//! 有序存储直接就是 `JumpArray<T, Pair<K, E>>`：没有适配层，也没有多后端开关。
//! 排序键 `T` 需要同时满足 `field_collex::FieldValue`（槽位/数值能力）与
//! `jump_array::Key`（全序比较 + 块号计算），二者对同一类型的顺序必须一致。

use field_collex::collex::ModifyError;
use field_collex::{Collexetable, ConstUnit, FieldValue};
use jump_array::JumpArray;

use crate::{Id, IdMap, Pair};

/// `Option<&Pair<K,E>>` 的简写（`find_*` 系列的返回）。
pub type PairRef<'a, K, E> = Option<&'a Pair<K, E>>;

#[derive(Debug, Clone)]
pub struct OrdIdMap<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    pub id_map: IdMap<K, T>,
    /// 按排序键升序存放 `Pair(id, 元素)`。
    pub store: JumpArray<T, Pair<K, O>>,
}

/// 相等 = **按排序键顺序，元素 id 与载荷都相同**。`id_map` 与 `max_id` 都**不参与**。
impl<K, E, V> PartialEq for OrdIdMap<K, E, V>
where
    K: Id + PartialEq,
    E: Collexetable<V> + PartialEq,
    V: FieldValue + jump_array::Key,
{
    fn eq(&self, other: &Self) -> bool {
        let mut a = self.store.iter().map(|(_, p)| p);
        let mut b = other.store.iter().map(|(_, p)| p);
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some(x), Some(y)) => {
                    if x != y {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
}

impl<K, E, V> Default for OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue + jump_array::Key + ConstUnit,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, E, V> OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue + jump_array::Key + ConstUnit,
{
    pub fn new() -> Self {
        Self {
            id_map: IdMap::with_id(),
            store: JumpArray::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id_map: IdMap::with_id_capacity(capacity),
            store: JumpArray::with_capacity(capacity),
        }
    }

    pub fn with_elements(vec: Vec<E>) -> Self {
        let mut id_map = IdMap::with_id();
        let mut store = JumpArray::with_capacity(vec.len());
        for elem in vec {
            let v = elem.collexate();
            let id = id_map.insert(v);
            store.insert(v, Pair(id, elem)).ok();
        }
        store.rebuild();
        Self { id_map, store }
    }
}

impl<K, E, V> OrdIdMap<K, E, V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue + jump_array::Key,
{
    /// 排序键必须非负（与旧实现/上层约定一致）。
    #[inline]
    fn valid(v: &V) -> bool {
        *v >= V::zero()
    }

    pub fn insert(&mut self, elem: E) -> Result<K, E> {
        let v = elem.collexate();
        if !Self::valid(&v) {
            return Err(elem);
        }
        let id = self.id_map.insert(v);
        match self.store.insert(v, Pair(id, elem)) {
            Ok(()) => Ok(id),
            Err(pair) => {
                self.id_map.remove(id);
                Err(pair.1)
            }
        }
    }

    pub fn insert_with_id(&mut self, id: K, elem: E) -> Result<Option<E>, E> {
        let v = elem.collexate();
        if !Self::valid(&v) {
            return Err(elem);
        }
        let old_v = self.id_map.insert_with_id(id, v);
        let old_elem = match old_v {
            Some(ref old_v) => self.store.remove(old_v).map(|pair| pair.1),
            None => None,
        };
        match self.store.insert(v, Pair(id, elem)) {
            Ok(()) => Ok(old_elem),
            Err(pair) => {
                if let Some(old_elem) = old_elem {
                    let old_v = old_elem.collexate();
                    self.id_map.insert_with_id(id, old_v);
                    self.store.insert(old_v, Pair(id, old_elem)).ok();
                } else {
                    self.id_map.remove(id);
                }
                Err(pair.1)
            }
        }
    }

    pub fn remove(&mut self, id: K) -> Option<E> {
        let v = self.id_map.remove(id)?;
        if !Self::valid(&v) {
            return None;
        }
        self.store.remove(&v).map(|pair| pair.1)
    }

    pub fn get_with_id(&self, id: K) -> Option<&E> {
        let v = self.id_map.get(id)?;
        let pair = self.store.find_ge(v)?.1;
        (pair.collexate_ref() == v).then_some(&pair.1)
    }

    /// 排序键**恰好等于** `value` 的元素（`(id, 元素)`）。
    pub fn find_exact(&self, value: &V) -> Option<(&K, &E)> {
        let pair = self.store.find_ge(value)?.1;
        (pair.collexate_ref() == value).then_some((&pair.0, &pair.1))
    }

    /// 区间遍历：排序键在 `[lo, hi]`（含两端；`hi = None` 上无界），按升序，
    /// 推进 O(1)/元素。
    pub fn range(&self, lo: &V, hi: Option<&V>) -> RangeIter<'_, K, E, V>
    where
        V: PartialOrd,
    {
        if hi.is_some_and(|h| lo > h) {
            return RangeIter { inner: None };
        }
        let from = self.store.lower_bound_index(lo);
        let to = match hi {
            Some(h) => self.store.upper_bound_index(h),
            None => self.store.len(),
        };
        RangeIter {
            inner: Some(self.store.span_iter((from, to))),
        }
    }

    pub fn extend(&mut self, vec: Vec<E>) {
        for elem in vec {
            let v = elem.collexate();
            if !Self::valid(&v) {
                continue;
            }
            let id = self.id_map.insert(v);
            self.store.insert(v, Pair(id, elem)).ok();
        }
    }

    pub fn try_extend(&mut self, vec: Vec<E>) -> (Vec<K>, Vec<E>) {
        let mut accepted = Vec::with_capacity(vec.len());
        let mut rejected = Vec::new();
        for elem in vec {
            let v = elem.collexate();
            if !Self::valid(&v) {
                rejected.push(elem);
                continue;
            }
            let id = self.id_map.insert(v);
            match self.store.insert(v, Pair(id, elem)) {
                Ok(()) => accepted.push(id),
                Err(pair) => {
                    self.id_map.remove(id);
                    rejected.push(pair.1);
                }
            }
        }
        (accepted, rejected)
    }

    /// 重建块索引：批量写入后调用一次（写后不重建时查询走二分，结果仍正确）。
    pub fn rebuild(&mut self) {
        self.store.rebuild();
    }

    pub fn into_raw_parts(self) -> (IdMap<K, V>, JumpArray<V, Pair<K, E>>) {
        (self.id_map, self.store)
    }

    pub fn from_raw_parts(id_map: IdMap<K, V>, store: JumpArray<V, Pair<K, E>>) -> Self {
        Self { id_map, store }
    }

    // ==================== 修改 ====================

    pub fn modify<F, R>(&mut self, id: K, f: F) -> Result<R, ModifyError<R, E>>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(ModifyError::NotFound)?;
        let mut pair = match self.store.remove(&v) {
            Some(pair) => pair,
            None => return Err(ModifyError::NotFound),
        };
        let r = f(&mut pair.1);
        let new_v = pair.collexate();
        match self.store.insert(new_v, pair) {
            Ok(()) => {
                if new_v != v {
                    self.id_map.remove(id);
                    self.id_map.insert_with_id(id, new_v);
                }
                Ok(r)
            }
            Err(pair) => Err(ModifyError::InsertError(r, pair.1)),
        }
    }

    pub fn modify_by_val<F, R>(&mut self, value: &V, f: F) -> Result<R, ModifyError<R, E>>
    where
        F: FnOnce(&mut E) -> R,
        V: PartialEq,
    {
        if !Self::valid(value) {
            return Err(ModifyError::NotFound);
        }
        let mut pair = match self.store.remove(value) {
            Some(pair) => pair,
            None => return Err(ModifyError::NotFound),
        };
        let r = f(&mut pair.1);
        let new_v = pair.collexate();
        match self.store.insert(new_v, pair) {
            Ok(()) => Ok(r),
            Err(pair) => Err(ModifyError::InsertError(r, pair.1)),
        }
    }

    /// 与 [`Self::modify`] 同义，但插入失败时**回滚**旧键并放回原处。
    pub fn try_modify<F, R>(&mut self, id: K, f: F) -> Result<R, ModifyError<(R, V), ()>>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(ModifyError::NotFound)?;
        let mut pair = match self.store.remove(&v) {
            Some(pair) => pair,
            None => return Err(ModifyError::NotFound),
        };
        let r = f(&mut pair.1);
        let new_v = pair.collexate();
        if new_v == v {
            self.store.insert(v, pair).ok();
            return Ok(r);
        }
        match self.store.insert(new_v, pair) {
            Ok(()) => {
                self.id_map.remove(id);
                self.id_map.insert_with_id(id, new_v);
                Ok(r)
            }
            Err(mut rejected) => {
                *rejected.collexate_mut() = v;
                self.store.insert(v, rejected).ok();
                Err(ModifyError::InsertError((r, new_v), ()))
            }
        }
    }

    // ==================== 有序容器面（按排序键查询） ====================

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn clear(&mut self) {
        self.id_map = IdMap::with_id();
        self.store = JumpArray::new();
    }

    pub fn first(&self) -> Option<&Pair<K, E>> {
        self.store.first().map(|(_, p)| p)
    }

    pub fn last(&self) -> Option<&Pair<K, E>> {
        self.store.last().map(|(_, p)| p)
    }

    pub fn contains(&self, value: &V) -> bool {
        if !Self::valid(value) {
            return false;
        }
        self.store.find_ge(value).is_some_and(|(k, _)| k == value)
    }

    /// 最后一个 `key <= value`。
    pub fn find_le(&self, value: &V) -> Option<&Pair<K, E>> {
        if !Self::valid(value) {
            return None;
        }
        self.store.find_le(value).map(|(_, p)| p)
    }

    /// 最后一个 `key < value`。
    pub fn find_lt(&self, value: &V) -> Option<&Pair<K, E>> {
        if !Self::valid(value) {
            return None;
        }
        self.store.find_lt(value).map(|(_, p)| p)
    }

    /// 第一个 `key >= value`。
    pub fn find_ge(&self, value: &V) -> Option<&Pair<K, E>> {
        if !Self::valid(value) {
            return None;
        }
        self.store.find_ge(value).map(|(_, p)| p)
    }

    /// 第一个 `key > value`。
    pub fn find_gt(&self, value: &V) -> Option<&Pair<K, E>> {
        if !Self::valid(value) {
            return None;
        }
        self.store.find_gt(value).map(|(_, p)| p)
    }

    /// `(≤ value 的最后一个, > value 的第一个)`。
    pub fn find_le_and_gt(&self, value: &V) -> (PairRef<'_, K, E>, PairRef<'_, K, E>) {
        if !Self::valid(value) {
            return (None, self.first());
        }
        (self.find_le(value), self.find_gt(value))
    }
}

/// 只读迭代器：`(id, &元素)`，按排序键升序。
pub struct Iter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    inner: std::iter::Zip<std::slice::Iter<'a, T>, std::slice::Iter<'a, Pair<K, O>>>,
}

/// [`OrdIdMap::range`] 的迭代器：区间内 `(id, &元素)`，按排序键升序。
pub struct RangeIter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    /// `None` = 反向区间（空迭代器）。
    inner: Option<jump_array::RangeIter<'a, T, Pair<K, O>>>,
}

impl<'a, K, O, T> Iterator for RangeIter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    type Item = (K, &'a O);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.as_mut()?.next().map(|(_, pair)| (pair.0, &pair.1))
    }
}

impl<'a, K, O, T> Iterator for Iter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    type Item = (K, &'a O);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, pair)| (pair.0, &pair.1))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

pub struct IntoIter<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
{
    inner: std::vec::IntoIter<Pair<K, O>>,
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<K, O, T> Iterator for IntoIter<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
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
    T: FieldValue + jump_array::Key,
{
    pub fn iter(&self) -> Iter<'_, K, O, T> {
        let (keys, pairs) = self.store.as_slices();
        Iter {
            inner: keys.iter().zip(pairs.iter()),
        }
    }
}

impl<'a, K, O, T> IntoIterator for &'a OrdIdMap<K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key,
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
    T: FieldValue + jump_array::Key,
{
    type Item = (K, O);
    type IntoIter = IntoIter<K, O, T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            inner: self.store.into_sorted_vec().into_iter(),
            _marker: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DefaultId;

    #[derive(Debug, Clone, PartialEq)]
    struct Elem {
        at: i64,
        tag: u8,
    }

    impl Collexetable<i64> for Elem {
        fn collexate(&self) -> i64 {
            self.at
        }
        fn collexate_ref(&self) -> &i64 {
            &self.at
        }
        fn collexate_mut(&mut self) -> &mut i64 {
            &mut self.at
        }
    }

    fn el(at: i64, tag: u8) -> Elem {
        Elem { at, tag }
    }

    fn map(elems: &[(i64, u8)]) -> OrdIdMap<DefaultId, Elem, i64> {
        let mut m = OrdIdMap::<DefaultId, Elem, i64>::new();
        for &(at, tag) in elems {
            m.insert(el(at, tag)).unwrap();
        }
        m.rebuild();
        m
    }

    #[test]
    fn eq_compares_the_payload_not_just_the_sort_key() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(1, 11)).unwrap();
        assert_eq!(a.id_map, b.id_map, "前提：id_map 完全相同");
        assert!(a != b, "载荷不同必须判不等");
        let mut c = OrdIdMap::<DefaultId, Elem, i64>::new();
        c.insert(el(1, 10)).unwrap();
        assert!(a == c);
    }

    #[test]
    fn eq_respects_the_order() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        a.insert(el(2, 20)).unwrap();

        // 同样的 (载荷) 但插入顺序不同 ⇒ 发到的 id 与排序键的配对不同
        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(2, 20)).unwrap();
        b.insert(el(1, 10)).unwrap();
        let ids_a: Vec<_> = a.iter().map(|p| p.0).collect();
        let ids_b: Vec<_> = b.iter().map(|p| p.0).collect();
        assert_ne!(ids_a, ids_b, "前提：迭代出的 id 顺序确实不同");
        assert!(a != b, "配对不同应判不等");

        // 同一份内容 ⇒ 相等
        let mut c = OrdIdMap::<DefaultId, Elem, i64>::new();
        c.insert(el(1, 10)).unwrap();
        c.insert(el(2, 20)).unwrap();
        assert!(a == c);
    }

    #[test]
    fn eq_ignores_the_derived_id_map_index() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        a.insert(el(2, 20)).unwrap();
        let mut b = a.clone();
        b.id_map = IdMap::new();
        assert!(b.id_map.is_empty());
        assert!(a == b, "id_map 是派生索引，不影响相等判定");
    }

    #[test]
    fn eq_ignores_the_max_id_watermark() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(1, 10)).unwrap();
        let tmp = b.insert(el(9, 99)).unwrap();
        b.remove(tmp);
        assert!(a == b);
        let mut d = OrdIdMap::<DefaultId, Elem, i64>::new();
        d.insert(el(1, 10)).unwrap();
        d.insert(el(2, 20)).unwrap();
        assert!(a != d);
    }

    /// `find_*` 的边界语义（含"≤ 命中本身""> 不取本身"、负键 None）。
    #[test]
    fn find_boundaries_match_the_contract() {
        let m = map(&[(2, 20), (4, 40), (6, 60)]);
        assert_eq!(m.find_le(&1).map(|p| p.1.tag), None);
        assert_eq!(m.find_le(&2).map(|p| p.1.tag), Some(20));
        assert_eq!(m.find_le(&3).map(|p| p.1.tag), Some(20));
        assert_eq!(m.find_le(&9).map(|p| p.1.tag), Some(60));
        assert_eq!(m.find_lt(&2).map(|p| p.1.tag), None);
        assert_eq!(m.find_lt(&3).map(|p| p.1.tag), Some(20));
        assert_eq!(m.find_ge(&1).map(|p| p.1.tag), Some(20));
        assert_eq!(m.find_ge(&2).map(|p| p.1.tag), Some(20));
        assert_eq!(m.find_gt(&2).map(|p| p.1.tag), Some(40));
        assert_eq!(m.find_gt(&6).map(|p| p.1.tag), None);
        let (le, gt) = m.find_le_and_gt(&2);
        assert_eq!(le.map(|p| p.1.tag), Some(20));
        assert_eq!(gt.map(|p| p.1.tag), Some(40));
        assert_eq!(m.find_le(&-1).map(|p| p.1.tag), None, "负键查询一律 None");
        assert_eq!(m.find_ge(&-1).map(|p| p.1.tag), None);
    }

    /// 区间 `[lo, hi]`：含两端、按序、`hi=None` 上无界、负下界从头开始。
    #[test]
    fn range_walks_bounds_inclusively_in_order() {
        let m = map(&[(0, 0), (2, 2), (4, 4), (5, 5), (9, 9), (12, 12)]);
        let tags = |lo: i64, hi: Option<i64>| -> Vec<u8> {
            m.range(&lo, hi.as_ref()).map(|(_, e)| e.tag).collect()
        };
        assert_eq!(tags(4, Some(9)), vec![4, 5, 9], "含两端");
        assert_eq!(tags(3, Some(9)), vec![4, 5, 9], "空档下界从第一个 ≥ lo 开始");
        assert_eq!(tags(5, None), vec![5, 9, 12], "上无界");
        assert_eq!(tags(-3, Some(2)), vec![0, 2], "负下界等价于从头开始");
        assert_eq!(tags(13, None), Vec::<u8>::new(), "下界越界");
        assert_eq!(tags(7, Some(8)), Vec::<u8>::new(), "区间无元素");
        let empty = OrdIdMap::<DefaultId, Elem, i64>::new();
        assert_eq!(empty.range(&0, Some(&9)).count(), 0, "空集合");
        assert_eq!(tags(9, Some(4)), Vec::<u8>::new(), "反向区间为空");
    }

    /// 插入拒绝重复键与负键；`modify` 改键会搬位；`try_modify` 失败回滚。
    #[test]
    fn insert_rejects_duplicates_and_modify_moves_elements() {
        let mut m = map(&[(2, 20), (4, 40), (6, 60)]);
        assert!(m.insert(el(4, 99)).is_err(), "重复键拒绝");
        assert!(m.insert(el(-1, 99)).is_err(), "负键拒绝");
        // 改键 ⇒ 搬到新位置。
        m.modify(DefaultId(2), |e| e.at = 5).unwrap();
        assert!(!m.contains(&4));
        assert_eq!(m.find_ge(&5).map(|p| p.1.tag), Some(40));
        // try_modify 改成已占用的键 ⇒ 报错并回滚到原键。
        assert!(matches!(
            m.try_modify(DefaultId(3), |e| e.at = 5),
            Err(ModifyError::InsertError(..))
        ));
        assert_eq!(m.find_le(&6).map(|p| p.1.tag), Some(60), "回滚后仍在原键");
    }

    /// 显式 `rebuild` 之后块索引生效；不重建时查询仍正确（二分）。
    #[test]
    fn rebuild_enables_the_index() {
        let mut m = OrdIdMap::<DefaultId, Elem, i64>::new();
        for i in 0..3000i64 {
            m.insert(el(i * 2, 0)).unwrap();
        }
        assert!(m.store.is_dirty());
        assert_eq!(m.find_le(&1500).map(|p| p.1.at), Some(1500));
        m.rebuild();
        assert!(!m.store.is_dirty());
        assert_eq!(m.find_ge(&1501).map(|p| p.1.at), Some(1502));
    }
}
