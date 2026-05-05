use core::{
    cell::{RefCell, SyncUnsafeCell},
    fmt::{self, Write},
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use avr_device::interrupt::Mutex;

use crate::async_queue::{AsyncQueue, QueueError};

const NUM_BUFFERS: usize = 4;
const BUFFER_SIZE: usize = 64;

static BUFFER_POOL: Mutex<RefCell<BufferPool<NUM_BUFFERS, BUFFER_SIZE>>> =
    Mutex::new(RefCell::new(BufferPool::new()));

static BUFFER_REGISTER: AsyncQueue<u8, NUM_BUFFERS> = AsyncQueue::new();

struct BufferAllocator<const BUFFER_COUNT: usize> {
    alloc_idx: u8,
    dealloc_idx: u8,
    free_allocations: [u8; BUFFER_COUNT],
    count: u8,
}

impl<const BUFFER_COUNT: usize> BufferAllocator<BUFFER_COUNT> {
    pub const fn new() -> Self {
        let mut free = [0; BUFFER_COUNT];
        for i in 0..BUFFER_COUNT {
            free[i] = i as u8;
        }

        Self {
            alloc_idx: 0,
            dealloc_idx: 0,
            free_allocations: free,
            count: BUFFER_COUNT as u8,
        }
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

    pub async fn get_buffer() -> Result<BufferHandle, QueueError> {
        // let index = BUFFER_REGISTER.pop().await?;

        let data = avr_device::interrupt::free(|_| unsafe { &mut *BUFFER_POOL[0 as usize].get() });
        Ok(BufferHandle::new(0, data))
    }

    pub fn release_buffer(index: u8) {
        if (index as usize) < NUM_BUFFERS {
            avr_device::interrupt::free(|cs| BUFFER_REGISTER.try_push(index));
        }
    }
}

impl Future for BufferRequest {
    type Output = BufferHandle;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        avr_device::interrupt::free(|_cs| {
            let buffer_handle = BUFFER_POOL.iter().enumerate().find_map(|(index, cell)| {
                let buffer = unsafe { &mut *cell.get() };
                if buffer.in_use {
                    None
                } else {
                    buffer.in_use = true;
                    Some(BufferHandle::new(index as u8, &mut buffer.data))
                }
            });

            match buffer_handle {
                Some(buffer) => Poll::Ready(buffer),
                None => Poll::Pending,
            }
        })
    }
}

pub struct BufferHandle {
    index: u8,
    pub slice: &'static mut [u8],
    write_pos: u8,
}

impl BufferHandle {
    #[rustfmt::skip]
    pub fn new(index: u8, slice: &'static mut [u8]) -> Self {
        Self { index, slice, write_pos: 0 }
    }

    pub fn length(&self) -> u8 {
        self.write_pos
    }
}

impl Write for BufferHandle {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let len = bytes.len();
        let write_pos = self.write_pos as usize;

        if write_pos + len > self.slice.len() {
            return Err(fmt::Error);
        }

        self.slice[write_pos..write_pos + len].copy_from_slice(bytes);
        self.write_pos = write_pos as u8 + len as u8;
        Ok(())
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        BufferPool::release_buffer(self.index);
    }
}

impl Deref for BufferHandle {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.slice
    }
}

impl DerefMut for BufferHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slice
    }
}
