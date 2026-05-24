use core::{error::Error, fmt};

use crate::{BufferRequest, CircularBuffer, FlatBuffer};

#[derive(Debug)]
pub enum BufferError {
    BufferEmpty,
    InsufficientSpace,
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let txt = match self {
            BufferError::BufferEmpty => "BufferEmpty",
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

pub struct BufferHandle {
    // pointer to the start of the buffer
    ptr: *mut u8,
    // buffer capacity
    capacity: usize,
    // index to return to buffer pool
    pool_idx: u8,
}

impl BufferHandle {
    pub fn new(ptr: *mut u8, capacity: usize, pool_idx: u8) -> Self {
        Self {
            ptr,
            capacity,
            pool_idx,
        }
    }
}

impl From<BufferHandle> for CircularBuffer {
    fn from(handle: BufferHandle) -> Self {
        CircularBuffer::new(handle.ptr, handle.capacity)
    }
}

impl From<BufferHandle> for FlatBuffer {
    fn from(handle: BufferHandle) -> Self {
        FlatBuffer::new(handle.ptr, handle.capacity)
    }
}

// impl Drop for BufferHandle {
//     fn drop(&mut self) {
//         _ = BufferRequest::release_buffer(self.pool_idx);
//     }
// }
