use core::{
    error::Error,
    fmt::{self, Write},
    ops::{Deref, DerefMut},
};

use crate::BufferRequest;

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

type Result<T> = core::result::Result<T, BufferError>;

// because pointers can't be sent between threads safely
unsafe impl Send for BufferHandle {}
unsafe impl Sync for BufferHandle {}

pub struct BufferHandle {
    // pointer to the start of the buffer
    start_ptr: *mut u8,
    // pointer to the end of the buffer
    end_ptr: *mut u8,
    // buffer capacity
    capacity: usize,
    // read pointer
    read_ptr: *mut u8,
    // write pointer
    write_ptr: *mut u8,
    // index to return to buffer pool
    pool_idx: u8,
}

impl BufferHandle {
    pub fn new(ptr: *mut u8, capacity: usize, pool_idx: u8) -> Self {
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
    pub fn len(&self) -> usize {
        // get the offset between the read and write pointers
        let offset = unsafe { self.write_ptr.offset_from(self.read_ptr) };
        // if the offset is less then 0 the write pointer has wrapped around
        let len = if offset < 0 {
            self.capacity as isize + offset
        } else {
            offset
        };
        // cast len to a usize
        len as usize
    }

    #[inline(always)]
    pub fn free_space(&self) -> usize {
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
        unsafe { core::slice::from_raw_parts(self.read_ptr as *const u8, self.len()) }
    }

    #[inline(always)]
    /// there will be issues with this since there is no way to update write_idx or len
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // get a slice for writing based off write index
        unsafe { core::slice::from_raw_parts_mut(self.write_ptr, self.free_space()) }
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.read_ptr == self.write_ptr {
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
        if self.write_ptr == self.end_ptr {
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
        if self.read_ptr == self.write_ptr {
            return None;
        }
        // read a byte
        let byte = unsafe { self.read_ptr.read_volatile() };
        // update the read pointer
        self.read_ptr = unsafe { self.read_ptr.add(1) };
        // check for wrapping
        if self.read_ptr == self.end_ptr {
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
        if next_write_ptr == self.end_ptr {
            next_write_ptr = self.start_ptr;
        }
        // if the next write position equals the read position we are full
        if next_write_ptr == self.read_ptr {
            return;
        }
        // write the byte
        unsafe { self.write_ptr.write_volatile(byte) };
        // update the write pointer
        self.write_ptr = next_write_ptr;
    }

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        // get the length
        let len = bytes.len();
        // first see if it will fit
        if self.free_space() < len {
            return Err(BufferError::InsufficientSpace);
        }
        // get a mutable slice and write the bytes
        self.as_mut_slice()[..len].copy_from_slice(bytes);
        // update the write pointer
        self.write_ptr = unsafe { self.write_ptr.add(len) };
        // return
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
