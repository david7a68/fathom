use std::{marker::PhantomData, mem::MaybeUninit};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawIndex {
    pub index: u32,
    generation: u32,
}

#[derive(Copy, Debug, Eq)]
pub struct Index<T> {
    pub index: u32,
    generation: u32,
    phantom_data: PhantomData<T>,
}

impl<T> Clone for Index<T> {
    fn clone(&self) -> Self {
        Self {
            index: self.index,
            generation: self.generation,
            phantom_data: PhantomData,
        }
    }
}

impl<T> Index<T> {
    pub fn null() -> Self {
        Self {
            index: 0,
            generation: 0,
            phantom_data: PhantomData,
        }
    }

    pub fn is_null(&self) -> bool {
        self.index == 0 && self.generation == 0
    }

    pub fn as_raw(&self) -> RawIndex {
        RawIndex {
            index: self.index,
            generation: self.generation,
        }
    }

    /// # Safety
    ///
    /// The caller must ensure that the index refers to an object of the same
    /// type.
    pub unsafe fn from_raw(raw: RawIndex) -> Self {
        Self {
            index: raw.index,
            generation: raw.generation,
            phantom_data: PhantomData,
        }
    }
}

impl<T> Default for Index<T> {
    fn default() -> Self {
        Self::null()
    }
}

impl<T> PartialEq for Index<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

/// Derives a newtype of [`Index`](crate::indexed_store::Index) to permit the
/// use of internal types as the discriminant without exposing them to the user
/// and to offer a simple way of further distinguishing between different
/// [`IndexedStore`](crate::indexed_store::IndexedStore)s.
///
/// The derived implements `Clone`, `Copy`, `Debug`, `Default`, and `PartialEq`,
/// as well as `From<Index<T>> for NewType` and `From<NewType> for Index<T>` for
/// convenience.
macro_rules! newtype_index {
    ($name: ident, $discriminant: ty) => {
        pub struct $name(
            crate::indexed_store::RawIndex,
            std::marker::PhantomData<$discriminant>,
        );

        impl Clone for $name {
            fn clone(&self) -> Self {
                Self(self.0.clone(), std::marker::PhantomData)
            }
        }

        impl Copy for $name {}

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_tuple("$name").field(&self.0).finish()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self(
                    crate::indexed_store::RawIndex::default(),
                    std::marker::PhantomData,
                )
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl $name {
            pub fn index(&self) -> u32 {
                self.0.index
            }
        }

        impl From<crate::indexed_store::Index<$discriminant>> for $name {
            /// Converts from an `Index` into a newtype for use.
            fn from(index: crate::indexed_store::Index<$discriminant>) -> Self {
                Self(index.as_raw(), std::marker::PhantomData)
            }
        }

        impl From<$name> for crate::indexed_store::Index<$discriminant> {
            /// Converts from a newtype to the `Index` base type.
            fn from(index: $name) -> Self {
                unsafe { Self::from_raw(index.0) }
            }
        }
    };
}

pub(crate) use newtype_index;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the store has run out of concurrent indices (max u32::MAX)")]
    OutOfIndices,
}

/// An `IndexedStore` holds a mapping from indices to values such that
/// individual values such that insert, get, and remove operations are all O(1),
/// with performance comparable to indexing into an array. Furthermore, each
/// index is unique an will not be reused, allowing for trivial tests for
/// validity.
///
/// # Implementation Notes
///
/// - Indices are tracked as (Index, Generation) pairs where each is 32-bits in
/// size. This permits a maximum of `u32::MAX` concurrent allocations and at
/// most `u32::MAX - 1` total allocations. For simplicity, the tuple (0, 0) is
/// reserved for a `null` value.
/// - The current implementation is not thread-safe and does not guarantee fixed
///   pointers for values.
#[derive(Debug)]
pub struct IndexedStore<T> {
    free_indices: Vec<u32>,
    generations: Vec<u32>,
    values: Vec<MaybeUninit<T>>,
}

impl<T> Default for IndexedStore<T> {
    fn default() -> Self {
        Self {
            free_indices: vec![],
            generations: vec![],
            values: vec![],
        }
    }
}

