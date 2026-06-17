use core::{error::Error, fmt, slice};

use embedded_io::ErrorKind;

use crate::{BufferRequest, CircularBuffer, FlatBuffer};

#[derive(Debug)]
pub enum ReadStatus {
    /// read completed with bytes read
    Complete(usize),
    /// partial read complete with bytes read
    Partial(usize),
    /// read complete because buffer has been filled with bytes
    BufferFull(usize),
}

#[derive(Debug)]
pub enum ReadError {
    /// read called and destination buffer is empty
    DestinationEmpty,
    /// read called and no bytes to read from source
    SourceEmpty,
    /// read called and io is disconnected
    Disconnected,
}
impl Error for ReadError {}
impl embedded_io_async::Error for ReadError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}
impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let txt = match self {
            ReadError::DestinationEmpty => "DestinationEmpty",
            ReadError::SourceEmpty => "SourceEmpty",
            ReadError::Disconnected => "Disconnected",
        };
        write!(f, "{}", txt)
    }
}

#[derive(Debug)]
pub enum BufferError {
    BufferEmpty,
    InsufficientSpace,
    PartialRead(usize),
}

impl From<BufferError> for embedded_io::ErrorKind {
    fn from(err: BufferError) -> Self {
        match err {
            BufferError::BufferEmpty => ErrorKind::WriteZero,
            BufferError::InsufficientSpace => ErrorKind::OutOfMemory,
            BufferError::PartialRead(_) => ErrorKind::WriteZero,
        }
    }
}

impl From<ErrorKind> for BufferError {
    fn from(err: ErrorKind) -> Self {
        match err {
            ErrorKind::WriteZero => BufferError::BufferEmpty,
            ErrorKind::OutOfMemory => BufferError::InsufficientSpace,
            _ => BufferError::BufferEmpty,
        }
    }
}

impl fmt::Display for BufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let txt = match self {
            BufferError::BufferEmpty => "BufferEmpty",
            BufferError::InsufficientSpace => "InsufficientSpace",
            BufferError::PartialRead(_) => "PartialRead",
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

impl From<BufferHandle> for FlatBuffer<'_> {
    fn from(handle: BufferHandle) -> Self {
        FlatBuffer::new(handle.ptr, handle.capacity)
    }
}

impl From<BufferHandle> for &mut [u8] {
    fn from(handle: BufferHandle) -> Self {
        unsafe { slice::from_raw_parts_mut(handle.ptr, handle.capacity) }
    }
}

impl Drop for BufferHandle {
    fn drop(&mut self) {
        _ = BufferRequest::release_buffer(self.pool_idx);
    }
}
