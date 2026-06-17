use core::{cmp::min, fmt};

use crate::{BufferError, ReadError, ReadStatus};

// because pointers can't be sent between threads safely
unsafe impl<const CAPACITY: usize> Send for ConstCircularBuffer<CAPACITY> {}
// unsafe impl Sync for ConstCircularBuffer {}

pub struct ConstCircularBuffer<const CAPACITY: usize> {
    // read position
    r_pos: usize,
    // write position
    w_pos: usize,
    // tracked length
    len: usize,
    // our storage
    buf: [u8; CAPACITY],
}

impl<const CAPACITY: usize> ConstCircularBuffer<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            r_pos: 0,
            w_pos: 0,
            len: 0,
            buf: [0; CAPACITY],
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn free_space(&self) -> usize {
        CAPACITY - self.len
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.len == CAPACITY
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn reset(&mut self) -> usize {
        self.r_pos = 0;
        self.w_pos = 0;
        self.len = 0;
        return CAPACITY;
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Result<u8, ReadError> {
        // is there anything to read
        if self.is_empty() {
            return Err(ReadError::SourceEmpty);
        }
        // read a byte
        let byte = self.buf[self.r_pos];
        // update the read position
        self.r_pos += 1;
        // check for wrapping
        if self.r_pos == CAPACITY {
            self.r_pos = 0;
        }
        // update the length
        self.len -= 1;
        // return the next byte
        Ok(byte)
    }

    #[inline(always)]
    pub fn read(&mut self, mut dest: &mut [u8]) -> Result<usize, ReadError> {
        // is the destination empty
        if dest.is_empty() {
            return Err(ReadError::DestinationEmpty);
        }
        // do we have anything to read
        if self.is_empty() {
            return Err(ReadError::SourceEmpty);
        }
        // get how much we can read
        let len = min(dest.len(), self.len());
        // get the length from the read position to the end
        let len_to_end = CAPACITY - self.r_pos;
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = min(len, len_to_end);
        let first_copy_end = self.r_pos + first_copy_len;
        // preform the copy
        dest[..first_copy_len].copy_from_slice(&self.buf[self.r_pos..first_copy_end]);
        // figure out if we have a second half to write
        let second_copy_len = len - first_copy_len;
        if second_copy_len > 0 {
            // preform the copy
            dest[first_copy_len..len].copy_from_slice(&self.buf[..second_copy_len]);
        }
        // update the write position
        self.r_pos += len;
        // check for wrapping
        if self.r_pos >= CAPACITY {
            self.r_pos -= CAPACITY;
        }
        // update length
        self.len -= len;
        // return
        Ok(len)
    }

    #[inline(always)]
    pub fn try_read_to(&mut self, term: u8, mut dest: &mut [u8]) -> Result<ReadStatus, ReadError> {
        // is the destination empty
        if dest.is_empty() {
            return Err(ReadError::DestinationEmpty);
        }
        // do we have anything to read
        if self.is_empty() {
            return Err(ReadError::SourceEmpty);
        }
        // get how much we can read
        let len_allowed = min(dest.len(), self.len());
        // loop through the allowable read length and search for the terminator
        let mut idx = self.r_pos;
        let mut len_to_term = 0;
        let (term_found, len) = loop {
            // increment the length to the terminator
            len_to_term += 1;
            // check for the terminator
            if self.buf[self.r_pos] == term {
                break (true, len_to_term);
            }
            // are we at the allowed length
            if len_to_term == len_allowed {
                break (false, len_allowed);
            }
            // increment the index
            idx += 1;
            // check for rollover
            if idx == CAPACITY {
                idx = 0;
            }
        };
        // get the length from the read position to the end
        let len_to_end = CAPACITY - self.r_pos;
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = min(len, len_to_end);
        let first_copy_end = self.r_pos + first_copy_len;
        // preform the copy
        dest[..first_copy_len].copy_from_slice(&self.buf[self.r_pos..first_copy_end]);
        // figure out if we have a second half to write
        let second_copy_len = len - first_copy_len;
        if second_copy_len > 0 {
            // preform the copy
            dest[first_copy_len..len].copy_from_slice(&self.buf[..second_copy_len]);
        }
        // update the write position
        self.r_pos += len;
        // check for wrapping
        if self.r_pos >= CAPACITY {
            self.r_pos -= CAPACITY;
        }
        // update length
        self.len -= len;
        // return
        if term_found {
            Ok(ReadStatus::Complete(len))
        } else {
            Ok(ReadStatus::Partial(len))
        }
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) -> Result<(), BufferError> {
        // are we full
        if self.is_full() {
            return Err(BufferError::InsufficientSpace);
        }
        // write the byte
        self.buf[self.w_pos] = byte;
        // update the write position
        self.w_pos += 1;
        // check for wrapping
        if self.w_pos == CAPACITY {
            self.w_pos = 0;
        }
        // update the length
        self.len += 1;
        // everything is fine
        Ok(())
    }

    #[inline(always)]
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<usize, BufferError> {
        // get the length
        let len = bytes.len();
        // first see if it will fit
        if self.free_space() < len {
            return Err(BufferError::InsufficientSpace);
        }
        // get the length from the write position to the end
        let len_to_end = CAPACITY - self.w_pos;
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = min(len, len_to_end);
        let first_copy_end = self.w_pos + first_copy_len;
        // preform the copy
        self.buf[self.w_pos..first_copy_end].copy_from_slice(&bytes[..first_copy_len]);
        // figure out if we have a second half to write
        let second_copy_len = len - first_copy_len;
        if second_copy_len > 0 {
            // preform the copy
            self.buf[..second_copy_len].copy_from_slice(&bytes[first_copy_len..]);
        }
        // update the write position
        self.w_pos += len;
        // check for wrapping
        if self.w_pos >= CAPACITY {
            self.w_pos -= CAPACITY;
        }
        // update length
        self.len += len;
        // return
        Ok(len)
    }

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, BufferError> {
        // can we write any
        if self.is_full() {
            return Err(BufferError::InsufficientSpace);
        }
        // figure out how much we can write
        let can_copy_len = min(bytes.len(), self.free_space());
        // get the length from the write position to the end
        let to_end_len = CAPACITY - self.w_pos;
        // figure out if we can copy the whole thing or just to the end of the buffer
        let first_copy_len = min(can_copy_len, to_end_len);
        let first_copy_end = self.w_pos + first_copy_len;
        // preform the copy
        self.buf[self.w_pos..first_copy_end].copy_from_slice(&bytes[..first_copy_len]);
        // figure out if we have a second half to write
        let second_copy_len = can_copy_len - first_copy_len;
        if second_copy_len > 0 {
            // preform the copy
            self.buf[..second_copy_len].copy_from_slice(&bytes[first_copy_len..]);
        }
        // update the write position
        self.w_pos += can_copy_len;
        // check for wrapping
        if self.w_pos >= CAPACITY {
            self.w_pos -= CAPACITY;
        }
        // update length
        self.len += can_copy_len;
        // return
        Ok(can_copy_len)
    }
}

impl<const CAPACITY: usize> fmt::Write for ConstCircularBuffer<CAPACITY> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        _ = self.write(s.as_bytes())?;
        Ok(())
    }
}
