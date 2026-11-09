

use std::ops::{Deref, DerefMut};
use field_collex::{Collexetable, ConstUnit, FieldValue};
use field_collex::collex::ModifyError;
use crate::store::{Cursor, Store, StoreIntoIter, StoreIter};
use crate::{Id, IdMap, Pair};

#[derive(Debug, Clone)]
pub struct OrdIdMap<K,O,T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    pub id_map: IdMap<K,T>,
    pub store: crate::store::Store<Pair<K,O>,T>
}

impl<K,E,V> Deref for OrdIdMap<K,E,V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    type Target = crate::store::Store<Pair<K,E>,V>;
    fn deref(&self) -> &Self::Target {
        &self.store
    }
}

impl<K,E,V> DerefMut for OrdIdMap<K,E,V>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.store
    }
}

/// 相等 = **按排序键顺序，元素 id 与载荷都相同**。`id_map` 与 `max_id` 都**不参与**。
///
/// ## 为什么不能 derive，也不能只比 `id_map`
///
/// `OrdIdMap` 的两个字段都不是"内容"本身：
/// - `id_map: IdMap<K, T>` 只存 **id → 排序键**，**不含载荷**。只比它会把
///   "同 id 同排序键、载荷不同"判成相等 —— 那是**假阳性**，比"判成不等"危险得多
///   （撤销会以为谱面没变而跳过还原）
/// - `collex` 里的 `Slot { prev, next, values }` 是**链表布局**，`Vec` 里可能留着
///   删除后的空洞。derive `Collex` 会去比**容量/空洞位置**，那是历史而非内容
///
/// 真正的内容是 `collex` 按排序键迭代出的 `Pair<K, O>` 序列（含 id、载荷与
/// 相对顺序）。故手写实现：比迭代序列，长度不同立刻不等。
///
/// `id_map` 被排除还带来一个好处：它与 `collex` 若失配（外部直接改 `pub` 字段、
/// 从旧版本反序列化恢复），这里仍按**内容**判定，不会把一次数据损坏误报成
/// "内容变了"，也不会掩盖它。
impl<K, E, V> PartialEq for OrdIdMap<K, E, V>
where
    K: Id + PartialEq,
    E: Collexetable<V> + PartialEq,
    V: FieldValue,
{
    fn eq(&self, other: &Self) -> bool {
        let mut a = self.store.iter();
        let mut b = other.store.iter();
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
            store: Store::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let _ = capacity;
        Self {
            id_map: IdMap::with_id_capacity(capacity),
            store: Store::new(),
        }
    }

    pub fn with_elements(vec: Vec<E>) -> Self {
        let mut id_map = IdMap::with_id();
        let mut store = Store::new();
        for elem in vec {
            let v = elem.collexate();
            let id = id_map.insert(v);
            store.insert(Pair(id, elem)).ok();
        }
        Self { id_map, store }
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
        match self.store.insert(Pair(id, elem)) {
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
            self.store.remove(old_v).ok().map(|pair| pair.1)
        } else {
            None
        };

        match self.store.insert(Pair(id, elem)) {
            Ok(()) => Ok(old_elem),
            Err(pair) => {
                // 回滚：恢复旧元素
                if let Some(old_elem) = old_elem {
                    let old_v = old_elem.collexate();
                    self.id_map.insert_with_id(id, old_v);
                    self.store.insert(Pair(id, old_elem)).ok();
                } else {
                    self.id_map.remove(id);
                }
                Err(pair.1)
            }
        }
    }

    pub fn remove(&mut self, id: K) -> Option<E> {
        let v = self.id_map.remove(id)?;
        self.store.remove(&v).ok().map(|pair| pair.1)
    }

    pub fn get_with_id(&self, id: K) -> Option<&E> {
        let v = self.id_map.get(id)?;
        let pair = self.store.find_ge(v)?;
        if pair.collexate_ref() == v {
            Some(&pair.1)
        } else {
            None
        }
    }

    /// 按排序键定位：`(prev, current, next, cursor)`。
    ///
    /// - `prev`：最后一个排序键 `< value` 的元素；
    /// - `current`：排序键 `== value` 的元素；
    /// - `next`：第一个排序键 `> value` 的元素；
    /// - `cursor`：指向第一个排序键 `>= value` 的元素（`current` 或 `next`），空集合为 `None`。
    ///
    /// 复杂度：槽位下标是 `value / unit` 的算术定位，槽内至多一次二分 —— **与元素总数无关**。
    ///
    /// ## 游标怎么用
    ///
    /// 游标是"记住这次找到了哪"的把手，配 [`Self::step`] 做**同一批查询里的顺序推进**
    /// （遍历一段区间：先 `find(lo)` 播种，再对每个已读到的排序键调 `step`）。
    ///
    /// ⚠️ **不要跨"集合被改动"复用它**：位置是 `(槽下标, 槽内下标)`，往同一个槽插入会让
    /// 槽内下标整体后移、删除可能合并槽。游标只在一次遍历内可靠；每次遍历重新 `find`。
    pub fn find(
        &self,
        value: &V,
    ) -> (
        Option<&Pair<K, E>>,
        Option<&Pair<K, E>>,
        Option<&Pair<K, E>>,
        Option<Cursor>,
    ) {
        self.store.find(value)
    }

    /// 从游标当前位置推进到 `value`，返回 `(prev, current, next)`，语义同 [`Self::find`]。
    ///
    /// 推进后游标停在找到的位置（写回 `cursor.pos`），所以顺序遍历一段区间时，
    /// 每次只从**上一个元素**往前走一格，而不是每次从集合起点重新定位。
    ///
    /// 游标的复用边界见 [`Self::find`] 的文档。
    pub fn step<'a>(
        &'a self,
        cursor: &mut Cursor,
        value: &V,
    ) -> (
        Option<&'a Pair<K, E>>,
        Option<&'a Pair<K, E>>,
        Option<&'a Pair<K, E>>,
    ) {
        self.store.step(cursor, value)
    }

    pub fn extend(&mut self, vec: Vec<E>) {
        for elem in vec {
            let v = elem.collexate();
            let id = self.id_map.insert(v);
            self.store.insert(Pair(id, elem)).ok();
        }
    }

    pub fn try_extend(&mut self, vec: Vec<E>) -> (Vec<K>, Vec<E>) {
        let mut accepted = Vec::with_capacity(vec.len());
        let mut rejected = Vec::new();
        for elem in vec {
            let v = elem.collexate();
            let id = self.id_map.insert(v);
            match self.store.insert(Pair(id, elem)) {
                Ok(()) => accepted.push(id),
                Err(pair) => {
                    self.id_map.remove(id);
                    rejected.push(pair.1);
                }
            }
        }
        (accepted, rejected)
    }

    pub fn into_raw_parts(self) -> (IdMap<K,V>, Store<Pair<K,E>,V>) {
        (self.id_map, self.store)
    }

    pub fn from_raw_parts(id_map: IdMap<K,V>, store: Store<Pair<K,E>,V>) -> Self {
        Self { id_map, store }
    }

    pub fn modify<F, R>(&mut self, id: K, f: F) -> Result<R, ModifyError<R, E>>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(ModifyError::NotFound)?;

        match self.store.modify(&v, |pair| {
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
        match self.store.modify(value, |pair| {
            let r = f(&mut pair.1);
            let new_v = *pair.collexate_ref();
            (r, new_v)
        }) {
            Ok((r, _new_v)) => Ok(r),
            Err(ModifyError::NotFound) => Err(ModifyError::NotFound),
            Err(ModifyError::InsertError((r, _), pair)) => Err(ModifyError::InsertError(r, pair.1)),
        }
    }

    pub fn try_modify<F, R>(&mut self, id: K, f: F) -> Result<R, ModifyError<(R, V), ()>>
    where
        F: FnOnce(&mut E) -> R,
    {
        let v = *self.id_map.get(id).ok_or(ModifyError::NotFound)?;

        match self.store.try_modify(&v, |pair| {
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
            Err(err) => Err(err),
        }
    }
}

pub struct Iter<'a, K, O, T>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    inner: StoreIter<'a, Pair<K, O>, T>,
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
    inner: StoreIntoIter<Pair<K, O>, T>,
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
            inner: self.store.iter(),
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
            inner: self.store.into_iter(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DefaultId;

    /// 最小可排序载荷：一个排序键。`Collexetable` 只需这三个方法。
    ///
    /// 排序键用 `i64` 而不是浮点：`FieldValue` 只对整数实现（`NumOps`+`Zero`+`Ord`），
    /// 浮点还得先解决 NaN 的相等语义才能进 `Ord`。
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

    /// 相等**必须看载荷**。
    ///
    /// 这条是回归网：`OrdIdMap.id_map` 只存 `id → 排序键`、**不含载荷**，
    /// 曾经有一版实现只比 `id_map`，于是"同 id 同排序键、`tag` 不同"的两个
    /// map 被判成**相等**。对撤销栈来说那是假阳性 ⇒ 会以为谱面没改而跳过还原。
    #[test]
    fn eq_compares_the_payload_not_just_the_sort_key() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();

        // 同 id、同排序键，**只有载荷不同**
        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(1, 11)).unwrap();
        assert_eq!(
            a.id_map, b.id_map,
            "前提：两者的 id_map（id→排序键）完全相同"
        );
        assert!(a != b, "载荷不同必须判不等（否则是假阳性，会跳过撤销还原）");

        // 载荷相同则相等
        let mut c = OrdIdMap::<DefaultId, Elem, i64>::new();
        c.insert(el(1, 10)).unwrap();
        assert!(a == c);
    }

    /// 相等 = **按排序键顺序，id 与载荷都相同** ⇒ 顺序不同即不等。
    #[test]
    fn eq_respects_the_collex_order() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        a.insert(el(2, 20)).unwrap();

        // 同样的 (id, 载荷) 配对，但插入顺序不同 ⇒ 迭代出的 id 序列不同
        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(2, 20)).unwrap();
        b.insert(el(1, 10)).unwrap();
        let ids_a: Vec<_> = a.iter().map(|p| p.0).collect();
        let ids_b: Vec<_> = b.iter().map(|p| p.0).collect();
        assert_ne!(ids_a, ids_b, "前提：两者迭代出的 id 顺序确实不同");
        assert!(a != b, "顺序不同应判不等");

        // 但**内容集合**相同（只是发号顺序不同）—— 这是"id 是身份"的必然结果，
        // 记在这里提醒：跨会话重建的谱面 id 可能不同，不要指望 PartialEq 能判等
        assert_eq!(a.len(), b.len());
    }

    /// `id_map` 是派生索引：把它弄坏（清空）**不影响**相等判定。
    ///
    /// 理由：`OrdIdMap` 的载荷只存在 `collex` 里，`id_map` 丢了还能从
    /// `collex` 重建（`rebuild` 路径）。若相等性依赖它，一次索引失配会被
    /// 误报成"内容变了"。
    #[test]
    fn eq_ignores_the_derived_id_map_index() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();
        a.insert(el(2, 20)).unwrap();

        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(1, 10)).unwrap();
        b.insert(el(2, 20)).unwrap();
        assert!(a == b);

        b.id_map = IdMap::new(); // 人为失配
        assert!(b.id_map.is_empty(), "前提：id_map 确实被清空");
        assert!(a == b, "id_map 是派生索引，不该影响相等判定");
    }

    /// 删掉最大号再重插，水位升高但存活元素相同 ⇒ 判相等。
    #[test]
    fn eq_ignores_the_max_id_watermark() {
        let mut a = OrdIdMap::<DefaultId, Elem, i64>::new();
        a.insert(el(1, 10)).unwrap();

        let mut b = OrdIdMap::<DefaultId, Elem, i64>::new();
        b.insert(el(1, 10)).unwrap();
        let tmp = b.insert(el(9, 99)).unwrap();
        b.remove(tmp).unwrap();
        assert!(a == b, "水位更高但存活元素相同，必须判相等");

        // 元素个数不同 ⇒ 不等
        let mut d = OrdIdMap::<DefaultId, Elem, i64>::new();
        d.insert(el(1, 10)).unwrap();
        d.insert(el(2, 20)).unwrap();
        assert!(a != d, "存活元素个数不同，应判不等");
    }

    /// ⭐ 游标版区间遍历：`find` 播种 + `step` 前进，产出必须与"全量迭代再过滤"一致。
    ///
    /// 这是 `vault-chart::KeyLane::keys_between` 依赖的语义：`step(cursor, 上一颗的键)`
    /// 返回的 `next` 恰好是"第一个比它大的元素"，所以按顺序推进不重不漏。
    #[test]
    fn find_and_step_walk_a_range_in_the_same_order_as_a_full_iteration() {
        let mut m = OrdIdMap::<DefaultId, Elem, i64>::new();
        for at in [0i64, 2, 4, 5, 9, 12] {
            m.insert(el(at, at as u8)).unwrap();
        }

        // 起点：第一个 >= 4；游标也落在它上面。
        let (prev, cur, next, mut cursor) = m.find(&4);
        assert_eq!(prev.map(|p| p.at), Some(2), "prev 是最后一个 < 4 的");
        assert_eq!(cur.map(|p| p.at), Some(4), "current 恰好命中");
        assert_eq!(next.map(|p| p.at), Some(5), "next 是第一个 > 4 的");
        let mut cursor = cursor.expect("非空集合必有游标");

        // 从 4 开始按 step 前进，直到 9（含）——与过滤全量迭代的结果比对。
        let mut walked = vec![cur.unwrap().at];
        loop {
            let last = *walked.last().expect("非空");
            let (_, same, nxt) = m.step(&mut cursor, &last);
            assert_eq!(same.map(|p| p.at), Some(last), "step 先定位到 value 本身");
            let Some(n) = nxt else { break };
            if n.at > 9 {
                break;
            }
            walked.push(n.at);
        }
        let want: Vec<i64> = m
            .iter()
            .map(|(_, e)| e.at)
            .filter(|a| (4..=9).contains(a))
            .collect();
        assert_eq!(walked, want, "游标遍历与全量过滤必须一致");

        // 起点未命中时（3 落在空档）：current 为 None，next 是第一个 > 3。
        let (_, cur, next, _) = m.find(&3);
        assert!(cur.is_none(), "没有恰好等于 3 的元素");
        assert_eq!(next.map(|p| p.at), Some(4), "next 是第一个 > 3 的");

        // 空集合：三个位置都空、游标为 None。
        let empty = OrdIdMap::<DefaultId, Elem, i64>::new();
        let (prev, cur, next, cursor) = empty.find(&0);
        assert!(prev.is_none() && cur.is_none() && next.is_none());
        assert!(cursor.is_none(), "空集合没有游标");
    }
}