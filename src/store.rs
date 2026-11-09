//! 有序存储适配层：`OrdIdMap` 里那个"按排序键有序"的容器。
//!
//! 默认实现是 [`field_collex::Collex`]（按 `排序键 / unit` 分槽 + 槽内有序，
//! 自带 `prev/next` 链表与游标）。开 feature `store-btreemap` 换成
//! [`std::collections::BTreeMap`]。
//!
//! ## 为什么做成适配层而不是让上层换类型
//!
//! `OrdIdMap` 用 `Deref` 把容器的方法直接暴露给调用方（`find_le` / `find` / `step`
//! / `iter` / `modify` …），所以只要这里的**方法名与语义逐字一致**，上层一行都不用改
//! —— 这正是"对比两种容器在真实负载上的差异"需要的：两个构建跑同一份代码。
//!
//! ## 语义契约（两种实现都必须满足）
//!
//! * 排序键唯一：插入已存在的键 ⇒ `Err(elem)`，**不覆盖**；
//! * 排序键为负 ⇒ 拒绝（`insert` / `remove` 报错，查询返回 `None`）；
//! * `find_le` = 最后一个 `≤ v`，`find_lt` = 最后一个 `< v`，
//!   `find_ge` = 第一个 `≥ v`，`find_gt` = 第一个 `> v`；
//! * `find_le_and_gt` = (`≤ v` 的最后一个, `> v` 的第一个)；
//! * `find` = (`< v` 的最后一个, `== v`, `> v` 的第一个, 游标)；
//! * `step` 要求查询序列**非递减**（上游就是按时间顺序查的），推进游标。
//!
//! BTreeMap 版直接用 `range` 表达这些语义；游标在它这里只是"记住查过的值"，
//! 每次 `step` 仍然是一次 `O(log n)` 的 range 定位（`Collex` 那边是槽内推进）。

pub use field_collex::collex::ModifyError;

/// 有序容器（默认 `Collex`；`store-btreemap` 时是 [`BTreeStore`]）。
#[cfg(not(feature = "store-btreemap"))]
pub type Store<E, V> = field_collex::Collex<E, V>;

/// 游标类型（默认 `CollexCursor`；`store-btreemap` 时是 [`BTreeCursor`]）。
#[cfg(not(feature = "store-btreemap"))]
pub type Cursor = field_collex::collex::CollexCursor;

/// 只读迭代器（`OrdIdMap::iter` 包的是它）。
#[cfg(not(feature = "store-btreemap"))]
pub type StoreIter<'a, E, V> = field_collex::collex::iter::Iter<'a, E, V>;

/// 所有权迭代器（`OrdIdMap::into_iter` 包的是它）。
#[cfg(not(feature = "store-btreemap"))]
pub type StoreIntoIter<E, V> = field_collex::collex::iter::IntoIter<E, V>;

// ==================== BTreeMap 版 ====================

#[cfg(feature = "store-btreemap")]
pub use btree::{BTreeCursor, BTreeStore};

#[cfg(feature = "store-btreemap")]
pub type Store<E, V> = btree::BTreeStore<E, V>;
#[cfg(feature = "store-btreemap")]
pub type Cursor = btree::BTreeCursor;
#[cfg(feature = "store-btreemap")]
pub type StoreIter<'a, E, V> = std::collections::btree_map::Values<'a, V, E>;
#[cfg(feature = "store-btreemap")]
pub type StoreIntoIter<E, V> = std::collections::btree_map::IntoValues<V, E>;

#[cfg(feature = "store-btreemap")]
mod btree {
    use field_collex::{Collexetable, FieldValue};
    use super::ModifyError;
    use std::collections::BTreeMap;

    /// `Collex` 的 BTreeMap 替身：`排序键 → 元素`，键唯一、拒绝负键。
    ///
    /// 方法名/签名逐字对齐 `Collex`（含 `find*` 那套与 `modify` 的两个变体），
    /// 因为上层是经 `Deref` 直接调它们的。
    #[derive(Debug, Clone)]
    pub struct BTreeStore<E, V> {
        map: BTreeMap<V, E>,
    }

