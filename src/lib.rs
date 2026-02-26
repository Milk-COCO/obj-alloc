mod obj;

use field_collex::{Collexetable, FieldCollex, FieldValue};
use field_collex::collex::*;
use slotmap::{Key, SlotMap};
use span_core::Span;
use crate::obj::{Obj};

pub struct ObjAllocator<K,T,O>
where
    K: Key,
    O: Collexetable<T>,
    T: FieldValue,
{
    pub slot: SlotMap<K,T>,
    pub collex: FieldCollex<Obj<K,O>,T>
}


impl<K, V, E> ObjAllocator<K, V, E>
where
    K: Key,
    E: Collexetable<V>,
    V: FieldValue,
{
    pub fn new(span: Span<V>, unit: V) -> Result<Self, NewFieldCollexError<V>> {
        Ok(Self{
            slot: SlotMap::with_key(),
            collex: FieldCollex::new(span, unit)?,
        })
    }
    
    pub fn with_capacity(
        span: Span<V>,
        unit: V,
        capacity: usize,
    ) -> Result<Self, WithCapacityFieldCollexError<V>>
    {
        Ok(Self{
            slot: SlotMap::with_capacity_and_key(capacity),
            collex: FieldCollex::with_capacity(span, unit, capacity)?,
        })
    }
    
    pub fn with_elements(
        span: Span<V>,
        unit: V,
        other: Vec<E>,
    ) -> Result<Self, WithElementsFieldCollexError<V>>
    {
        let mut other: Vec<Obj<K,E>> = other.into_iter().map(|v| Obj(K::null(), v)).collect();
        let mut slot: SlotMap<K,V>  = SlotMap::with_key();
        other.iter_mut().for_each(|v|
            {
                slot.insert_with_key(|k|
                    {
                        v.0 = k;
                        v.1.collexate()
                    }
                );
            }
        );
        
        Ok(Self{
            slot,
            collex: FieldCollex::with_elements(span, unit, other)?,
        })
    }
}

