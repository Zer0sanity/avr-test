use core::fmt::{self, Write};

use crate::{BufferError, BufferHandle};

type Result<T> = core::result::Result<T, BufferError>;

// because pointers can't be sent between threads safely
unsafe impl Send for FlatBuffer {}
unsafe impl Sync for FlatBuffer {}

pub struct FlatBuffer {
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
}

impl FlatBuffer {
    pub fn new(ptr: *mut u8, capacity: usize) -> Self {
        Self {
            start_ptr: ptr,
            end_ptr: unsafe { ptr.add(capacity as usize) },
            capacity,
            read_ptr: ptr,
            write_ptr: ptr,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        // get the offset between the read and write pointers
        unsafe { self.write_ptr.offset_from(self.read_ptr) as usize }
    }

    #[inline(always)]
    pub fn free_space(&self) -> usize {
        // space from end to write
        unsafe { self.end_ptr.offset_from(self.write_ptr) as usize }
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
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        // get the length
        let len = bytes.len();
        // first see if it will fit
        if self.free_space() < len {
            return Err(BufferError::InsufficientSpace);
        }
        // preform the copy
        unsafe {
            core::slice::from_raw_parts_mut(self.write_ptr, len).copy_from_slice(bytes);
            self.write_ptr = self.write_ptr.add(len);
        }
        // return
        Ok(len)
    }
}

impl Write for FlatBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}
