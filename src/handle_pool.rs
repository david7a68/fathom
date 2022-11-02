use std::{hash::Hash, marker::PhantomData, mem::MaybeUninit};

/// A handle to an element in a `HandlePool`. Note that handles act like weak
/// references, so elements may be deleted while handles to it still exist. If
/// that happens, calls to `get()` and `get_mut()` will fail, and calling to
/// `remove()` will do nothing.
///
/// The generic argument `T` provides some basic type checking to reduce the
/// risk that a handle from one pool is used with another.
#[must_use]
pub struct Handle<T> {
    value: u32,
    phantom: PhantomData<T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value,
            phantom: PhantomData,
        }
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl<T> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(&format!("Handle<{}>", std::any::type_name::<T>()))
            .field("value", &self.value)
            .finish()
    }
}

#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawIndex(u32);

impl From<RawIndex> for usize {
    fn from(ri: RawIndex) -> Self {
        ri.0 as usize
    }
}

impl std::ops::Add<u32> for RawIndex {
    type Output = Self;
    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

/// Errors that may occur when working with [`HandlePool`]s.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("the handle is invalid")]
    InvalidHandle,
    /// An attempt was made to insert more than [`HandlePool::MAX_ELEMENTS`]
    /// elements into the pool.
    #[error("the pool has run out of slots")]
    TooManyObjects {
        /// The number of slots in active circulation.
        num_allocated: usize,
        /// The number of slots that have exhausted their generation indices and
        /// can no longer be used.
        num_retired: usize,
        /// The maximum number of slots that can be allocated at once.
        capacity: usize,
    },
    /// The pool has retired all of its slots. If you encounter this error,
    /// either reduce `MAX_ELEMENTS` or move to 64-bit handles (not yet
    /// implemented).
    #[error("the pool has retired all of its slots and can no longer service insertions")]
    Exhausted { capacity: usize },
}

/// NOTE(straivers): I chose to implement slots in this way instead of with two
/// separate arrays under the assumption that `get()` and `remove()` operations
/// are more likely than `contains()`. This _might_ cost a bit more memory if
/// `Value` is not 4-byte aligned, but that is countered by the benefit of not
/// having to look up two vectors (and possibly two cache misses).
///
/// The likelihood of cache misses decreases substantially if handles are
/// accesed in a loop; a possibility that may require a move to SOA form after
/// all with a bit of profiling. But a struct with two members is more
/// convenient, so that's what I did.
struct Slot<Value, KeyType> {
    /// The index and cycle count of the slot. The index is overloaded to serve
    /// two purposes: it marks the slot as in allocated when it points to
    /// itself, and marks itself as free (and the index of the next entry in the
    /// free list) when it points away from itself. The slot at the end of the
    /// free list will still point away from itself, so you need to refer to
    /// `HandlePool::num_free_slots` to determine the end of the list.
    index_and_cycles: Handle<KeyType>,

    /// Storage for a value.
    ///
    /// ## Safety
    ///
    /// A value is only present when the index points _away_ from the slot.
    value: MaybeUninit<Value>,
}

struct IndexAndCycles(u32);

impl<T> PartialEq<Handle<T>> for IndexAndCycles {
    fn eq(&self, other: &Handle<T>) -> bool {
        self.0 == other.value
    }
}

