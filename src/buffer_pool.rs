use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
};

pub struct Buffer {
    data: [u8; 64],
    in_use: bool,
}

// wrapper for unsafe cell since it doesn't implement sync
pub struct SyncUnsafeCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }
    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

#[rustfmt::skip]
static BUFFER_POOL: [SyncUnsafeCell<Buffer>; 4] = [
    SyncUnsafeCell::new(Buffer { data: [0; 64], in_use: false }),
    SyncUnsafeCell::new(Buffer { data: [0; 64], in_use: false }),
    SyncUnsafeCell::new(Buffer { data: [0; 64], in_use: false }),
    SyncUnsafeCell::new(Buffer { data: [0; 64], in_use: false }),
];

pub struct BufferPool;

impl BufferPool {
    pub fn get_buffer(&self) -> Option<BufferHandle> {
        avr_device::interrupt::free(|_cs| {
            // add enumerate to get the index
            BUFFER_POOL.iter().enumerate().find_map(|(index, cell)| {
                let buffer = unsafe { &mut *cell.get() };
                if buffer.in_use {
                    None
                } else {
                    buffer.in_use = true;
                    Some(BufferHandle::new(index as u8, &mut buffer.data))
                }
            })
        })
    }
}

pub struct BufferHandle {
    index: u8,
    pub slice: &'static mut [u8],
}

impl BufferHandle {
    pub fn new(index: u8, slice: &'static mut [u8]) -> Self {
        Self { index, slice }
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        avr_device::interrupt::free(|_| {
            let buffer = unsafe { &mut *BUFFER_POOL[self.index as usize].get() };
            buffer.in_use = false;
        });
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
