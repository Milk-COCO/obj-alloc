//! 极简版 IdMap：自动生成递增 Id + Id 透明序列化 + 无条件编译
//! 核心特性：插入值自动返回递增 Id、Id 浅包装 u64、无任何条件编译

use core::fmt;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

// ============================ 核心 Id 定义 ============================
/// Id 基础 trait，所有自定义 Id 需实现此 trait
pub trait Id: Copy + Clone + Eq + PartialEq + fmt::Debug + Into<u64> + From<u64> {
    /// 快速转换为 u64
    fn as_u64(&self) -> u64 {
        (*self).into()
    }
    
    /// 从 u64 构建 Id
    fn from_u64(val: u64) -> Self {
        Self::from(val)
    }
}


// ============================ 自定义 Id 生成宏 ============================
/// 生成自定义 Id 类型的极简宏
#[macro_export]
macro_rules! new_id_type {
    // 递归终止条件：无剩余参数时结束
    () => {};

    // 核心匹配模式：单个 ID 结构体定义（带可选 vis + 属性 + 名称）
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident;
        $($rest:tt)*
    ) => {
        // 生成单个 ID 结构体的完整定义
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis struct $name(pub u64);

        impl From<u64> for $name {
            #[inline]
            fn from(val: u64) -> Self {
                Self(val)
            }
        }

        impl From<$name> for u64 {
            #[inline]
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl $crate::Id for $name {}

        impl serde::Serialize for $name {
            #[inline]
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.0.serialize(serializer)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            #[inline]
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let val = u64::deserialize(deserializer)?;
                Ok(Self(val))
            }
        }

        $crate::new_id_type!($($rest)*);
    };
}

new_id_type!{
    pub struct DefaultId;
}


// ============================ IdMap 核心实现（自动生成递增 Id） ============================
/// 极简版 IdMap：自动生成递增 Id + HashMap 存储 + 无条件编译
#[derive(Debug, Clone)]
pub struct IdMap<K: Id, V> {
    pub(crate) inner: HashMap<u64, V>, // 底层存储：u64 -> V
    max_id: u64,            // 记录最大 Id，用于生成递增 Id
    _marker: PhantomData<K>,
}

impl<V> IdMap<DefaultId, V> {
    /// 创建空的 IdMap（初始 max_id = 0）
    pub fn new() -> Self { Self::with_id_capacity(0) }
    
    /// 创建指定初始容量的 IdMap
    pub fn with_capacity(capacity: usize) -> Self { Self::with_id_capacity(capacity) }
}

impl<K: Id, V> IdMap<K, V> {
    /// 为自定义 Id 类型创建空 IdMap
    pub fn with_id() -> Self {
        Self {
            inner: HashMap::new(),
            max_id: 0,
            _marker: PhantomData,
        }
    }
    
    /// 自定义 Id 类型创建指定初始容量的 IdMap
    pub fn with_id_capacity(capacity: usize) -> Self {
        Self {
            inner: HashMap::with_capacity(capacity),
            max_id: 0,
            _marker: PhantomData,
        }
    }
    
    /// 插入值，自动生成递增 Id 并返回
    pub fn insert(&mut self, value: V) -> K {
        self.max_id += 1; // 递增生成新 Id（从 1 开始，避免 0 作为初始值）
        let id_u64 = self.max_id;
        self.inner.insert(id_u64, value); // 存储值
        K::from_u64(id_u64) // 转换为指定 Id 类型并返回
    }
    
    
    /// 【手动指定 Id】插入键值对，返回旧值（若存在）
    ///
    /// 注意：若手动传入的 Id 大于当前 max_id，会更新 max_id 以保证自动生成的 Id 不重复
    pub fn insert_with_id(&mut self, id: K, value: V) -> Option<V> {
        let id_u64 = id.as_u64();
        // 若手动传入的 Id 更大，更新 max_id，避免自动生成 Id 重复
        if id_u64 > self.max_id {
            self.max_id = id_u64;
        }
        self.inner.insert(id_u64, value)
    }
    
    /// 从 Vec<V> 批量插入值，自动生成递增 Id，返回对应的 Id 列表
    /// 生成的 Id 从当前 max_id + 1 开始连续递增
    pub fn from_vec(values: Vec<V>) -> (Self, Vec<K>) {
        let mut map = Self {
            inner: HashMap::with_capacity(values.len()),
            max_id: 0,
            _marker: PhantomData,
        };
        let ids = values
            .into_iter()
            .map(|val| {
                map.max_id += 1;
                let id_u64 = map.max_id;
                map.inner.insert(id_u64, val);
                K::from_u64(id_u64)
            })
            .collect();
        (map, ids)
    }
    