/// An object pool that uses opaque handles instead of pointers. The pool
/// behaves like a fixed-size slab allocator with the added benefit that every
/// single handle returned by the pool will be unique. These handles are weak
/// references to the identified data and may be safely invalidated at any time
/// by another copy of the handle. Attempting to call `get()` or `get_mut()` on
/// an invalidated handle simply returns `None`.
///
/// This is conceptually similar to a `HashMap<u32, T>` where the keys are
/// unique for the lifetime of the application with a few tradeoffs in favor of
/// improved performance:
///
///  - +Performance: Handles hold explicit references to the position of the
///    data they point to, eliminating the need for hashing or table probing.
///  - +Performance: Objects are densely packed in memory, reducing memory
///    consumption and avoiding the need to go to the allocator as frequently.
///  - -Flexibility: There is a maximum number of concurrent objects that can be
///    alive at any time determined by `MIN_ELEMENTS`.
///  - -Applicability: Each slot has a fixed number of `insert()/remove()`
///    cycles that it can support before it must be retired, making the
///    `HandlePool` unsuitable for applications where objects need to be
///    allocated and freed millions or billions or times.
///
/// This implementation also includes an idiosyncracy in the use of both `Value`
/// and `KeyType` generic arguments. The addition of the `KeyType` argument
/// permits library code to define a `Handle` type that is separate from any
/// implementation, as when defining runtime-selectable renderer backends for
/// example.
///
/// ## Slots
///
/// The precise number of slots available to a `HandlePool` is defined according
/// to the following formula:
///
/// ```text
/// // where MIN_ELEMENTS <= u32::MAX / 2
/// max_elements = 2 ^ ilog2(min(2, MIN_ELEMENTS)))
/// ```
///
/// and the cycle limit is defined as:
///
/// ```text
/// max_cycles = 2 ^ (u32::NUM_BITS - bits(max_elements))
/// ```
#[must_use]
pub struct HandlePool<Value, KeyType, const MIN_ELEMENTS: u32> {
    first_free_slot: RawIndex,

    num_free_slots: u32,

    /// For informational purposes only. We don't actually need to do anything
    /// with this except for when returning an error from `insert()`.
    num_retired_slots: u32,

    slots: Vec<Slot<Value, KeyType>>,
}

/// Workaround while `std::cmp::min` is not yet const.
const fn min_slots(min: u32) -> u32 {
    if min < 2 {
        1
    } else {
        min - 1
    }
}

impl<Value, KeyType, const MIN_ELEMENTS: u32> HandlePool<Value, KeyType, MIN_ELEMENTS> {
    /// The number of bits needed to store `MIN_ELEMENTS` indices.
    const INDEX_BITS: u32 = u32::BITS - min_slots(MIN_ELEMENTS).leading_zeros();

    /// A bitmask for the bits used to store the index.
    const INDEX_MASK: u32 = (1 << Self::INDEX_BITS) - 1;

    /// A bitmask for the bits used to store the cycle count.
    const CYCLE_MASK: u32 = !Self::INDEX_MASK;

    // Add one since `INDEX_MASK` starts at 0
    /// The maximum number of slots available to this pool.
    pub const MAX_ELEMENTS: usize = Self::INDEX_MASK as usize + 1;
    /// The maximum number of times a slot may be reused before it is
    /// permanently retired.
    pub const MAX_CYCLES: u32 = Self::CYCLE_MASK >> Self::INDEX_BITS;

    /// Preallocates the memory required to store `MAX_SLOTS` slots. Be careful
    /// when calling with large values of `MIN_ELEMENTS` as it may consume a lot of
    /// memory.
    pub fn preallocate() -> Self {
        Self {
            first_free_slot: RawIndex(0),
            num_free_slots: 0,
            num_retired_slots: 0,
            slots: Vec::with_capacity(Self::MAX_ELEMENTS),
        }
    }

    /// Preallocates the memory requried to store `min(initial_capacity,
    /// MAX_SLOTS)`. Be careful when calling with large values of
    /// `initial_capacity` and `MIN_ELEMENTS` as it may consume a lot of memory.
    pub fn preallocate_n(initial_capacity: usize) -> Self {
        Self {
            first_free_slot: RawIndex(0),
            num_free_slots: 0,
            num_retired_slots: 0,
            slots: Vec::with_capacity(std::cmp::min(Self::MAX_ELEMENTS, initial_capacity)),
        }
    }

    /// Checks if the handle pool has no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Retrieves the number of elements in the pool.
    #[must_use]
    pub fn count(&self) -> usize {
        self.slots.len() - (self.num_free_slots + self.num_retired_slots) as usize
    }

