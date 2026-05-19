use core::{
    cell::RefCell,
    error::Error,
    fmt::{self, Write},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use avr_device::interrupt::Mutex;

const NUM_BUFFERS: usize = 8;
const BUFFER_SIZE: usize = 64;

#[derive(Debug)]
pub enum BufferError {
    PoolFull,
    PoolEmpty,
    AlreadyDeallocated,
    InvalidIndex,
    InsufficientSpace,
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let txt = match self {
            BufferError::PoolFull => "PoolFull",
            BufferError::PoolEmpty => "PoolEmpty",
            BufferError::AlreadyDeallocated => "AlreadyDeallocated",
            BufferError::InvalidIndex => "InvalidIndex",
            BufferError::InsufficientSpace => "InsufficientSpace",
        };
        write!(f, "{}", txt)
    }
}

impl From<BufferError> for fmt::Error {
    fn from(_err: BufferError) -> Self {
        fmt::Error
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
        // if self.count == 0 {
        //     return Err(BufferError::PoolEmpty);
        // }
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

    pub fn free_buffers() -> u8 {
        avr_device::interrupt::free(|cs| {
            // get the pool
            let mut buffer_pool = BUFFER_POOL.borrow(cs).borrow_mut();
            buffer_pool.allocator.count
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

// because buffer handle
unsafe impl Send for BufferHandle {}
unsafe impl Sync for BufferHandle {}

pub struct BufferHandle {
    // pointer to the start of the buffer
    ptr: *mut u8,
    // read index
    read_idx: u8,
    // write index
    write_idx: u8,
    // number of bytes in the buffer
    len: u8,
    // buffer capacity
    capacity: u8,
    // index to return to buffer pool
    pool_idx: u8,
}

impl BufferHandle {
    pub fn new(ptr: *mut u8, capacity: u8, pool_idx: u8) -> Self {
        Self {
            ptr,
            read_idx: 0,
            write_idx: 0,
            len: 0,
            capacity,
            pool_idx,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> u8 {
        self.len
    }

    #[inline(always)]
    pub fn free_space(&self) -> u8 {
        self.capacity - self.len
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.read_idx = 0;
    }

    #[inline(always)]
    /// there will be issues with this since there is no way to update read_idx or len
    pub fn as_slice(&self) -> &[u8] {
        // get a slice for reading based off the read_idx
        unsafe {
            let ptr = self.ptr.add(self.read_idx as usize);
            let len = self.capacity - self.read_idx;
            core::slice::from_raw_parts(ptr, len as usize)
        }
    }

    #[inline(always)]
    /// there will be issues with this since there is no way to update write_idx or len
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // get a slice for writing based off write index
        unsafe {
            let ptr = self.ptr.add(self.write_idx as usize);
            let len = self.capacity - self.write_idx;
            core::slice::from_raw_parts_mut(ptr, len as usize)
        }
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.len == 0 {
            return None;
        }
        // read a byte
        let byte = unsafe { self.ptr.add(self.read_idx as usize).read_volatile() };
        // update the read index
        self.read_idx += 1;
        // update the length
        self.len -= 1;
        // return the next byte
        Some(byte)
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) {
        // check if we have space
        if self.len == self.capacity {
            return;
        }
        // write the byte
        unsafe { self.ptr.add(self.write_idx as usize).write_volatile(byte) };
        // update the write index
        self.write_idx += 1;
        // increment the length
        self.len += 1;
    }

    #[inline(always)]
    pub fn read_byte_wrapped(&mut self) -> Option<u8> {
        // is there anything to read
        if self.len == 0 {
            return None;
        }
        // read a byte
        let byte = unsafe { self.ptr.add(self.read_idx as usize).read_volatile() };
        // update the read index
        self.read_idx += 1;
        // did the index hit the capacity
        if self.read_idx == self.capacity {
            // wrap it back to the beginning
            self.read_idx = 0;
        }
        // decrement the length
        self.len -= 1;
        // return the next byte
        Some(byte)
    }

    #[inline(always)]
    pub fn write_byte_wrapped(&mut self, byte: u8) {
        // check if we have space
        if self.len == self.capacity {
            return;
        }
        // write the byte
        unsafe { self.ptr.add(self.write_idx as usize).write_volatile(byte) };
        // update the write index
        self.write_idx += 1;
        // did the index hit the capacity
        if self.write_idx == self.capacity {
            // wrap it back to the beginning
            self.write_idx = 0;
        }
        // increment the length
        self.len += 1;
    }

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<u8> {
        // get the length
        let len = bytes.len() as u8;
        // first see if it will fit
        if self.free_space() < len {
            return Err(BufferError::InsufficientSpace);
        }
        // get a mutable slice and write the bytes
        self.as_mut_slice()[..len as usize].copy_from_slice(bytes);
        // update the write index
        self.write_idx += len;
        // increment the length
        self.len += len;
        // return the len
        Ok(len)
    }
}

impl Write for BufferHandle {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        _ = BufferRequest::release_buffer(self.pool_idx);
    }
}

impl Deref for BufferHandle {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for BufferHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

// because buffer handle
unsafe impl Send for BufferHandle2 {}
unsafe impl Sync for BufferHandle2 {}

pub struct BufferHandle2 {
    // pointer to the start of the buffer
    start_ptr: *mut u8,
    // pointer to the end of the buffer
    end_ptr: *mut u8,
    // buffer capacity
    capacity: u8,
    // read pointer
    read_ptr: *mut u8,
    // write pointer
    write_ptr: *mut u8,
    // index to return to buffer pool
    pool_idx: u8,
}

impl BufferHandle2 {
    pub fn new(ptr: *mut u8, capacity: u8, pool_idx: u8) -> Self {
        Self {
            start_ptr: ptr,
            end_ptr: unsafe { ptr.add(capacity as usize) },
            capacity,
            read_ptr: ptr,
            write_ptr: ptr,
            pool_idx,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> u8 {
        // convert the the pointer addresses into u8's
        let write_addr = self.write_ptr.addr() as u8;
        let read_addr = self.read_ptr.addr() as u8;
        // simple wrapping subtract will get us the length
        write_addr.wrapping_sub(read_addr)
    }

    #[inline(always)]
    pub fn free_space(&self) -> u8 {
        self.capacity - self.len()
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.read_ptr = self.start_ptr;
        self.write_ptr = self.start_ptr;
    }

    #[inline(always)]
    /// there will be issues with this since there is no way to update read_idx or len
    pub fn as_slice(&self) -> &[u8] {
        // get a slice for reading based off the read_idx
        unsafe {
            let ptr = self.read_ptr;
            let len = self.end_ptr.offset_from_unsigned(self.read_ptr);
            core::slice::from_raw_parts(ptr, len)
        }
    }

    #[inline(always)]
    /// there will be issues with this since there is no way to update write_idx or len
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // get a slice for writing based off write index
        unsafe {
            let ptr = self.write_ptr;
            let len = self.end_ptr.offset_from_unsigned(self.write_ptr);
            core::slice::from_raw_parts_mut(ptr, len)
        }
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.read_ptr.addr() == self.write_ptr.addr() {
            return None;
        }
        // read a byte
        let byte = unsafe { self.read_ptr.read_volatile() };
        // update the read pointer
        self.read_ptr = unsafe { self.read_ptr.add(1) };
        // return the next byte
        Some(byte)
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) {
        // are we full
        if self.write_ptr.addr() == self.end_ptr.addr() {
            return;
        }
        // write the byte
        unsafe { self.write_ptr.write_volatile(byte) };
        // update the write pointer
        self.write_ptr = unsafe { self.write_ptr.add(1) };
    }

    #[inline(always)]
    pub fn read_byte_wrapped(&mut self) -> Option<u8> {
        // is there anything to read
        if self.read_ptr.addr() == self.write_ptr.addr() {
            return None;
        }
        // read a byte
        let byte = unsafe { self.read_ptr.read_volatile() };
        // update the read pointer
        self.read_ptr = unsafe { self.read_ptr.add(1) };
        // check for wrapping
        if self.read_ptr.addr() == self.end_ptr.addr() {
            self.read_ptr = self.start_ptr;
        }
        // return the next byte
        Some(byte)
    }

    #[inline(always)]
    pub fn write_byte_wrapped(&mut self, byte: u8) {
        // get the next write position
        let mut next_write_ptr = unsafe { self.write_ptr.add(1) };
        // check for wrapping
        if next_write_ptr.addr() == self.end_ptr.addr() {
            next_write_ptr = self.start_ptr;
        }
        // if the next write position equals the read position we are full
        if next_write_ptr.addr() == self.read_ptr.addr() {
            return;
        }
        // write the byte
        unsafe { self.write_ptr.write_volatile(byte) };
        // update the write pointer
        self.write_ptr = next_write_ptr;
    }

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<u8> {
        // get the length
        let len = bytes.len();
        // first see if it will fit
        if self.free_space() < len as u8 {
            return Err(BufferError::InsufficientSpace);
        }
        // get a mutable slice and write the bytes
        self.as_mut_slice()[..len as usize].copy_from_slice(bytes);
        // update the write pointer
        self.write_ptr = unsafe { self.write_ptr.add(len) };
        // return the len
        Ok(len as u8)
    }
}

impl Write for BufferHandle2 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}

impl Drop for BufferHandle2 {
    fn drop(&mut self) {
        _ = BufferRequest::release_buffer(self.pool_idx);
    }
}

impl Deref for BufferHandle2 {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for BufferHandle2 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

// Local Variables:
// jinx-local-words: "Deallocated idx len"
// End:
