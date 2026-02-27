pub mod obj;
pub mod id_map;
pub mod deser;

pub use id_map::*;
pub use obj::*;

use std::ops::{Deref, DerefMut};
use field_collex::{Collexetable, FieldCollex, FieldValue};
use field_collex::collex::*;
use span_core::Span;

pub(crate) fn insert<K,E,V>(id_map: &mut IdMap<K,V>, elem: E) -> Obj<K,E>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    Obj(
        id_map.insert(elem.collexate()),
        elem
    )
}

pub(crate) fn extend_from_vec<K,E,V>(id_map: &mut IdMap<K,V>, vec: Vec<E>) -> Vec<Obj<K,E>>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    let mut other: Vec<Obj<K,E>> = Vec::new();
    vec.into_iter().for_each(|e|
        {
            other.push(insert(id_map, e))
        }
    );
    
    other
}

#[derive(Debug)]
#[derive(serde::Serialize)]
#[serde(transparent)]
pub struct ObjAllocator<K,T,O>
where
    K: Id,
    O: Collexetable<T>,
    T: FieldValue,
{
    #[serde(skip)]
    pub id_map: IdMap<K,T>,
    pub collex: FieldCollex<Obj<K,O>,T>
}

impl<K,V,E> Deref for ObjAllocator<K,V,E>
where
    K: Id,
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
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.collex
    }
}


impl<K, V, E> ObjAllocator<K, V, E>
where
    K: Id,
    E: Collexetable<V>,
    V: FieldValue,
{
    pub fn new(span: Span<V>, unit: V) -> Result<Self, NewFieldCollexError<V>> {
        Ok(Self{
            id_map: IdMap::with_id(),
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
            id_map: IdMap::with_id_capacity(capacity),
            collex: FieldCollex::with_capacity(span, unit, capacity)?,
        })
    }
    
    pub fn with_elements(
        span: Span<V>,
        unit: V,
        vec: Vec<E>,
    ) -> Result<Self, WithElementsFieldCollexError<V>>
    {
        let mut id_map = IdMap::with_id();
        let other = extend_from_vec(&mut id_map, vec);
        
        Ok(Self{
            id_map,
            collex: FieldCollex::with_elements(span, unit, other)?,
        })
    }
    
    pub fn extend(&mut self, vec: Vec<E>) {
        let other = extend_from_vec(&mut self.id_map, vec);
        self.collex.extend(other)
    }
    
    pub fn try_extend(&mut self, vec: Vec<E>) -> TryExtendResult<Obj<K, E>> {
        let other = extend_from_vec(&mut self.id_map, vec);
        self.collex.try_extend(other)
    }
    
    pub fn insert(&mut self, elem: E) -> Result<K, InsertFieldCollexError<E>> {
        use InsertFieldCollexError::*;
        let obj = insert(&mut self.id_map, elem);
        let id = obj.0;
        self.collex.insert(obj)
            .map(|_| id)
            .map_err(|err|
                {
                    self.id_map.remove(id);
                    match err {
                        OutOfSpan(o) => { OutOfSpan(o.1) }
                        AlreadyExist(o) => { AlreadyExist(o.1) }
                    }
                }
            )
    }
    
    pub fn remove(&mut self, id: K) -> Option<E> {
        let v = self.id_map.remove(id)?;
        
        Some(self.collex
            .remove(v)
            .unwrap()
            .1)
    }
    
    pub fn modify<F,R>(&mut self, id: K, f: F) -> Result<R, ModifyFieldCollexError<(R,E)>>
    where
        F: Fn(&mut E) -> R,
    {
        use ModifyFieldCollexError::*;
        let v = self.id_map.get(id).ok_or(CannotFind)?;
        let (r,new_v) =
            self.collex
                .modify(*v,|e| (f(e),e.collexate()) )
                .map_err(|err|
                    err.map(|e| (e.0.0, e.1.1))
                )?;
        *self.id_map.get_mut(id).unwrap() = new_v;
        Ok(r)
    }
    
    pub fn try_modify<F,R>(&mut self, id: K, f: F) -> Result<R, ModifyFieldCollexError<R>>
    where
        F: Fn(&mut E) -> R,
    {
        use ModifyFieldCollexError::*;
        let v = self.id_map.get(id).ok_or(CannotFind)?;
        let (r,new_v) =
            self.collex
                .try_modify(*v, |e| (f(e),e.collexate()) )
                .map_err(|err|
                     err.map(|e| e.0)
                )?;
        *self.id_map.get_mut(id).unwrap() = new_v;
        Ok(r)
    }
    
    pub fn get_with_id(&self, id: K) -> Option<&E> {
        let v = self.id_map.get(id)?;
        self.collex.get(*v).map(|v| &v.1)
    }
    
    pub fn into_raw_parts(self) -> (IdMap<K,V>, FieldCollex<Obj<K,E>,V>) {
        (self.id_map,self.collex)
    }
    
    pub fn from_raw_parts(id_map: IdMap<K,V>, collex: FieldCollex<Obj<K,E>,V>) -> Self {
        Self {
            id_map, collex
        }
    }
}