    /// 游标：对 BTreeMap 只是"上次查到哪"，不参与定位（每次 `step` 都重新 range）。
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct BTreeCursor {
        /// 上次 `find` / `step` 查的键。
        last: Option<()>,
    }

    impl<E, V> Default for BTreeStore<E, V> {
        fn default() -> Self {
            Self { map: BTreeMap::new() }
        }
    }

    impl<E, V> BTreeStore<E, V>
    where
        E: Collexetable<V>,
        V: FieldValue,
    {
        pub fn new() -> Self {
            Self { map: BTreeMap::new() }
        }

        /// 容量参数对 BTreeMap 没有意义（只为与 `Collex` 同签名）。
        pub fn with_capacity(_capacity: usize) -> Self {
            Self::new()
        }

        pub fn len(&self) -> usize {
            self.map.len()
        }

        pub fn is_empty(&self) -> bool {
            self.map.is_empty()
        }

        pub fn first(&self) -> Option<&E> {
            self.map.values().next()
        }

        pub fn last(&self) -> Option<&E> {
            self.map.values().next_back()
        }

        pub fn contains(&self, value: &V) -> bool {
            self.map.contains_key(value)
        }

        /// 插入：负键 / 重复键都拒绝（与 `Collex` 同语义）。
        pub fn insert(&mut self, elem: E) -> Result<(), E> {
            let v = *elem.collexate_ref();
            if v.lt(&V::zero()) {
                return Err(elem);
            }
            if self.map.contains_key(&v) {
                return Err(elem);
            }
            self.map.insert(v, elem);
            Ok(())
        }

        pub fn remove(&mut self, value: &V) -> Result<E, ()> {
            if value.lt(&V::zero()) {
                return Err(());
            }
            self.map.remove(value).ok_or(())
        }

        pub fn clear(&mut self) {
            self.map.clear();
        }

        pub fn iter(&self) -> std::collections::btree_map::Values<'_, V, E> {
            self.map.values()
        }

        // ---- 查询：语义见模块文档 ----

        pub fn find_le(&self, value: &V) -> Option<&E> {
            if value.lt(&V::zero()) {
                return None;
            }
            self.map.range(..=*value).next_back().map(|(_, e)| e)
        }

        pub fn find_lt(&self, value: &V) -> Option<&E> {
            if value.lt(&V::zero()) {
                return None;
            }
            self.map.range(..*value).next_back().map(|(_, e)| e)
        }

        pub fn find_ge(&self, value: &V) -> Option<&E> {
            if value.lt(&V::zero()) {
                return None;
            }
            self.map.range(*value..).next().map(|(_, e)| e)
        }

        pub fn find_gt(&self, value: &V) -> Option<&E> {
            if value.lt(&V::zero()) {
                return None;
            }
            self.map
                .range((std::ops::Bound::Excluded(*value), std::ops::Bound::Unbounded))
                .next()
                .map(|(_, e)| e)
        }

        pub fn find_le_and_gt(&self, value: &V) -> (Option<&E>, Option<&E>) {
            if value.lt(&V::zero()) {
                return (None, self.first());
            }
            let le = self.map.range(..=*value).next_back().map(|(_, e)| e);
            let gt = self
                .map
                .range((std::ops::Bound::Excluded(*value), std::ops::Bound::Unbounded))
                .next()
                .map(|(_, e)| e);
            (le, gt)
        }

        pub fn find(
            &self,
            value: &V,
        ) -> (
            Option<&E>,
            Option<&E>,
            Option<&E>,
            Option<BTreeCursor>,
        ) {
            if value.lt(&V::zero()) {
                let first = self.first();
                let cursor = first.map(|_| BTreeCursor { last: Some(()) });
                return (None, None, first, cursor);
            }
            let prev = self.map.range(..*value).next_back().map(|(_, e)| e);
            let cur = self.map.get(value);
            let next = self
                .map
                .range((std::ops::Bound::Excluded(*value), std::ops::Bound::Unbounded))
                .next()
                .map(|(_, e)| e);
            // 与 `Collex` 一致：只有"这一点之后还有东西"时才给游标。
            let cursor = cur.or(next).map(|_| BTreeCursor { last: Some(()) });
            (prev, cur, next, cursor)
        }

