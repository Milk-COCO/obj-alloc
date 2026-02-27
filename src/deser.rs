use field_collex::{Collexetable, FieldCollex, FieldValue};
use field_collex::collex::serialize::{FieldCollexSerdeHelper, FieldCollexSerdeWrapper};
use serde::{Deserialize, Deserializer};
use serde::de::Error;
use crate::{Id, IdMap, ObjAllocator};
use crate::obj::Obj;

impl<'de, K, T, O> Deserialize<'de> for ObjAllocator<K, T, O>
where
    O: Collexetable<T> + Deserialize<'de>,
    T: FieldValue + Deserialize<'de>,
    K: Id + Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // 核心优化1：直接复用 FieldCollexSerdeHelper，避免二次解析 FieldCollex
        // （原逻辑是先解析 FieldCollex，再从 FieldCollex 取元素；现在直接解析到 Helper，提前拿到结构化数据）
        let collex_helper: FieldCollexSerdeHelper<Obj<K,O>, T> = FieldCollexSerdeWrapper::<Obj<K,O>, T>::deserialize(deserializer)
            .map_err(|err|D::Error::custom(format!("反序列化 FieldCollexSerdeHelper 失败: {}", err)))?
            .into();
        
        // 核心优化2：利用 elements 的长度预分配 IdMap 容量，避免 HashMap 动态扩容（性能提升关键）
        let elements_len = collex_helper.elements.len();
        let mut id_map = IdMap::<K, T>::with_id_capacity(elements_len);
        
        // 核心优化3：直接遍历预解析的 Vec<E>，而非通过 FieldCollex 迭代器（减少迭代器开销）
        // 遍历过程中无额外内存分配，直接操作已有 Vec
        for obj in collex_helper.elements.iter() {
            let obj_id = obj.0; // 取出 Obj 中的 K（Id）
            let t_value = obj.1.collexate(); // 从 O 生成 T
            // 插入时无 HashMap 扩容开销（已预分配）
            id_map.insert_with_id(obj_id, t_value);
        }
        
        // 还原 FieldCollex（复用已解析的 span/unit/elements，无重复构造）
        let collex = FieldCollex::with_elements(collex_helper.span, collex_helper.unit, collex_helper.elements)
            .map_err(|e| D::Error::custom(format!("反序列化时创建 FieldCollex 失败: {}", e)))?;
        
        Ok(Self {
            id_map,
            collex,
        })
    }
}


#[cfg(test)]
mod tests {
    use field_collex::collex::serialize::{default_span, default_unit};
    use serde::Serialize;
    use super::*;
    use serde_json;
    use span_core::Span;
    use crate::DefaultId;
    
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct TestO(pub u32);
    pub type TestT = u32;
    impl Collexetable<TestT> for TestO {
        fn collexate(&self) -> TestT { self.0 }
        
        fn collexate_ref(&self) -> &TestT {
            &self.0
        }
        
        fn collexate_mut(&mut self) -> &mut TestT {
            &mut self.0
        }
    }
    
    // ============================ 2. 功能测试 ============================
    /// 核心功能：验证序列化/反序列化后数据一致性
    #[test]
    fn test_obj_allocator_serde_consistency() {
        // 步骤1：构造原始 ObjAllocator
        let span = Span::Finite(0u32..100u32);
        let unit = 10u32;
        let elements = vec![
            Obj(DefaultId(1), TestO(10)),
            Obj(DefaultId(2), TestO(20)),
            Obj(DefaultId(3), TestO(30)),
        ];
        // 构造 FieldCollex
        let collex = FieldCollex::with_elements(span.clone(), unit.clone(), elements.clone())
            .expect("构造 FieldCollex 失败");
        // 构造 IdMap（手动插入与 elements 匹配的 Id/T）
        let mut id_map = IdMap::<DefaultId, TestT>::with_capacity(elements.len());
        for obj in &elements {
            id_map.insert_with_id(obj.0, obj.1.collexate());
        }
        // 原始 ObjAllocator
        let original = ObjAllocator { id_map, collex };
        
        // 步骤2：序列化
        let json = serde_json::to_string(&original).expect("序列化失败");
        println!("序列化结果：\n{}", json);
        
        // 步骤3：反序列化
        let deserialized: ObjAllocator<DefaultId, TestT, TestO> = serde_json::from_str(&json)
            .expect("反序列化失败");
        
        // 步骤4：验证一致性
        let (id_map,collex) = deserialized.into_raw_parts();
        // 验证 collex 一致
        assert_eq!(collex.span().clone(), default_span());
        assert_eq!(collex.unit().clone(), default_unit::<u32>());
        assert_eq!(collex
                       .into_iter()
                       .collect::<Vec<Obj<DefaultId, TestO>>>(), elements);
        // 验证 id_map 一致（遍历所有 Id 检查值）
        for obj in &elements {
            let id = obj.0;
            let expected_t = obj.1.collexate();
            assert_eq!(id_map.inner.get(&id.as_u64()), Some(&expected_t));
        }
        // 验证 IdMap 容量（预分配生效）
        assert!(id_map.inner.capacity() >= elements.len());
    }
    
    /// 边界测试：空元素场景
    #[test]
    fn test_obj_allocator_serde_empty() {
        // 构造空元素的 FieldCollex
        let span = Span::new_finite(0u32, 50u32);
        let unit = 1032;
        let collex = FieldCollex::new(span, unit)
            .expect("构造空 FieldCollex 失败");
        let original: ObjAllocator<DefaultId, TestT, TestO> = ObjAllocator {
            id_map: IdMap::with_capacity(0),
            collex,
        };
        
        // 序列化 + 反序列化
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ObjAllocator<DefaultId, TestT, TestO> = serde_json::from_str(&json).unwrap();
        
        // 验证空
        assert!(deserialized.collex.is_empty());
        assert!(deserialized.id_map.inner.is_empty());
    }
}