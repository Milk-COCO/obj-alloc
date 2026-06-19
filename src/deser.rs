use field_collex::{Collexetable, ConstUnit, Collex, FieldValue};
use serde::{Deserialize, Deserializer};
use crate::{Id, IdMap, OrdIdMap};
use crate::pair::Pair;

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
        let elements: Vec<Pair<K, O>> = Vec::deserialize(deserializer)?;

        let mut id_map = IdMap::<K, T>::with_id_capacity(elements.len());
        let mut collex = Collex::new();

        for pair in elements {
            let obj_id = pair.0;
            let t_value = pair.1.collexate();
            id_map.insert_with_id(obj_id, t_value);
            collex.insert(pair).ok();
        }

        Ok(Self { id_map, collex })
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

        // 构造 Collex
        let mut collex = Collex::new();
        for pair in &elements {
            collex.insert(pair.clone()).ok();
        }

        // 构造 IdMap（手动插入与 elements 匹配的 Id/T）
        let mut id_map = IdMap::<DefaultId, TestT>::with_capacity(elements.len());
        for obj in &elements {
            id_map.insert_with_id(obj.0, obj.1.collexate());
        }
        let original = OrdIdMap { id_map, collex };

        // 序列化
        let json = serde_json::to_string(&original).expect("序列化失败");
        println!("序列化结果：\n{}", json);

        // 反序列化
        let deserialized: OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json)
            .expect("反序列化失败");

        // 验证一致性
        let (id_map, collex) = deserialized.into_raw_parts();
        assert_eq!(collex.unit(), &1u32);
        assert_eq!(
            collex.into_iter().collect::<Vec<Pair<DefaultId, TestO>>>(),
            elements
        );
        for obj in &elements {
            let id = obj.0;
            let expected_t = obj.1.collexate();
            assert_eq!(id_map.inner.get(&id.as_u64()), Some(&expected_t));
        }
        assert!(id_map.inner.capacity() >= elements.len());
    }

    /// 边界测试：空元素场景
    #[test]
    fn test_obj_allocator_serde_empty() {
        let collex = Collex::<Pair<DefaultId, TestO>, TestT>::new();
        let original: OrdIdMap<DefaultId, TestO, TestT> = OrdIdMap {
            id_map: IdMap::with_capacity(0),
            collex,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: OrdIdMap<DefaultId, TestO, TestT> = serde_json::from_str(&json).unwrap();

        assert!(deserialized.collex.is_empty());
        assert!(deserialized.id_map.inner.is_empty());
    }
}