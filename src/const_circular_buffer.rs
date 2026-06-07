use core::fmt;

use crate::BufferError;

type Result<T> = core::result::Result<T, BufferError>;

// because pointers can't be sent between threads safely
unsafe impl<const CAPACITY: usize> Send for ConstCircularBuffer<CAPACITY> {}
// unsafe impl Sync for ConstCircularBuffer {}

pub struct ConstCircularBuffer<const CAPACITY: usize> {
    // pointer to the start of the buffer
    start_ptr: *mut u8,
    // pointer to the end of the buffer
    end_ptr: *mut u8,
    // read pointer
    read_ptr: *mut u8,
    // write pointer
    write_ptr: *mut u8,
    // our storage
    _buffer: [u8; CAPACITY],
}

impl<const CAPACITY: usize> ConstCircularBuffer<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            start_ptr: core::ptr::null_mut(),
            end_ptr: core::ptr::null_mut(),
            read_ptr: core::ptr::null_mut(),
            write_ptr: core::ptr::null_mut(),
            _buffer: [0; CAPACITY],
        }
    }

    pub fn init(&mut self) {
        // now that the buffer has been placed in static memory, we must wire up pointers
        let start_ptr = self._buffer.as_mut_ptr();
        let end_ptr = unsafe { self.start_ptr.add(CAPACITY as usize) };
        self.start_ptr = start_ptr;
        self.end_ptr = end_ptr;
        self.read_ptr = start_ptr;
        self.write_ptr = start_ptr;
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        // get the offset between the read and write pointers
        let offset = unsafe { self.write_ptr.offset_from(self.read_ptr) };
        // if the offset is less then 0 the write pointer has wrapped around
        let len = if offset < 0 {
            CAPACITY as isize + offset
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
        CAPACITY - self.len() - 1
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        // if free space is zero
        self.free_space() == 0
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        // if length is zero
        self.len() == 0
    }

    #[inline(always)]
    pub fn reset(&mut self) -> usize {
        // reset pointers to the start
        self.read_ptr = self.start_ptr;
        self.write_ptr = self.start_ptr;
        // return the capacity
        return CAPACITY - 1;
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Result<u8> {
        // is there anything to read
        if self.read_ptr == self.write_ptr {
            return Err(BufferError::BufferEmpty);
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
        Ok(byte)
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) -> Result<()> {
        // get the next write position
        let mut next_write_ptr = unsafe { self.write_ptr.add(1) };
        // check for wrapping
        if next_write_ptr == self.end_ptr {
            next_write_ptr = self.start_ptr;
        }
        // if the next write position equals the read position we are full
        if next_write_ptr == self.read_ptr {
            return Err(BufferError::InsufficientSpace);
        }
        // write the byte
        unsafe { self.write_ptr.write_volatile(byte) };
        // update the write pointer
        self.write_ptr = next_write_ptr;
        // everything is fine
        Ok(())
    }

    #[inline(always)]
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<usize> {
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

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        // first see if it will fit
        if self.is_full() {
            return Err(BufferError::InsufficientSpace);
        }
        // get the length
        let bytes_len = bytes.len();
        // get the free space
        let free_space = self.free_space();
        // figure out how much we can write
        let bytes_written = if bytes_len <= free_space {
            bytes_len
        } else {
            free_space
        };

        // get the length from the write pointer to the end
        let space_to_end = unsafe { self.end_ptr.offset_from(self.write_ptr) as usize };
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = core::cmp::min(bytes_written, space_to_end);
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
        let second_copy_len = bytes_written - first_copy_len;
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
        Ok(bytes_written)
    }
}

impl<const CAPACITY: usize> fmt::Write for ConstCircularBuffer<CAPACITY> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}
