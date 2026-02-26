mod obj;

use std::ops::{Deref, DerefMut};
use field_collex::{Collexetable, FieldCollex, FieldValue};
use field_collex::collex::*;
use slotmap::{Key, SlotMap};
use span_core::Span;
use crate::obj::{Obj};

pub(crate) fn insert<K,E,V>(slot: &mut SlotMap<K,V>, elem: E) -> Obj<K,E>
where
    K: Key,
    E: Collexetable<V>,
    V: FieldValue,
{
    Obj(
        slot.insert(elem.collexate()),
        elem
    )
}

pub(crate) fn extend_from_vec<K,E,V>(slot: &mut SlotMap<K,V>, vec: Vec<E>) -> Vec<Obj<K,E>>
where
    K: Key,
    E: Collexetable<V>,
    V: FieldValue,
{
    let mut other: Vec<Obj<K,E>> = Vec::new();
    vec.into_iter().for_each(|e|
        {
            other.push(insert(slot, e))
        }
    );
    
    other
}

pub struct ObjAllocator<K,T,O>
where
    K: Key,
    O: Collexetable<T>,
    T: FieldValue,
{
    pub slot: SlotMap<K,T>,
    pub collex: FieldCollex<Obj<K,O>,T>
}

impl<K,V,E> Deref for ObjAllocator<K,V,E>
where
    K: Key,
    E: Collexetable<V>,
    V: FieldValue,
{
    type Target =  FieldCollex<Obj<K,E>,V>;
    fn deref(&self) -> &Self::Target {
        &self.collex
    }
}

impl<K,V,E> DerefMut for ObjAllocator<K,V,E>
where
    K: Key,
    E: Collexetable<V>,
    V: FieldValue,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.collex
    }
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
        vec: Vec<E>,
    ) -> Result<Self, WithElementsFieldCollexError<V>>
    {
        let mut slot = SlotMap::with_key();
        let other = extend_from_vec(&mut slot, vec);
        
        Ok(Self{
            slot,
            collex: FieldCollex::with_elements(span, unit, other)?,
        })
    }
    
    pub fn extend(&mut self, vec: Vec<E>) {
        let other = extend_from_vec(&mut self.slot, vec);
        self.collex.extend(other)
    }
    
    pub fn try_extend(&mut self, vec: Vec<E>) -> TryExtendResult<Obj<K, E>> {
        let other = extend_from_vec(&mut self.slot, vec);
        self.collex.try_extend(other)
    }
    
    pub fn insert(&mut self, elem: E) -> Result<K, InsertFieldCollexError<E>> {
        use InsertFieldCollexError::*;
        let obj = insert(&mut self.slot, elem);
        let key = obj.0;
        self.collex.insert(obj)
            .map(|_| key)
            .map_err(|err|
                {
                    self.slot.remove(key);
                    match err {
                        OutOfSpan(o) => { OutOfSpan(o.1) }
                        AlreadyExist(o) => { AlreadyExist(o.1) }
                    }
                }
            )
    }
    
    pub fn remove(&mut self, key: K) -> Option<E> {
        let v = self.slot.remove(key)?;
        
        Some(self.collex
            .remove(v)
            .unwrap()
            .1)
    }
    
    pub fn modify<F,R>(&mut self, key: K, f: F) -> Result<R, ModifyFieldCollexError<(R,E)>>
    where
        F: Fn(&mut E) -> R,
    {
        use ModifyFieldCollexError::*;
        let v = self.slot.get(key).ok_or(CannotFind)?;
        let (r,new_v) =
            self.collex
                .modify(*v,|e| (f(e),e.collexate()) )
                .map_err(|err|
                    err.map(|e| (e.0.0, e.1.1))
                )?;
        *self.slot.get_mut(key).unwrap() = new_v;
        Ok(r)
    }
    
    pub fn try_modify<F,R>(&mut self, key: K, f: F) -> Result<R, ModifyFieldCollexError<R>>
    where
        F: Fn(&mut E) -> R,
    {
        use ModifyFieldCollexError::*;
        let v = self.slot.get(key).ok_or(CannotFind)?;
        let (r,new_v) =
            self.collex
                .try_modify(*v, |e| (f(e),e.collexate()) )
                .map_err(|err|
                     err.map(|e| e.0)
                )?;
        *self.slot.get_mut(key).unwrap() = new_v;
        Ok(r)
    }
    
    pub fn get_with_key(&self, key: K) -> Option<&E> {
        let v = self.slot.get(key)?;
        self.collex.get(*v).map(|v| &v.1)
    }
}

