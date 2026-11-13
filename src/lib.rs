#![allow(dead_code)]
pub mod pair;
pub mod id_map;
pub mod deser;
pub mod ord_id_map;

pub use ord_id_map::*;
pub use id_map::*;
pub use pair::*;

/// 有序存储实现名（诊断/基准输出用）。
pub const STORE_NAME: &str = "JumpArray";