impl<T> IndexedStore<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks that the given index refers to a value.
    pub fn is_valid(&self, index: impl Into<Index<T>>) -> bool {
        self.validate_invariants();

        let index = index.into();
        if let Some(slot_generation) = self.generations.get(index.index as usize) {
            *slot_generation == index.generation
        } else {
            false
        }
    }

    pub fn is_empty(&self) -> bool {
        self.free_indices.len() == self.values.len()
    }

    /// Inserts a new value into the store.
    pub fn insert(&mut self, value: T) -> Result<Index<T>, Error> {
        self.validate_invariants();

        if self.values.is_empty() {
            self.values.push(MaybeUninit::new(value));
            self.generations.push(1);

            Ok(Index {
                index: 0,
                generation: 1,
                phantom_data: PhantomData,
            })
        } else if let Some(index) = self.free_indices.pop() {
            self.values[index as usize] = MaybeUninit::new(value);

            Ok(Index {
                index,
                generation: self.generations[index as usize],
                phantom_data: PhantomData,
            })
        } else {
            let index = self
                .values
                .len()
                .try_into()
                .map_err(|_| Error::OutOfIndices)?;

            self.values.push(MaybeUninit::new(value));
            self.generations.push(0);

            Ok(Index {
                index,
                generation: 0,
                phantom_data: PhantomData,
            })
        }
    }

    pub fn get(&self, index: impl Into<Index<T>>) -> Option<&T> {
        self.validate_invariants();

        let index = index.into();
        if let Some(slot_generation) = self.generations.get(index.index as usize) {
            if *slot_generation == index.generation {
                let value = self.values.get(index.index as usize)?;
                return Some(unsafe { value.assume_init_ref() });
            }
        }
        None
    }

    pub fn get_mut(&mut self, index: impl Into<Index<T>>) -> Option<&mut T> {
        self.validate_invariants();

        let index = index.into();
        if let Some(slot_generation) = self.generations.get_mut(index.index as usize) {
            if *slot_generation == index.generation {
                let value = self.values.get_mut(index.index as usize)?;
                return Some(unsafe { value.assume_init_mut() });
            }
        }
        None
    }

    pub fn remove(&mut self, index: impl Into<Index<T>>) -> Option<T> {
        self.validate_invariants();

        let index = index.into();
        if let Some(slot_generation) = self.generations.get_mut(index.index as usize) {
            if *slot_generation == index.generation {
                let mut value_swap = MaybeUninit::uninit();
                std::mem::swap(&mut value_swap, &mut self.values[index.index as usize]);

                *slot_generation += 1;
                self.free_indices.push(index.index);

                return Some(unsafe { value_swap.assume_init() });
            }
        }
        None
    }

    fn validate_invariants(&self) {
        debug_assert_eq!(
            self.values.len(),
            self.generations.len(),
            "IndexedStore invariant; values and generations are out of sync"
        );
    }
}

impl<T> Drop for IndexedStore<T> {
    /// Drops all currently initialized values in the store and frees its
    /// backing memory.
    ///
    /// Note: Some values may not be dropped if a destructor panics.
    fn drop(&mut self) {
        self.free_indices.sort_unstable();

        for index in (0..self.values.len()).rev() {
            if let Some(last) = self.free_indices.last() {
                if *last == index as u32 {
                    self.free_indices.pop();
                    continue;
                }
            } else {
                // SAFETY: This is safe because we're iterating solely through
                // indices within self.values
                unsafe {
                    self.values
                        .get_unchecked_mut(index as usize)
                        .assume_init_drop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    #[test]
    fn init() {
        let store = IndexedStore::<u32>::new();
        assert_eq!(store.values.len(), 0);
        assert_eq!(store.generations.len(), 0);
        assert_eq!(store.free_indices.len(), 0);
    }

    #[test]
    fn alloc_valid_get() {
        let mut store = IndexedStore::<u32>::new();

        let index_1 = store.insert(0).unwrap();
        assert_eq!(store.values.len(), 1);
        assert_eq!(store.generations.len(), 1);
        assert_eq!(store.free_indices.len(), 0);
        assert!(store.is_valid(index_1));
        assert_eq!(store.get(index_1), Some(&0));

        assert_eq!(index_1.index, 0);
        assert_eq!(index_1.generation, 1);

        let index_2 = store.insert(1).unwrap();
        assert_eq!(store.values.len(), 2);
        assert_eq!(store.generations.len(), 2);
        assert_eq!(store.free_indices.len(), 0);
        assert!(store.is_valid(index_2));
        assert_eq!(store.get(index_2), Some(&1));

        assert_eq!(index_2.index, 1);
        assert_eq!(index_2.generation, 0);

        let index_3 = Index {
            index: 0,
            generation: 0,
            phantom_data: PhantomData,
        };
        assert_eq!(store.get(index_3), None);

        let index_4 = Index {
            index: 0,
            generation: 2,
            phantom_data: PhantomData,
        };
        assert_eq!(store.get(index_4), None);

        let index_5 = Index {
            index: 10,
            generation: 0,
            phantom_data: PhantomData,
        };
        assert_eq!(store.get(index_5), None);
    }

    #[test]
    fn alloc_remove() {
        let mut store = IndexedStore::<u32>::new();

        let index_1 = store.insert(0).unwrap();
        assert_eq!(store.remove(index_1), Some(0));
        assert_eq!(&store.free_indices, &[0]);

        let index_2 = store.insert(1).unwrap();
        assert_eq!(index_2.index, 0);
        assert_eq!(index_2.generation, 2);
        assert_eq!(store.remove(index_2), Some(1));
    }

    #[test]
    fn drop() {
        struct T(Rc<RefCell<bool>>);

        impl Drop for T {
            fn drop(&mut self) {
                *self.0.borrow_mut() = true;
            }
        }

        let mut store = IndexedStore::new();

        let dropped = Rc::new(RefCell::new(false));

        {
            // pad
            let a = store.insert(T(dropped.clone())).unwrap();
            let b = store.insert(T(dropped.clone())).unwrap();
            let c = store.insert(T(dropped.clone())).unwrap();
            let d = store.insert(T(dropped.clone())).unwrap();

            store.remove(a);
            store.remove(b);
            store.remove(c);
            store.remove(d);
        }

        let _ = store.insert(T(dropped.clone())).unwrap();

        {
            let a = store.insert(T(dropped.clone())).unwrap();
            let b = store.insert(T(dropped.clone())).unwrap();
            let c = store.insert(T(dropped.clone())).unwrap();
            let d = store.insert(T(dropped.clone())).unwrap();

            store.remove(a);
            store.remove(b);
            store.remove(c);
            store.remove(d);
        }

        std::mem::drop(store);

        assert!(*dropped.borrow());
    }
}
