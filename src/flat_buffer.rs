use core::{
    cmp::min,
    fmt::{self, Error, Write},
    slice,
};

use crate::BufferError;

type Result<T> = core::result::Result<T, BufferError>;

// because pointers can't be sent between threads safely
unsafe impl Send for FlatBuffer<'_> {}
unsafe impl Sync for FlatBuffer<'_> {}

pub struct FlatBuffer<'a> {
    // read position
    r_pos: usize,
    // write position
    w_pos: usize,
    // tracked length
    len: usize,
    // reference to storage
    buf: &'a mut [u8],
}

impl FlatBuffer<'_> {
    pub fn new(ptr: *mut u8, capacity: usize) -> Self {
        Self {
            r_pos: 0,
            w_pos: 0,
            len: 0,
            buf: unsafe { slice::from_raw_parts_mut(ptr, capacity) },
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn free_space(&self) -> usize {
        self.buf.len() - self.len
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.len == self.buf.len()
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
        return self.buf.len();
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Result<u8> {
        // is there anything to read
        if self.is_empty() {
            return Err(BufferError::BufferEmpty);
        }
        // read a byte
        let byte = self.buf[self.r_pos];
        // update the read position
        self.r_pos += 1;
        // update the length
        self.len -= 1;
        // return the next byte
        Ok(byte)
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) -> Result<()> {
        // are we full
        if self.is_full() {
            return Err(BufferError::InsufficientSpace);
        }
        // write the byte
        self.buf[self.w_pos] = byte;
        // update the write position
        self.w_pos += 1;
        // update the length
        self.len += 1;
        // everything is fine
        Ok(())
    }

    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        // can we write any
        if self.is_full() {
            return Err(BufferError::InsufficientSpace);
        }
        // figure out how much we can write
        let len = min(bytes.len(), self.free_space());
        let end = self.w_pos + len;
        // preform the copy
        self.buf[self.w_pos..end].copy_from_slice(&bytes[..len]);
        // update the write position
        self.w_pos += len;
        // update length
        self.len += len;
        // return
        Ok(len)
    }

    #[inline(always)]
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<usize> {
        // cache the length
        let len = bytes.len();
        // first see if it will fit
        if self.free_space() < len {
            return Err(BufferError::InsufficientSpace);
        }
        let end = self.w_pos + len;
        // preform the copy
        self.buf[self.w_pos..end].copy_from_slice(bytes);
        // update the write position
        self.w_pos += len;
        // update length
        self.len += len;
        // return
        Ok(len)
    }
}

impl fmt::Write for FlatBuffer<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // just use the buffer handle write
        self.write_all(s.as_bytes())
            .map(|_| ())
            .map_err(|_| fmt::Error)
    }
}

impl AsRef<[u8]> for FlatBuffer<'_> {
    fn as_ref(&self) -> &[u8] {
        let start = self.r_pos;
        let end = start + self.len();
        &self.buf[start..end]
    }
}

impl AsMut<[u8]> for FlatBuffer<'_> {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.w_pos..]
    }
}
