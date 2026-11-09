use field_collex::{Collexetable, ConstUnit, FieldValue};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use crate::{Id, IdMap, OrdIdMap};
use crate::pair::Pair;
use crate::store::Store;

/// 落盘形状：`{"max_id": N, "items": [[id, elem], ...]}`。
///
/// why 从裸数组改为具名结构：水位是发号状态而非派生量，必须跨存档保留。
/// 见 `IdMap` 的 Serialize 文档。
impl<K, O, T> Serialize for OrdIdMap<K, O, T>
where
    O: Collexetable<T> + Serialize,
    T: FieldValue + Serialize,
    K: Id + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("OrdIdMap", 2)?;
        st.serialize_field("max_id", &self.id_map.max_id())?;
        st.serialize_field("items", &self.store)?;
        st.end()
    }
}

impl<'de, K, O, T> Deserialize<'de> for OrdIdMap<K, O, T>
where
    O: Collexetable<T> + Deserialize<'de>,
    T: FieldValue + Deserialize<'de> + ConstUnit,
    K: Id + Deserialize<'de>,
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

        // 先灌存活元素：`insert_with_id` 只会抬高水位，不会压低，
        // 随后显式抬到落盘水位即可（不低于任何存活 id）。
        let mut id_map = IdMap::<K, T>::with_id_capacity(repr.items.len());
        let mut store = Store::new();

        for pair in repr.items {
            let obj_id = pair.0;
            let t_value = pair.1.collexate();
            id_map.insert_with_id(obj_id, t_value);
            store.insert(pair).ok();
        }
        id_map.bump_max_id_to(repr.max_id);

        Ok(Self { id_map, store })
    }
}


#[cfg(test)]
mod tests {
    use serde::Serialize;
    use super::*;
    use serde_json;
    use crate::DefaultId;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct TestO(pub u32);

    fn oid(v: u32) -> OrdIdMap<DefaultId, TestO, TestT> {
        let mut store = Store::new();
        let mut id_map = IdMap::<DefaultId, TestT>::with_id_capacity(0);
        for i in 1..=v {
            let pair = Pair(DefaultId(i as u64), TestO(i * 10));
            id_map.insert_with_id(pair.0, pair.1.collexate());
            store.insert(pair).ok();
        }
        OrdIdMap { id_map, store }
    }
    pub type TestT = u32;
    impl Collexetable<TestT> for TestO {
        fn collexate(&self) -> TestT { self.0 }
        fn collexate_ref(&self) -> &TestT { &self.0 }
        fn collexate_mut(&mut self) -> &mut TestT { &mut self.0 }
    }

    /// 核心功能：验证序列化/反序列化后数据一致性
    #[test]
    fn test_obj_allocator_serde_consistency() {
        let elements = vec![
            Pair(DefaultId(1), TestO(10)),
            Pair(DefaultId(2), TestO(20)),
            Pair(DefaultId(3), TestO(30)),
        ];

        // 构造有序存储
        let mut store = Store::new();
        for pair in &elements {
            store.insert(pair.clone()).ok();
        }

        // 构造 IdMap（手动插入与 elements 匹配的 Id/T）
        let mut id_map = IdMap::<DefaultId, TestT>::with_capacity(elements.len());
        for obj in &elements {
            id_map.insert_with_id(obj.0, obj.1.collexate());
        }
        let original = OrdIdMap { id_map, store };

        // 序列化
        let json = serde_json::to_string(&original).expect("序列化失败");
        println!("序列化结果：\n{}", json);

        // 反序列化
        let deserialized: OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json)
            .expect("反序列化失败");

        // 验证一致性
        let (id_map, store) = deserialized.into_raw_parts();
        assert_eq!(
            store.into_iter().collect::<Vec<Pair<DefaultId, TestO>>>(),
            elements
        );
        for obj in &elements {
            let id = obj.0;
            let expected_t = obj.1.collexate();
            assert_eq!(id_map.inner.get(&id.as_u64()), Some(&expected_t));
        }
        assert!(id_map.inner.capacity() >= elements.len());
    }

    /// 核心不变式：删掉最大号 → 保存 → 重载 → 新号必须接着历史最大值，
    /// 不得复用已删除的 id（否则指向旧 id 的残留引用会静默改指新对象）。
    /// 改动前水位只从存活元素推断，此处会发出撞车的 `DefaultId(3)`。
    #[test]
    fn max_id_survives_delete_roundtrip() {
        let mut m = oid(3); // ids 1,2,3，水位 3
        let removed = m.remove(DefaultId(3)).expect("删除 id=3");
        assert_eq!(removed.0, 30);
        assert_eq!(m.len(), 2, "只剩 1,2");

        let json = serde_json::to_string(&m).expect("序列化失败");
        let mut back: OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&json).expect("反序列化失败");

        let fresh = back.insert(TestO(999)).expect("发号应成功");
        assert_eq!(
            fresh,
            DefaultId(4),
            "水位须保持 3，新号应为 4 而非复用 3（json={json}）"
        );
    }

    /// 水位可以高于存活 id（中间号被删过），须精确还原而非取存活最大值。
    #[test]
    fn max_id_is_not_survivor_max() {
        let mut m = oid(5);
        m.remove(DefaultId(5));
        m.remove(DefaultId(4));
        let json = serde_json::to_string(&m).unwrap();
        let mut back: OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            back.insert(TestO(1)).expect("发号应成功"),
            DefaultId(6),
            "水位 5 → 新号 6"
        );
    }

    /// 落盘水位偏低（手工编辑/跨版本）时以存活 id 为准兜底，不发撞车 id。
    #[test]
    fn low_persisted_max_id_is_clamped_up() {
        let m = oid(3);
        let json = serde_json::to_string(&m).unwrap();
        // 把水位篡改成 0
        let broken = json.replacen(r#""max_id":3"#, r#""max_id":0"#, 1);
        let mut back: OrdIdMap<DefaultId, TestO, TestT> =
            serde_json::from_str(&broken).expect("仍应可加载");
        assert_eq!(
            back.insert(TestO(1)).expect("发号应成功"),
            DefaultId(4),
            "偏低水位须被存活最大值 3 兜底"
        );
    }

    /// 边界测试：空元素场景
    #[test]
    fn test_obj_allocator_serde_empty() {
        let store = Store::<Pair<DefaultId, TestO>, TestT>::new();
        let original: OrdIdMap<DefaultId, TestO, TestT> = OrdIdMap {
            id_map: IdMap::with_capacity(0),
            store,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json).unwrap();

        assert!(deserialized.store.is_empty());
        assert!(deserialized.id_map.inner.is_empty());
    }
}