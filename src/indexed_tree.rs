use std::cell::Cell;

use crate::indexed_store::IndexedStore;

#[repr(transparent)]
#[derive(Debug, Eq)]
pub struct Index<T>(crate::indexed_store::Index<T>);

impl<T> From<crate::indexed_store::Index<T>> for Index<T> {
    fn from(index: crate::indexed_store::Index<T>) -> Self {
        Index(index)
    }
}

impl<T> Clone for Index<T> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<T> Copy for Index<T> {}

impl<T> Default for Index<T> {
    fn default() -> Self {
        Self(crate::indexed_store::Index::default())
    }
}

impl<T> PartialEq for Index<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Index<T> {
    pub fn index(&self) -> u32 {
        self.0.index
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the store has run out of concurrent indices (max u32::MAX)")]
    OutOfIndices,
    #[error("index does not refer to a value")]
    InvalidIndex,
    #[error("the root has already been set")]
    RootAlreadySet,
    #[error("parent cannot be child of itself")]
    ParentIsChild,
}

#[derive(Debug)]
struct Node<T> {
    next: Cell<Index<T>>,
    prev: Cell<Index<T>>,
    parent: Cell<Index<T>>,
    first_child: Cell<Index<T>>,
    value: T,
}

#[derive(Debug)]
pub struct IndexedTree<T> {
    root: Index<T>,
    store: IndexedStore<Node<T>>,
}

impl<T> Default for IndexedTree<T> {
    fn default() -> Self {
        Self {
            root: Index::default(),
            store: IndexedStore::default(),
        }
    }
}

impl<T> IndexedTree<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn is_ancestor(&self, ancestor: Index<T>, descendant: Index<T>) -> bool {
        if ancestor == descendant {
            return true;
        }

        let mut current = self.store.get(ancestor.0).unwrap().first_child.get();
        while current != Index::default() {
            if self.is_ancestor(current, descendant) {
                return true;
            } else {
                current = self.store.get(current.0).unwrap().next.get();
            }
        }

        false
    }

    pub fn get(&self, node_id: Index<T>) -> Option<&T> {
        self.store.get(node_id.0).map(|node| &node.value)
    }

    pub fn get_mut(&mut self, node_id: Index<T>) -> Option<&mut T> {
        self.store.get_mut(node_id.0).map(|node| &mut node.value)
    }

    pub fn root(&self) -> Option<&T> {
        self.store.get(self.root.0).map(|node| &node.value)
    }

    pub fn root_id(&self) -> Option<Index<T>> {
        if self.root == Index::default() {
            None
        } else {
            Some(self.root)
        }
    }

    pub fn children(&self, parent_id: Index<T>) -> impl Iterator<Item = &T> {
        struct Iter<'a, T> {
            store: &'a IndexedStore<Node<T>>,
            current: Index<T>,
        }

        impl<'a, T> Iterator for Iter<'a, T> {
            type Item = &'a T;

            fn next(&mut self) -> Option<Self::Item> {
                if self.current == Index::default() {
                    return None;
                }

                // We should never encounter an invalid node index within the
                // tree.
                let node = self
                    .store
                    .get(self.current.0)
                    .expect("invalid internal node index");
                self.current = node.next.get();
                Some(&node.value)
            }
        }

        let current = if let Some(node) = self.store.get(parent_id.0) {
            node.first_child.get()
        } else {
            Index::default()
        };

        Iter {
            store: &self.store,
            current,
        }
    }

    pub fn children_ids(&self, parent_id: Index<T>) -> IndexIter<T> {
        let current = if let Some(node) = self.store.get(parent_id.0) {
            node.first_child.get()
        } else {
            Index::default()
        };

        IndexIter {
            store: &self.store,
            current,
        }
    }

    pub fn new_node(&mut self, value: T) -> Result<Index<T>, Error> {
        let node = Node {
            next: Cell::default(),
            prev: Cell::default(),
            parent: Cell::default(),
            first_child: Cell::default(),
            value,
        };

        match self.store.insert(node) {
            Ok(index) => Ok(Index(index)),
            Err(e) => match e {
                crate::indexed_store::Error::OutOfIndices => Err(Error::OutOfIndices),
            },
        }
    }

    pub fn set_root(&mut self, node_id: Index<T>) -> Result<(), Error> {
        if self.root != Index::default() {
            return Err(Error::RootAlreadySet);
        }
        self.root = node_id;
        Ok(())
    }

    pub fn add_child(&mut self, parent_id: Index<T>, child_id: Index<T>) -> Result<(), Error> {
        if child_id == parent_id {
            return Err(Error::ParentIsChild);
        }

        // TODO(straivers): Allow a more intensive check to make sure that
        // indices occur only once in the tree.

        debug_assert!(
            !self.is_ancestor(child_id, parent_id),
            "parent cannot be a descendant of itself"
        );

        let parent = self.store.get(parent_id.0).ok_or(Error::InvalidIndex)?;
        let child = self.store.get(child_id.0).ok_or(Error::InvalidIndex)?;

        if let Some(first_child) = self.store.get(parent.first_child.get().0) {
            first_child.prev.set(child_id);
        }

        child.next.set(parent.first_child.get());
        child.parent.set(parent_id);
        parent.first_child.set(child_id);

        Ok(())
    }

    pub fn add_children(
        &mut self,
        parent_id: Index<T>,
        children: NodeList<T>,
    ) -> Result<(), Error> {
        let mut child = children.unwrap();
        while child != Index::default() {
            // NOTE(straivers): This must happen before the child is added to
            // the tree since NodeList co-opts the next index to describe the
            // list.
            let next = self.store.get(child.0).unwrap().next.get();
            self.add_child(parent_id, child)?;
            child = next;
        }

        Ok(())
    }

    pub fn remove(&mut self, node_id: Index<T>) -> Result<T, Error> {
        if self.root == node_id {
            self.root = Index::default();
        }

        let node = self.store.remove(node_id.0).ok_or(Error::InvalidIndex)?;
        self.remove_internal(node_id, node)
    }

    fn remove_internal(&mut self, node_id: Index<T>, node: Node<T>) -> Result<T, Error> {
        let Node {
            next,
            prev,
            parent,
            first_child,
            value,
        } = node;

        let next_id = next.get();
        let prev_id = prev.get();
        let parent_id = parent.get();

        if let Some(parent) = self.store.get(parent_id.0) {
            if parent.first_child.get() == node_id {
                parent.first_child.set(next_id);
            }
        }

        if let Some(next_node) = self.store.get(next_id.0) {
            next_node.prev.set(prev_id);
        }

        if let Some(prev_node) = self.store.get(prev_id.0) {
            prev_node.next.set(next_id);
        }

        let mut child_id = first_child.get();
        while child_id != Index::default() {
            let child = self.store.remove(child_id.0).unwrap();
            child_id = child.next.get();
            let _dropped = self.remove_internal(child_id, child)?;
        }

        Ok(value)
    }
}