        /// 推进游标（要求查询序列非递减）。
        ///
        /// BTreeMap 没有"槽内游标"可推，这里每次做 range 定位 —— 语义与
        /// `Collex::step` 相同（`prev` 严格小于、`next` 严格大于）。
        pub fn step<'a>(
            &'a self,
            cursor: &mut BTreeCursor,
            value: &V,
        ) -> (Option<&'a E>, Option<&'a E>, Option<&'a E>) {
            if value.lt(&V::zero()) {
                return (None, None, self.first());
            }
            let prev = self.map.range(..*value).next_back().map(|(_, e)| e);
            let cur = self.map.get(value);
            let next = self
                .map
                .range((std::ops::Bound::Excluded(*value), std::ops::Bound::Unbounded))
                .next()
                .map(|(_, e)| e);
            cursor.last = Some(());
            (prev, cur, next)
        }

        // ---- 修改（含"改完要搬位"的语义） ----

        pub fn modify<F, R>(&mut self, value: &V, op: F) -> Result<R, ModifyError<R, E>>
        where
            F: FnOnce(&mut E) -> R,
        {
            let mut elem = self.remove(value).map_err(|_| ModifyError::NotFound)?;
            let old_val = *elem.collexate_ref();
            let result = op(&mut elem);
            if elem.collexate_ref().eq(&old_val) {
                self.insert(elem).ok();
                Ok(result)
            } else {
                match self.insert(elem) {
                    Ok(()) => Ok(result),
                    Err(e) => Err(ModifyError::InsertError(result, e)),
                }
            }
        }

        /// 与 [`Self::modify`] 同义，但插入失败时**回滚**旧值（与 `Collex::try_modify` 一致）。
        pub fn try_modify<F, R>(&mut self, value: &V, op: F) -> Result<R, ModifyError<R, ()>>
        where
            F: FnOnce(&mut E) -> R,
        {
            let mut elem = self.remove(value).map_err(|_| ModifyError::NotFound)?;
            let old_val = *elem.collexate_ref();
            let result = op(&mut elem);
            if elem.collexate_ref().eq(&old_val) {
                self.insert(elem).ok();
                Ok(result)
            } else {
                match self.insert(elem) {
                    Ok(()) => Ok(result),
                    Err(mut rejected) => {
                        *rejected.collexate_mut() = old_val;
                        self.insert(rejected).ok();
                        Err(ModifyError::InsertError(result, ()))
                    }
                }
            }
        }
    }

    impl<E, V> IntoIterator for BTreeStore<E, V> {
        type Item = E;
        type IntoIter = std::collections::btree_map::IntoValues<V, E>;

        fn into_iter(self) -> Self::IntoIter {
            self.map.into_values()
        }
    }

    impl<'a, E, V> IntoIterator for &'a BTreeStore<E, V> {
        type Item = &'a E;
        type IntoIter = std::collections::btree_map::Values<'a, V, E>;

        fn into_iter(self) -> Self::IntoIter {
            self.map.values()
        }
    }

    /// 落盘形状与 `Collex` 一致：`[元素, …]`（按排序键顺序）。
    ///
    /// 存档格式因此两种 feature 下相同 —— 对比时可以共用同一批谱面文件。
    impl<E, V> serde::Serialize for BTreeStore<E, V>
    where
        E: serde::Serialize,
    {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(Some(self.map.len()))?;
            for e in self.map.values() {
                seq.serialize_element(e)?;
            }
            seq.end()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use field_collex::Collexetable;

    #[derive(Debug, Clone, PartialEq, serde::Serialize)]
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

    fn store() -> Store<Elem, i64> {
        let mut s = Store::new();
        for (at, tag) in [(2, 20u8), (4, 40), (6, 60)] {
            s.insert(Elem { at, tag }).unwrap();
        }
        s
    }

    /// `find_*` 的边界语义（两种实现必须一致，这是整套适配的契约）。
    #[test]
    fn find_boundaries_match_the_contract() {
        let s = store();
        assert_eq!(s.find_le(&1).map(|e| e.tag), None);
        assert_eq!(s.find_le(&2).map(|e| e.tag), Some(20), "≤ 命中本身");
        assert_eq!(s.find_le(&3).map(|e| e.tag), Some(20));
        assert_eq!(s.find_le(&9).map(|e| e.tag), Some(60));
        assert_eq!(s.find_lt(&2).map(|e| e.tag), None, "< 不取本身");
        assert_eq!(s.find_lt(&3).map(|e| e.tag), Some(20));
        assert_eq!(s.find_ge(&1).map(|e| e.tag), Some(20));
        assert_eq!(s.find_ge(&2).map(|e| e.tag), Some(20), "≥ 命中本身");
        assert_eq!(s.find_gt(&2).map(|e| e.tag), Some(40), "> 不取本身");
        assert_eq!(s.find_gt(&6).map(|e| e.tag), None);
        assert_eq!(s.find_le_and_gt(&2), (Some(&Elem { at: 2, tag: 20 }), Some(&Elem { at: 4, tag: 40 })));
        // 负键查询一律 None（`Collex` 也只收非负）。
        assert_eq!(s.find_le(&-1).map(|e| e.tag), None);
        assert_eq!(s.find_ge(&-1).map(|e| e.tag), None);
        assert_eq!(s.find(&-1).0, None);
        assert_eq!(s.find(&-1).2.map(|e| e.tag), Some(20), "负键的 next 是第一个元素");
    }

    /// `find` 与 `step`：`(prev, cur, next)` 且游标只在"后面还有"时给出。
    #[test]
    fn find_and_step_advance_over_the_sequence() {
        let s = store();
        let (prev, cur, next, cursor) = s.find(&4);
        assert_eq!(prev.map(|e| e.tag), Some(20));
        assert_eq!(cur.map(|e| e.tag), Some(40));
        assert_eq!(next.map(|e| e.tag), Some(60));
        let mut cursor = cursor.expect("后面还有元素 ⇒ 有游标");

        let (p, c, n) = s.step(&mut cursor, &4);
        assert_eq!((p.map(|e| e.tag), c.map(|e| e.tag), n.map(|e| e.tag)), (Some(20), Some(40), Some(60)));
        let (_, _, n) = s.step(&mut cursor, &6);
        assert_eq!(n.map(|e| e.tag), None, "末尾之后没有 next");
        // 查询超出末尾：prev 是最后一个，cur/next 都没有（与 `Collex` 一致）。
        let (prev, cur, next, cursor) = s.find(&99);
        assert_eq!(prev.map(|e| e.tag), Some(60));
        assert_eq!((cur.map(|e| e.tag), next.map(|e| e.tag)), (None, None));
        assert!(cursor.is_none(), "末尾之后不给游标");
    }

    /// 插入/修改的唯一键与搬位语义。
    #[test]
    fn insert_rejects_duplicates_and_modify_moves_elements() {
        let mut s = store();
        assert!(s.insert(Elem { at: 4, tag: 99 }).is_err(), "重复键拒绝");
        assert!(s.insert(Elem { at: -1, tag: 99 }).is_err(), "负键拒绝");
        // 改键 ⇒ 搬到新位置；旧位置空出来。
        s.modify(&4, |e| e.at = 5).unwrap();
        assert!(!s.contains(&4));
        assert_eq!(s.find_ge(&5).map(|e| e.tag), Some(40));
        // `try_modify` 改成已占用的键 ⇒ 报错并**回滚**到原键。
        assert!(matches!(s.try_modify(&6, |e| e.at = 5), Err(ModifyError::InsertError(..))));
        assert_eq!(s.find_le(&6).map(|e| e.tag), Some(60), "回滚后仍在原键");
        // `modify` 改成已占用的键 ⇒ 报错且元素被**排出**（与 `Collex::modify` 同语义）。
        assert!(matches!(s.modify(&5, |e| e.at = 6), Err(ModifyError::InsertError(..))));
        assert!(!s.contains(&5), "失败的 modify 不把元素放回原位");
    }

    /// 序列化形状是"按序的元素数组"（两种 feature 共用同一批谱面文件）。
    #[test]
    fn serializes_as_an_ordered_sequence() {
        let s = store();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"[{"at":2,"tag":20},{"at":4,"tag":40},{"at":6,"tag":60}]"#);
    }
}