    /// Retrieves the number of slots that have been retired. Once this number
    /// reaches `MAX_SLOTS`, the pool becomes unusable and all attempts to call
    /// `insert()` will return `Error::OutOfIndices`.
    #[must_use]
    pub fn retired(&self) -> usize {
        self.num_retired_slots as usize
    }

    /// Retrieves the number of additonal elements that can be inserted into the
    /// pool before it must allocate additional memory.
    #[must_use]
    pub fn remaining_capacity(&self) -> usize {
        (self.slots.capacity() - self.slots.len()) + self.num_free_slots as usize
    }

    /// Checks if the handle is valid.
    #[must_use]
    pub fn contains(&self, handle: Handle<KeyType>) -> bool {
        if let Some(slot) = self.slots.get(usize::from(Self::index_of(handle))) {
            slot.index_and_cycles == handle
        } else {
            false
        }
    }

    /// Borrows a reference to the element identified by `handle` if it exists.
    pub fn get(&self, handle: Handle<KeyType>) -> Result<&Value, Error> {
        let slot = self
            .slots
            .get(usize::from(Self::index_of(handle)))
            .ok_or(Error::InvalidHandle)?;
        if slot.index_and_cycles == handle {
            Ok(unsafe { slot.value.assume_init_ref() })
        } else {
            Err(Error::InvalidHandle)
        }
    }

    /// Mutably borrows a reference to the element identified by `handle` if it
    /// exists.
    pub fn get_mut(&mut self, handle: Handle<KeyType>) -> Result<&mut Value, Error> {
        let slot = self
            .slots
            .get_mut(usize::from(Self::index_of(handle)))
            .ok_or(Error::InvalidHandle)?;
        if slot.index_and_cycles == handle {
            Ok(unsafe { slot.value.assume_init_mut() })
        } else {
            Err(Error::InvalidHandle)
        }
    }

    /// Inserts an element into the pool, returning a handle to that element.
    ///
    /// ## Errors
    ///
    /// Inserting a new value may fail if the pool has run out of slots. This
    /// becomes increasingly likely as handles are retired. See the
    /// documentation on [`HandlePool`] for how handles are retired.
    pub fn insert(&mut self, value: Value) -> Result<Handle<KeyType>, Error> {
        if self.num_free_slots > 0 {
            let slot_index = self.first_free_slot;

            let slot = &mut self.slots[usize::from(slot_index)];
            self.first_free_slot = Self::index_of(slot.index_and_cycles);
            Self::set_index(&mut slot.index_and_cycles, slot_index);
            slot.value = MaybeUninit::new(value);

            self.num_free_slots -= 1;

            Ok(slot.index_and_cycles)
        } else if self.slots.len() < Self::MAX_ELEMENTS {
            let slot_index = self.slots.len() as u32;
            let handle = Self::new_handle(slot_index);

            self.slots.push(Slot {
                index_and_cycles: handle,
                value: MaybeUninit::new(value),
            });

            Ok(handle)
        } else {
            Err(Error::TooManyObjects {
                num_allocated: self.slots.len(),
                num_retired: self.num_retired_slots as usize,
                capacity: Self::MAX_ELEMENTS,
            })
        }
    }

    /// Removes the element identified by `handle` from the pool if it exists and
    /// returns it to the caller.
    pub fn remove(&mut self, handle: Handle<KeyType>) -> Result<Value, Error> {
        let index = Self::index_of(handle);
        let slot = self
            .slots
            .get_mut(usize::from(index))
            .ok_or(Error::InvalidHandle)?;
        if slot.index_and_cycles == handle {
            let mut value = MaybeUninit::uninit();
            std::mem::swap(&mut value, &mut slot.value);

            if Self::is_saturated(slot.index_and_cycles) {
                self.num_retired_slots += 1;
            } else {
                Self::increment_cycle(&mut slot.index_and_cycles);
                Self::set_index(
                    &mut slot.index_and_cycles,
                    if self.first_free_slot == index {
                        index + 1
                    } else {
                        self.first_free_slot
                    },
                );
                self.first_free_slot = index;
                self.num_free_slots += 1;
            }

            // SAFETY: We have determined that the slot is valid and have
            // invalidated the handle.
            Ok(unsafe { value.assume_init() })
        } else {
            Err(Error::InvalidHandle)
        }
    }

