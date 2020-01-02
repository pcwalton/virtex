// virtex/src/stack.rs

//! A simple concurrent blocking stack implemented with a mutex lock.

use std::sync::{Condvar, Mutex};

pub struct ConcurrentStack<T> {
    vector: Mutex<Vec<T>>,
    cond: Condvar,
}

impl<T> ConcurrentStack<T> {
    #[inline]
    pub fn new() -> ConcurrentStack<T> {
        ConcurrentStack { vector: Mutex::new(vec![]), cond: Condvar::new() }
    }

    #[inline]
    pub fn push(&self, object: T) {
        let mut guard = self.vector.lock().unwrap();
        guard.push(object);
        self.cond.notify_one();
    }

    #[inline]
    pub fn pop(&self) -> T {
        let mut guard = self.vector.lock().unwrap();
        loop {
            if let Some(object) = guard.pop() {
                return object;
            }
            guard = self.cond.wait(guard).unwrap();
        }
    }
}
