use std::ops::{Deref, DerefMut};
use field_collex::{Collexetable, FieldValue};
use slotmap::Key;

pub struct Obj<K,O>(pub K, pub O)
where
    K: Key,
;

impl<K: Key,E> Deref for Obj<K,E>{
    type Target = E;
    fn deref(&self) -> &Self::Target {
        &self.1
    }
}

impl<K: Key,E> DerefMut for Obj<K,E>{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.1
    }
}

impl<K,E,V> Collexetable<V> for Obj<K,E>
where
    K: Key,
    E: Collexetable<V>,
{
    fn collexate(&self) -> V {
        self.1.collexate()
    }
    
    fn collexate_ref(&self) -> &V {
        self.1.collexate_ref()
    }
    
    fn collexate_mut(&mut self) -> &mut V {
        self.1.collexate_mut()
    }
}
