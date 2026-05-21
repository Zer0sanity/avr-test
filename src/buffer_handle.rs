use core::{
    error::Error,
    fmt::{self, Write},
    ops::{Deref, DerefMut},
};
use embedded_io::{ErrorKind, ErrorType};

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

impl embedded_io::Error for BufferError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            BufferError::BufferEmpty => ErrorKind::Other,
            BufferError::InsufficientSpace => ErrorKind::Other,
        }
    }
}

type Result<T> = core::result::Result<T, BufferError>;

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

impl Drop for BufferHandle {
    fn drop(&mut self) {
        _ = BufferRequest::release_buffer(self.pool_idx);
    }
}
