#![no_std]

pub struct MemPool<const BLK_SIZE: usize, const BLK_COUNT: usize> {
    // Stores the actual buffer memory
    storage: [[u8; BLK_SIZE]; BLK_COUNT],
    // Tracks allocation state: 0 = free, 1 = allocated
    // Supports up to 8 blocks per pool. Scale up to u16/u32 if needed.
    used_mask: u8,
}

impl<const BLK_SIZE: usize, const BLK_COUNT: usize> MemPool<BLK_SIZE, BLK_COUNT> {
    pub const fn new() -> Self {
        // Enforce safety limit for the 8-bit mask tracking
        assert!(BLK_COUNT <= 8);
        Self {
            storage: [[0; BLK_SIZE]; BLK_COUNT],
            used_mask: 0,
        }
    }

    /// Allocates a block from this pool
    pub fn alloc(&mut self) -> Option<&'static mut [u8; BLK_SIZE]> {
        for i in 0..BLK_COUNT {
            if (self.used_mask & (1 << i)) == 0 {
                self.used_mask |= 1 << i;
                // Safely extend the lifetime to 'static for global access
                let ptr = &mut self.storage[i] as *mut [u8; BLK_SIZE];
                return Some(unsafe { &mut *ptr });
            }
        }
        None
    }

    /// Frees a block by checking if the pointer belongs to this pool
    pub fn free(&mut self, block_ptr: *const u8) -> bool {
        let start = self.storage.as_ptr() as *const u8;
        let end = unsafe { start.add(BLK_SIZE * BLK_COUNT) };

        if block_ptr >= start && block_ptr < end {
            let offset = block_ptr as usize - start as usize;
            let index = offset / BLK_SIZE;
            self.used_mask &= !(1 << index);
            return true;
        }
        false
    }
}

// Global Allocator Framework managing multiple pools
pub struct ArbitraryAllocator {
    small_pool: MemPool<16, 8>,  // 8 blocks of 16 bytes
    medium_pool: MemPool<64, 4>, // 4 blocks of 64 bytes
    large_pool: MemPool<256, 2>, // 2 blocks of 256 bytes
}

impl ArbitraryAllocator {
    pub const fn new() -> Self {
        Self {
            small_pool: MemPool::new(),
            medium_pool: MemPool::new(),
            large_pool: MemPool::new(),
        }
    }

    /// Requests an arbitrary size and returns the best matching block
    pub fn request(&mut self, size: usize) -> Option<&'static mut [u8]> {
        if size <= 16 {
            self.small_pool.alloc().map(|b| &mut b[..size])
        } else if size <= 64 {
            self.medium_pool.alloc().map(|b| &mut b[..size])
        } else if size <= 256 {
            self.large_pool.alloc().map(|b| &mut b[..size])
        } else {
            None // Size too large
        }
    }

    /// Automatically routes the pointer to the correct pool to free it
    pub fn release(&mut self, block: &[u8]) {
        let ptr = block.as_ptr();
        if self.small_pool.free(ptr) {
            return;
        }
        if self.medium_pool.free(ptr) {
            return;
        }
        if self.large_pool.free(ptr) {
            return;
        }
    }
}

// Usage Example
static mut SYSTEM_ALLOCATOR: ArbitraryAllocator = ArbitraryAllocator::new();

fn process_data() {
    let allocator = unsafe { &mut SYSTEM_ALLOCATOR };

    // Request an arbitrary 40-byte chunk (will be served by the 64-byte pool)
    if let Some(my_buf) = allocator.request(40) {
        my_buf[0] = 0xAA;

        // When finished, pass the slice back to release it
        allocator.release(my_buf);
    }
}
