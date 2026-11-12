//! `Fixed<T, N>`: the engine's fixed-capacity vector (architecture §4.1).

use std::hash::{Hash, Hasher};

/// An `[T; N]` plus a `u8` length, `Copy` whenever `T` is. Pushing past `N` returns [`Full`],
/// which the engine turns into a stall, never a panic.
#[derive(Clone, Copy)]
pub struct Fixed<T, const N: usize> {
    items: [T; N],
    len: u8,
}

/// A push into a full [`Fixed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

impl<T: Clone + Default, const N: usize> Fixed<T, N> {
    pub fn new() -> Self {
        const { assert!(N <= u8::MAX as usize) };
        Self {
            items: std::array::from_fn(|_| T::default()),
            len: 0,
        }
    }
}

impl<T: Clone + Default, const N: usize> Default for Fixed<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone, const N: usize> Fixed<T, N> {
    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[T] {
        &self.items[..self.len()]
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        let len = self.len();
        &mut self.items[..len]
    }

    pub fn push(&mut self, item: T) -> Result<(), Full> {
        let len = self.len();
        if len == N {
            return Err(Full);
        }
        self.items[len] = item;
        self.len += 1;
        Ok(())
    }

    /// Inserts at `index`, shifting later items up.
    pub fn insert(&mut self, index: usize, item: T) -> Result<(), Full> {
        let len = self.len();
        if len == N {
            return Err(Full);
        }
        for i in (index..len).rev() {
            self.items[i + 1] = self.items[i].clone();
        }
        self.items[index] = item;
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        Some(self.items[self.len()].clone())
    }

    /// Removes the item at `index`, keeping the order of the rest.
    pub fn remove(&mut self, index: usize) -> T {
        let len = self.len();
        let item = self.as_slice()[index].clone();
        for i in index + 1..len {
            self.items[i - 1] = self.items[i].clone();
        }
        self.len -= 1;
        item
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Keeps only the items for which `keep` returns true, in order.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut kept = 0;
        for i in 0..self.len() {
            if keep(&self.items[i]) {
                self.items[kept] = self.items[i].clone();
                kept += 1;
            }
        }
        self.len = kept as u8;
    }

    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

impl<T: Copy + PartialEq, const N: usize> Fixed<T, N> {
    pub fn position(&self, item: T) -> Option<usize> {
        self.iter().position(|x| *x == item)
    }
}

impl<'a, T: Clone, const N: usize> IntoIterator for &'a Fixed<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// Equality and hashing see only the live items, so stale ones past `len` never matter.
impl<T: Clone + PartialEq, const N: usize> PartialEq for Fixed<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Clone + Eq, const N: usize> Eq for Fixed<T, N> {}

impl<T: Clone + Hash, const N: usize> Hash for Fixed<T, N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T: Clone + std::fmt::Debug, const N: usize> std::fmt::Debug for Fixed<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.as_slice()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{Fixed, Full};
    use crate::layout::Queue;

    #[test]
    fn push_past_capacity_is_an_error_not_a_panic() {
        let mut v: Fixed<u8, 2> = Fixed::new();
        assert_eq!(v.push(1), Ok(()));
        assert_eq!(v.push(2), Ok(()));
        assert_eq!(v.push(3), Err(Full));
        assert_eq!(v.as_slice(), &[1, 2]);
    }

    #[test]
    fn a_push_if_there_is_room_says_when_there_was_none() {
        let mut v: Fixed<u8, 1> = Fixed::new();
        assert!(v.push_if_room(1));
        assert!(!v.push_if_room(2));
        assert_eq!(v.as_slice(), &[1]);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "argued never to fill")]
    fn a_wrong_argument_that_a_list_cannot_fill_panics_in_a_debug_build() {
        let mut v: Fixed<u8, 1> = Fixed::new();
        v.push_bounded(1);
        v.push_bounded(2);
    }

    #[test]
    fn insert_and_remove_keep_order() {
        let mut v: Fixed<u8, 8> = Fixed::new();
        for i in [1, 2, 4] {
            v.push(i).unwrap();
        }
        v.insert(2, 3).unwrap();
        v.insert(0, 0).unwrap();
        assert_eq!(v.as_slice(), &[0, 1, 2, 3, 4]);
        assert_eq!(v.remove(1), 1);
        assert_eq!(v.as_slice(), &[0, 2, 3, 4]);
        v.retain(|&x| x % 2 == 0);
        assert_eq!(v.as_slice(), &[0, 2, 4]);
    }

    #[test]
    fn equality_ignores_stale_items() {
        let mut a: Fixed<u8, 4> = Fixed::new();
        let mut b: Fixed<u8, 4> = Fixed::new();
        a.push(9).unwrap();
        a.pop();
        b.push(1).unwrap();
        b.pop();
        assert_eq!(a, b);
    }
}