    /// Removes the element identified by `handle` from the pool if it exists
    /// and the predicate `f` returns true.
    ///
    /// ## Errors
    ///
    /// Returns an [`Error::InvalidHandle`] if the handle is not valid.
    pub fn remove_if(
        &mut self,
        handle: Handle<KeyType>,
        f: impl Fn(&Value) -> bool,
    ) -> Result<Option<Value>, Error> {
        let index = Self::index_of(handle);
        let slot = self
            .slots
            .get_mut(usize::from(index))
            .ok_or(Error::InvalidHandle)?;
        if slot.index_and_cycles == handle {
            if f(unsafe { slot.value.assume_init_ref() }) {
                let mut value = MaybeUninit::uninit();
                std::mem::swap(&mut value, &mut slot.value);

                if Self::is_saturated(slot.index_and_cycles) {
                    self.num_retired_slots += 1;
                } else {
                    Self::increment_cycle(&mut slot.index_and_cycles);
                    Self::set_index(
                        &mut slot.index_and_cycles,
                        if self.first_free_slot == index {
                            index + 1
                        } else {
                            self.first_free_slot
                        },
                    );
                    self.first_free_slot = index;
                    self.num_free_slots += 1;
                }

                // SAFETY: We have determined that the slot is valid and have
                // invalidated the handle.
                Ok(Some(unsafe { value.assume_init() }))
            } else {
                Ok(None)
            }
        } else {
            Err(Error::InvalidHandle)
        }
    }

    #[inline]
    fn new_handle(index: u32) -> Handle<KeyType> {
        assert!(index < (1 << Self::INDEX_BITS));

        Handle {
            value: index,
            phantom: PhantomData,
        }
    }

    #[inline]
    fn index_of(handle: Handle<KeyType>) -> RawIndex {
        RawIndex(handle.value & Self::INDEX_MASK)
    }

    #[inline]
    fn generation_of(handle: Handle<KeyType>) -> u32 {
        (handle.value & Self::CYCLE_MASK) >> Self::INDEX_BITS
    }

    #[inline]
    fn is_saturated(handle: Handle<KeyType>) -> bool {
        (handle.value & Self::CYCLE_MASK) == Self::CYCLE_MASK
    }

    #[inline]
    fn split(handle: Handle<KeyType>) -> (RawIndex, u32) {
        (Self::index_of(handle), Self::generation_of(handle))
    }

    #[inline]
    fn set_index(handle: &mut Handle<KeyType>, index: RawIndex) {
        assert!(index.0 < (1 << Self::INDEX_BITS));
        handle.value = (handle.value & Self::CYCLE_MASK) | (index.0);
    }

    #[inline]
    fn increment_cycle(handle: &mut Handle<KeyType>) {
        debug_assert!(!Self::is_saturated(*handle));
        handle.value = (handle.value & Self::CYCLE_MASK).saturating_add(1 << Self::INDEX_BITS)
            | Self::index_of(*handle).0;
    }
}

impl<Value, KeyType, const MIN_ELEMENTS: u32> Default for HandlePool<Value, KeyType, MIN_ELEMENTS> {
    fn default() -> Self {
        Self {
            first_free_slot: RawIndex(0),
            num_free_slots: 0,
            num_retired_slots: 0,
            slots: vec![],
        }
    }
}

