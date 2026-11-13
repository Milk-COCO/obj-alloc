//! `OrdIdMap` 的序列化：`{ max_id, items: [Pair…] }`（按排序键升序）。
//!
//! 反序列化不依赖存储的 `Deserialize`：先读 `Vec<Pair>`，再逐个 `insert`
//! 进 `JumpArray`。

use field_collex::{Collexetable, ConstUnit, FieldValue};
use jump_array::JumpArray;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Id, IdMap, Pair};

/// 把存储按升序序列化成 `[Pair…]`。
struct Items<'a, K, O, T>(&'a JumpArray<T, Pair<K, O>>)
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue + jump_array::Key;

impl<K, O, T> Serialize for Items<'_, K, O, T>
where
    K: Id,
    O: Collexetable<T> + Serialize,
    T: FieldValue + jump_array::Key,
    Pair<K, O>: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for (_, pair) in self.0.iter() {
            seq.serialize_element(pair)?;
        }
        seq.end()
    }
}

impl<K, O, T> Serialize for crate::OrdIdMap<K, O, T>
where
    K: Id + Serialize,
    O: Collexetable<T> + Serialize,
    T: FieldValue + jump_array::Key + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("OrdIdMap", 2)?;
        st.serialize_field("max_id", &self.id_map.max_id())?;
        st.serialize_field("items", &Items::<K, O, T>(&self.store))?;
        st.end()
    }
}

impl<'de, K, O, T> Deserialize<'de> for crate::OrdIdMap<K, O, T>
where
    K: Id + Deserialize<'de>,
    O: Collexetable<T> + Deserialize<'de>,
    T: FieldValue + jump_array::Key + Deserialize<'de> + ConstUnit,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Repr<K: Id, O> {
            max_id: u64,
            items: Vec<Pair<K, O>>,
        }

        let repr = Repr::<K, O>::deserialize(deserializer)?;

        let mut id_map = IdMap::<K, T>::with_id_capacity(repr.items.len());
        let mut store = JumpArray::with_capacity(repr.items.len());

        for pair in repr.items {
            let v = pair.1.collexate();
            id_map.insert_with_id(pair.0, v);
            store.insert(v, pair).ok();
        }
        id_map.bump_max_id_to(repr.max_id);
        store.rebuild();

        Ok(Self { id_map, store })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DefaultId;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct TestO(pub u32);

    pub type TestT = u32;

    impl Collexetable<TestT> for TestO {
        fn collexate(&self) -> TestT {
            self.0
        }
        fn collexate_ref(&self) -> &TestT {
            &self.0
        }
        fn collexate_mut(&mut self) -> &mut TestT {
            &mut self.0
        }
    }

    fn oid(v: u32) -> crate::OrdIdMap<DefaultId, TestO, TestT> {
        let mut m = crate::OrdIdMap::new();
        for i in 1..=v {
            m.insert(TestO(i * 10)).unwrap();
        }
        m.rebuild();
        m
    }

    /// 序列化形状 = 有序数组；反序列化后内容一致。
    #[test]
    fn test_obj_allocator_serde_consistency() {
        let elements = vec![
            Pair(DefaultId(1), TestO(10)),
            Pair(DefaultId(2), TestO(20)),
            Pair(DefaultId(3), TestO(30)),
        ];
        let mut m = crate::OrdIdMap::<DefaultId, TestO, TestT>::new();
        for pair in &elements {
            m.insert_with_id(pair.0, pair.1.clone()).unwrap();
        }
        m.rebuild();

        let json = serde_json::to_string(&m).expect("序列化失败");
        assert!(
            json.contains(r#""items":[[1,10],[2,20],[3,30]]"#),
            "有序数组形状，json={json}"
        );

        let back: crate::OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&json).expect("反序列化失败");
        let (id_map, store) = back.into_raw_parts();
        assert_eq!(
            store.into_sorted_vec(),
            elements,
            "存储内容（按序）应一致"
        );
        for obj in &elements {
            assert_eq!(
                id_map.inner.get(&obj.0.as_u64()),
                Some(&obj.1.collexate())
            );
        }
    }

    /// 删掉最大号 → 保存 → 重载 → 新号接着历史最大值，不复用。
    #[test]
    fn max_id_survives_delete_roundtrip() {
        let mut m = oid(3);
        let removed = m.remove(DefaultId(3)).expect("删除 id=3");
        assert_eq!(removed.0, 30);
        assert_eq!(m.len(), 2);
        let json = serde_json::to_string(&m).expect("序列化失败");
        let mut back: crate::OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(back.insert(TestO(999)).expect("发号应成功"), DefaultId(4));
    }

    /// 水位高于存活 id（中间号被删过），须精确还原。
    #[test]
    fn max_id_is_not_survivor_max() {
        let mut m = oid(5);
        m.remove(DefaultId(5));
        m.remove(DefaultId(4));
        let json = serde_json::to_string(&m).unwrap();
        let mut back: crate::OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.insert(TestO(1)).expect("发号应成功"), DefaultId(6));
    }

    /// 落盘水位偏低时以存活 id 兜底。
    #[test]
    fn low_persisted_max_id_is_clamped_up() {
        let m = oid(3);
        let json = serde_json::to_string(&m).unwrap();
        let broken = json.replacen(r#""max_id":3"#, r#""max_id":0"#, 1);
        let mut back: crate::OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&broken).expect("仍应可加载");
        assert_eq!(back.insert(TestO(1)).expect("发号应成功"), DefaultId(4));
    }

    /// 空集合。
    #[test]
    fn test_obj_allocator_serde_empty() {
        let original: crate::OrdIdMap<DefaultId, TestO, TestT> = crate::OrdIdMap::new();
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: crate::OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&json).unwrap();
        assert!(deserialized.store.is_empty());
        assert!(deserialized.id_map.inner.is_empty());
    }
}