pub struct IndexIter<'a, T> {
    store: &'a IndexedStore<Node<T>>,
    current: Index<T>,
}

impl<'a, T> Iterator for IndexIter<'a, T> {
    type Item = Index<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current == Index::default() {
            return None;
        }

        // We should never encounter an invalid node index within the
        // tree.

        let value = self.current;
        self.current = self
            .store
            .get(self.current.0)
            .expect("invalid internal node index")
            .next
            .get();

        Some(value)
    }
}

pub struct NodeList<T> {
    head: Index<T>,
    tail: Index<T>,
}

impl<T> Default for NodeList<T> {
    fn default() -> Self {
        Self {
            head: Default::default(),
            tail: Default::default(),
        }
    }
}

impl<T> NodeList<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, tree: &IndexedTree<T>, tail: Index<T>) {
        if self.head == Index::default() {
            self.head = tail;
        } else {
            // Co-opt the tail node's next pointer to point to the new tail. It
            // will be overridden when the list is added to the tree.
            let prev_tail = tree.store.get(self.tail.0).unwrap();
            prev_tail.next.set(tail);
        }

        self.tail = tail;
    }

    fn unwrap(self) -> Index<T> {
        self.head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let tree = IndexedTree::<u32>::new();
        assert!(tree.is_empty());
    }

    #[test]
    fn three_leaves() {
        let mut tree = IndexedTree::new();

        let a = tree.new_node(0).unwrap();
        let b = tree.new_node(1).unwrap();
        let c = tree.new_node(2).unwrap();
        let d = tree.new_node(3).unwrap();

        assert_eq!(*tree.get(a).unwrap(), 0);
        assert_eq!(*tree.get(b).unwrap(), 1);
        assert_eq!(*tree.get(c).unwrap(), 2);
        assert_eq!(*tree.get(d).unwrap(), 3);

        tree.add_child(a, b).unwrap();
        assert!(tree.is_ancestor(a, b));
        tree.add_child(a, c).unwrap();
        assert!(tree.is_ancestor(a, c));
        tree.add_child(a, d).unwrap();
        assert!(tree.is_ancestor(a, d));
        assert_eq!(tree.children(a).cloned().collect::<Vec<_>>(), [3, 2, 1]);

        tree.remove(d).unwrap();
        assert_eq!(tree.children(a).cloned().collect::<Vec<_>>(), [2, 1]);

        tree.remove(b).unwrap();
        assert_eq!(tree.children(a).cloned().collect::<Vec<_>>(), [2]);

        tree.remove(a).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn unbalanced_tree() {
        let mut tree = IndexedTree::new();

        let a = tree.new_node(0).unwrap();
        let b = tree.new_node(1).unwrap();
        let c = tree.new_node(2).unwrap();
        let d = tree.new_node(3).unwrap();
        let e = tree.new_node(4).unwrap();

        tree.add_child(a, b).unwrap();
        tree.add_child(b, c).unwrap();
        tree.add_child(c, d).unwrap();
        tree.add_child(d, e).unwrap();

        assert!(tree.is_ancestor(a, b));
        assert!(tree.is_ancestor(a, c));
        assert!(tree.is_ancestor(a, d));
        assert!(tree.is_ancestor(a, e));

        tree.remove(a).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn children() {
        let mut tree = IndexedTree::new();

        let root = tree.new_node(0).unwrap();

        let list = {
            let mut list = NodeList::new();
            let a = tree.new_node(1).unwrap();
            list.push(&tree, a);
            let b = tree.new_node(2).unwrap();
            list.push(&tree, b);
            let c = tree.new_node(3).unwrap();
            list.push(&tree, c);
            list
        };

        tree.add_children(root, list).unwrap();

        assert_eq!(tree.children(root).cloned().collect::<Vec<_>>(), [3, 2, 1]);
    }
}