    /// 循环插入：先生成递增 Id，再通过闭包（Id → V）生成值并插入
    /// 适用于值需要依赖自身 Id 的场景（如循环引用/关联 Id 的场景）
    pub fn insert_cyclic<F>(&mut self, f: F) -> K
    where
        F: FnOnce(K) -> V,
    {
        self.max_id += 1;
        let new_id = K::from_u64(self.max_id);
        let value = f(new_id);
        self.inner.insert(self.max_id, value);
        new_id
    }
    
    /// 根据 Id 查询值
    pub fn get(&self, id: K) -> Option<&V> {
        self.inner.get(&id.as_u64())
    }
    
    /// 根据 Id 查询可变值
    pub fn get_mut(&mut self, id: K) -> Option<&mut V> {
        self.inner.get_mut(&id.as_u64())
    }
    
    /// 根据 Id 删除值
    pub fn remove(&mut self, id: K) -> Option<V> {
        self.inner.remove(&id.as_u64())
    }
    
    /// 判断是否包含指定 Id
    pub fn contains_id(&self, id: K) -> bool {
        self.inner.contains_key(&id.as_u64())
    }
    
    /// 获取当前最大 Id（仅用于参考，删除 Id 后不会回退）
    pub fn max_id(&self) -> K {
        K::from_u64(self.max_id)
    }
    
    /// 获取元素数量
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    
    /// 判断是否为空
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    
    /// 清空所有元素（保留 max_id 不变，避免 Id 重复）
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

// ============================ Index/IndexMut 实现 ============================
impl<K: Id, V> Index<K> for IdMap<K, V> {
    type Output = V;
    
    fn index(&self, id: K) -> &Self::Output {
        self.get(id).expect("invalid IdMap id")
    }
}

impl<K: Id, V> IndexMut<K> for IdMap<K, V> {
    fn index_mut(&mut self, id: K) -> &mut Self::Output {
        self.get_mut(id).expect("invalid IdMap id")
    }
}

// ============================ 测试用例 ============================
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;
    
    // 测试默认 Id + 自动递增生成
    #[test]
    fn test_default_id_auto_generate() {
        let mut map = IdMap::new();
        
        // 插入值，自动返回递增 Id
        let id1 = map.insert("hello");
        let id2 = map.insert("world");
        let id3 = map.insert("rust");
        
        // 验证 Id 递增（从 1 开始）
        assert_eq!(id1, DefaultId(1));
        assert_eq!(id2, DefaultId(2));
        assert_eq!(id3, DefaultId(3));
        
        // 验证值查询
        assert_eq!(map.get(id1), Some(&"hello"));
        assert_eq!(map[id2], "world");
        assert_eq!(map.max_id(), DefaultId(3));
        
        // 删除值后，max_id 不回退
        map.remove(id2);
        assert_eq!(map.max_id(), DefaultId(3));
        let id4 = map.insert("new value");
        assert_eq!(id4, DefaultId(4)); // 继续递增
        
        // 数量/空判断
        assert_eq!(map.len(), 3);
        map.clear();
        assert!(map.is_empty());
    }
    
    // 测试自定义 Id
    new_id_type! {
        struct MyId;
    }
    
    #[test]
    fn test_custom_id() {
        let mut map = IdMap::<MyId, u32>::with_id();
        
        let id1 = map.insert(42);
        let id2 = map.insert(100);
        
        assert_eq!(id1, MyId(1));
        assert_eq!(id2, MyId(2));
        assert_eq!(map.get(id1), Some(&42));
        
        // 删除测试
        map.remove(id1);
        assert!(!map.contains_id(id1));
    }
    
    // 测试 Id 透明序列化
    #[test]
    fn test_id_serde() {
        // 测试默认 Id
        let id = DefaultId(123456789);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "123456789"); // 直接输出 u64 字符串
        let id2: DefaultId = serde_json::from_str(&json).unwrap();
        assert_eq!(id2, id);
        
        // 测试自定义 Id
        let my_id = MyId(987654321);
        let json = serde_json::to_string(&my_id).unwrap();
        let my_id2: MyId = serde_json::from_str(&json).unwrap();
        assert_eq!(my_id2, my_id);
    }
}