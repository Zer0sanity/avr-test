use core::{
    cell::RefCell,
    error::Error,
    fmt::{self, Write},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use avr_device::interrupt::Mutex;

const NUM_BUFFERS: usize = 4;
const BUFFER_SIZE: usize = 64;

#[derive(Debug)]
pub enum BufferError {
    PoolFull,
    PoolEmpty,
    AlreadyDeallocated,
    InvalidIndex,
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Custom error occurred")
    }
}

impl Error for BufferError {}

type Result<T> = core::result::Result<T, BufferError>;

static BUFFER_POOL: Mutex<RefCell<BufferPool<NUM_BUFFERS, BUFFER_SIZE>>> =
    Mutex::new(RefCell::new(BufferPool::new()));

pub struct BufferAllocator<const BUFFER_COUNT: usize> {
    alloc_idx: u8,
    dealloc_idx: u8,
    allocations: [u8; BUFFER_COUNT],
    count: u8,
    in_use_mask: u8,
}

impl<const BUFFER_COUNT: usize> BufferAllocator<BUFFER_COUNT> {
    pub const fn new() -> Self {
        let mut i = 0;
        let mut free = [0; BUFFER_COUNT];
        while i < BUFFER_COUNT {
            free[i] = i as u8;
            i += 1;
        }

        Self {
            alloc_idx: 0,
            dealloc_idx: 0,
            allocations: free,
            count: BUFFER_COUNT as u8,
            in_use_mask: 0,
        }
    }

    pub fn try_alloc(&mut self) -> Result<u8> {
        // do we have buffers any to give out
        if self.count == 0 {
            return Err(BufferError::PoolEmpty);
        }
        // grab the buffer index
        let index = self.allocations[self.alloc_idx as usize];
        // increment the allocation index
        self.alloc_idx = (self.alloc_idx + 1) % BUFFER_COUNT as u8;
        // decrement the count
        self.count -= 1;
        // update the in_use_mask
        self.in_use_mask |= 1 << index;
        // return
        Ok(index)
    }

    pub fn try_dealloc(&mut self, index: u8) -> Result<()> {
        // do we have buffers any to give out
        if self.count == BUFFER_COUNT as u8 {
            return Err(BufferError::PoolFull);
        }
        // is the index valid
        if index >= BUFFER_COUNT as u8 {
            return Err(BufferError::InvalidIndex);
        }
        // is the index allocated
        if (self.in_use_mask & (1 << index)) == 0 {
            return Err(BufferError::AlreadyDeallocated);
        }

        // add the index back into the allocations free array
        self.allocations[self.dealloc_idx as usize] = index;
        // increment the deallocation index
        self.dealloc_idx = (self.dealloc_idx + 1) % BUFFER_COUNT as u8;
        // increment the count
        self.count += 1;
        // clear the in_use_mask
        self.in_use_mask &= !(1 << index);
        // return
        Ok(())
    }

    pub fn try_pop(&mut self) -> Option<u8> {
        // do we have buffers any
        if self.count != BUFFER_COUNT as u8 {
            return None;
        }
        // get the index of the first one
        let index = self.allocations[self.dealloc_idx as usize];
        // increment the deallocation index
        self.dealloc_idx = (self.dealloc_idx + 1) % BUFFER_COUNT as u8;
        // increment the count
        self.count += 1;
        // clear the in_use_mask
        self.in_use_mask &= !(1 << index);
        // return
        Some(index)
    }
}

pub struct BufferPool<const BUFFER_COUNT: usize, const BUFFER_CAPACITY: usize> {
    pool: [[u8; BUFFER_CAPACITY]; BUFFER_COUNT],
    allocator: BufferAllocator<BUFFER_COUNT>,
    waker: Option<Waker>,
}

impl<const BUFFER_COUNT: usize, const BUFFER_CAPACITY: usize>
    BufferPool<BUFFER_COUNT, BUFFER_CAPACITY>
{
    const ELEM: [u8; BUFFER_CAPACITY] = const { [0 as u8; BUFFER_CAPACITY] };
    const INIT_POOL: [[u8; BUFFER_CAPACITY]; BUFFER_COUNT] = [Self::ELEM; BUFFER_COUNT];
    const INIT_ALLOC: BufferAllocator<BUFFER_COUNT> = BufferAllocator::new();

    pub const fn new() -> Self {
        Self {
            pool: Self::INIT_POOL,
            allocator: Self::INIT_ALLOC,
            waker: None,
        }
    }
}

pub struct BufferRequest;

impl BufferRequest {
    pub fn release_buffer(index: u8) -> Result<()> {
        avr_device::interrupt::free(|cs| {
            // get the pool
            let mut buffer_pool = BUFFER_POOL.borrow(cs).borrow_mut();
            // try to deallocate buffer
            let result = buffer_pool.allocator.try_dealloc(index);
            // if the result was OK wake the waker
            _ = result
                .as_ref()
                .map(|_| buffer_pool.waker.take().map(|w| w.wake()));
            // return
            result
        })
    }
}

impl Future for BufferRequest {
    type Output = BufferHandle;

    #[rustfmt::skip]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        avr_device::interrupt::free(|cs| {
            // get the pool
            let mut buffer_pool = BUFFER_POOL.borrow(cs).borrow_mut();
            // try to allocate a buffer
            buffer_pool
                .allocator
                .try_alloc()
                .map(|pool_idx| {
                    // some trickery to get a mutable slice
                    let ptr = buffer_pool.pool[pool_idx as usize].as_mut_ptr();
                    let len = buffer_pool.pool[pool_idx as usize].len() as u8;
                    // poll ready
                    Poll::Ready(BufferHandle::new(ptr, len, pool_idx))
                })
                .unwrap_or_else(|_| {
                    // set waker
                    if buffer_pool
                        .waker
                        .as_ref()
                        .map_or(true, |w| !w.will_wake(cx.waker()))
                    {
                        buffer_pool.waker = Some(cx.waker().clone());
                    }
                    // poll pending
                    Poll::Pending
                })
        })
    }
}

pub struct BufferHandle {
    pub ptr: *mut u8,
    pub len: u8,
    pool_idx: u8,
}

impl BufferHandle {
    #[rustfmt::skip]
    pub fn new(ptr: *mut u8, len: u8, pool_idx: u8) -> Self {
        Self { ptr, len, pool_idx }
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.len as usize) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len as usize) }
    }
}

impl Write for BufferHandle {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // get the bytes representation of the string
        let bytes = s.as_bytes();
        // first see if it will fit
        if self.len < bytes.len() as u8 {
            return Err(fmt::Error);
        }
        self.as_mut_slice().copy_from_slice(bytes);
        Ok(())
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        _ = BufferRequest::release_buffer(self.pool_idx);
    }
}

// impl Deref for BufferHandle {
//     type Target = [u8];
//     fn deref(&self) -> &Self::Target {
//         self.slice
//     }
// }

// impl DerefMut for BufferHandle {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         self.slice
//     }
// }