impl<Value, KeyType, const MIN_ELEMENTS: u32> Drop for HandlePool<Value, KeyType, MIN_ELEMENTS> {
    fn drop(&mut self) {
        for (i, mut slot) in self.slots.drain(..).enumerate() {
            let (index, generation) = Self::split(slot.index_and_cycles);

            if i == index.into() && generation < Self::MAX_CYCLES {
                // SAFETY: As per documentation on `Slot`, we have confirmed
                // that the slot's index points to itself.
                unsafe { slot.value.assume_init_drop() };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_invariants<Value, KeyType, const MIN_ELEMENTS: u32>(
        pool: &HandlePool<Value, KeyType, MIN_ELEMENTS>,
    ) {
        assert!(
            (pool.num_free_slots as usize + pool.num_retired_slots as usize) <= pool.slots.len()
        );

        if pool.num_free_slots > 0 {
            // check that the correct number of free slots are present
            let mut chain_length = 1;
            let mut current = pool.first_free_slot;

            while chain_length < pool.num_free_slots {
                let (index, generation) = HandlePool::<Value, KeyType, MIN_ELEMENTS>::split(
                    pool.slots[usize::from(current)].index_and_cycles,
                );
                assert_ne!(
                    index, current,
                    "free slots should never point to themselves"
                );
                assert!(
                    generation < HandlePool::<Value, KeyType, MIN_ELEMENTS>::MAX_CYCLES,
                    "free slots must not be have a saturated generation counter"
                );
                current = index;
                chain_length += 1;
            }

            assert_eq!(chain_length, pool.num_free_slots);
        }
    }

    #[test]
    fn handle_pool_sizing() {
        assert_eq!(HandlePool::<(), (), 0>::INDEX_BITS, 1);
        assert_eq!(HandlePool::<(), (), 10>::INDEX_BITS, 4);
        assert_eq!(HandlePool::<(), (), 16>::INDEX_BITS, 4);
        assert_eq!(HandlePool::<(), (), 1024>::INDEX_BITS, 10);
        assert_eq!(HandlePool::<(), (), { u32::MAX }>::INDEX_BITS, 32);
    }

    #[test]
    fn handle_pool() {
        let mut pool = HandlePool::<u128, (), { u32::MAX / 2 }>::default();
        pool.slots.reserve_exact(3);

        let a = pool.insert(100).unwrap();
        assert_eq!(pool.num_free_slots, 0);
        assert_eq!(pool.num_retired_slots, 0);

        // standard operations
        pool_invariants(&pool);
        assert!(!pool.is_empty());
        assert_eq!(pool.get(a), Ok(&100));
        assert_eq!(pool.count(), 1);
        assert!(pool.contains(a));
        assert_eq!(pool.remaining_capacity(), pool.slots.capacity() - 1);
        {
            let a_ = pool.get_mut(a).unwrap();
            *a_ = 200;
        }
        assert_eq!(pool.get(a), Ok(&200));

        {
            let a_ = pool.remove(a);
            assert_eq!(a_, Ok(200));
        }

        assert!(!pool.contains(a));
        assert!(pool.is_empty());
        assert_eq!(pool.num_free_slots, 1);
        assert_eq!(pool.num_retired_slots, 0);
        pool_invariants(&pool);

        // slot retirement
        let b = pool.insert(300).unwrap();
        assert_eq!(pool.num_free_slots, 0);
        assert_eq!(pool.num_retired_slots, 0);

        let _ = pool.remove(b);
        assert_eq!(pool.num_free_slots, 0);
        assert_eq!(pool.num_retired_slots, 1);
    }

    #[test]
    fn handle_pool_drop() {
        use std::{cell::Cell, rc::Rc};

        const COUNT: usize = 10_000;
        let half = COUNT / 2;

        let drop_counter = Rc::new(Cell::new(0));

        struct S {
            drop_counter: Rc<Cell<usize>>,
        }

        impl Drop for S {
            fn drop(&mut self) {
                self.drop_counter.set(self.drop_counter.get() + 1);
            }
        }

        let mut pool = HandlePool::<S, (), { COUNT as u32 }>::preallocate();
        let mut indices = Vec::with_capacity(COUNT);

        for _ in 0..COUNT {
            indices.push(
                pool.insert(S {
                    drop_counter: drop_counter.clone(),
                })
                .unwrap(),
            );
        }

        pool_invariants(&pool);

        for i in 0..half {
            assert!(pool.remove(indices[i * 2]).is_ok());
        }

        assert_eq!(drop_counter.get(), half);

        std::mem::drop(pool);

        assert_eq!(drop_counter.get(), COUNT);
    }
}
