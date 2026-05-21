use core::fmt::{self, Write};

use crate::{BufferError, BufferHandle};

type Result<T> = core::result::Result<T, BufferError>;

// because pointers can't be sent between threads safely
unsafe impl Send for CircularBuffer {}
unsafe impl Sync for CircularBuffer {}

pub struct CircularBuffer {
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

impl CircularBuffer {
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
        let offset = unsafe { self.write_ptr.offset_from(self.read_ptr) };
        // if the offset is less then 0 the write pointer has wrapped around
        let len = if offset < 0 {
            self.capacity as isize + offset
        } else {
            offset
        };
        // cast length to a usize
        len as usize
    }

    #[inline(always)]
    pub fn free_space(&self) -> usize {
        // since we don't track length, we always want to leave one slot open so the
        // write pointer never equals the read pointer so subtract 1
        self.capacity - self.len() - 1
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
        // check for wrapping
        if self.read_ptr == self.end_ptr {
            self.read_ptr = self.start_ptr;
        }
        // return the next byte
        Some(byte)
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) {
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
        let bytes_len = bytes.len();
        // first see if it will fit
        if self.free_space() < bytes_len {
            return Err(BufferError::InsufficientSpace);
        }
        // get the length from the write pointer to the end
        let space_to_end = unsafe { self.end_ptr.offset_from(self.write_ptr) as usize };
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = core::cmp::min(bytes_len, space_to_end);
        unsafe {
            // preform the copy
            core::slice::from_raw_parts_mut(self.write_ptr, first_copy_len)
                .copy_from_slice(&bytes[..first_copy_len]);
            // update the write pointer
            self.write_ptr = self.write_ptr.add(first_copy_len);
            // check for wrapping
            if self.write_ptr == self.end_ptr {
                self.write_ptr = self.start_ptr;
            }
        }
        // figure out if we have a second half to write
        let second_copy_len = bytes_len - first_copy_len;
        if second_copy_len > 0 {
            unsafe {
                // preform the copy
                core::slice::from_raw_parts_mut(self.start_ptr, second_copy_len)
                    .copy_from_slice(&bytes[first_copy_len..]);

                // update the write pointer
                self.write_ptr = self.start_ptr.add(second_copy_len);
            }
        }
        // return
        Ok(bytes_len)
    }
}

impl Write for CircularBuffer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}
