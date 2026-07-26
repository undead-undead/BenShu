//! memory.rs — Memory Pool and Arena for High-Performance Inference

use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const DEFAULT_ALIGNMENT: usize = 64; // Cache line aligned

/// Arena allocator for fast bump allocation during inference.
pub struct InferenceArena {
    memory: *mut u8,
    offset: AtomicUsize,
    capacity: usize,
    layout: Layout,
}

unsafe impl Send for InferenceArena {}
unsafe impl Sync for InferenceArena {}

impl InferenceArena {
    pub fn new(capacity: usize) -> Self {
        let aligned_capacity = (capacity + DEFAULT_ALIGNMENT - 1) & !(DEFAULT_ALIGNMENT - 1);
        let layout =
            Layout::from_size_align(aligned_capacity, DEFAULT_ALIGNMENT).expect("Invalid layout");

        let memory = unsafe { alloc_zeroed(layout) };
        if memory.is_null() {
            panic!("Failed to allocate arena of {} bytes", aligned_capacity);
        }

        Self {
            memory,
            offset: AtomicUsize::new(0),
            capacity: aligned_capacity,
            layout,
        }
    }

    /// Allocate a slice of type T from the arena.
    pub fn alloc<T: Copy + Default>(&self, count: usize) -> Option<&mut [T]> {
        let size = count * std::mem::size_of::<T>();
        let align = std::mem::align_of::<T>().max(DEFAULT_ALIGNMENT);

        loop {
            let current = self.offset.load(Ordering::Acquire);
            let aligned_offset = (current + align - 1) & !(align - 1);
            let new_offset = aligned_offset + size;

            if new_offset > self.capacity {
                return None;
            }

            if self
                .offset
                .compare_exchange(current, new_offset, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                unsafe {
                    let ptr = self.memory.add(aligned_offset) as *mut T;
                    std::ptr::write_bytes(ptr, 0, count);
                    return Some(std::slice::from_raw_parts_mut(ptr, count));
                }
            }
        }
    }

    pub fn reset(&self) {
        self.offset.store(0, Ordering::Release);
    }

    pub fn used(&self) -> usize {
        self.offset.load(Ordering::Acquire)
    }
}

impl Drop for InferenceArena {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.memory, self.layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn concurrent_allocations_do_not_spuriously_fail_before_capacity() {
        let arena = Arc::new(InferenceArena::new(4096));
        let mut threads = Vec::new();

        for _ in 0..8 {
            let arena = Arc::clone(&arena);
            threads.push(std::thread::spawn(move || arena.alloc::<u64>(8).is_some()));
        }

        let successes = threads
            .into_iter()
            .map(|handle| handle.join().expect("thread join"))
            .filter(|ok| *ok)
            .count();

        assert_eq!(successes, 8);
    }
}
