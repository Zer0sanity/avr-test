use core::{error::Error, fmt, mem::transmute, slice::Iter, slice::IterMut, task::Waker};

use crate::{BufferAllocator, BufferHandle};

unsafe impl Sync for TxState {}
unsafe impl Send for TxState {}

pub struct TxState {
    // current position we are reading from
    ptr: *mut u8,
    // number of bytes in the buffer
    len: u8,
    // store the buffer handle to our backing memory
    _buffer: Option<BufferHandle>,
}

impl TxState {
    pub fn new(buffer: BufferHandle, len: u8) -> Self {
        Self {
            ptr: buffer.ptr,
            len,
            _buffer: Some(buffer),
        }
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.len == 0 {
            return None;
        }
        // read a byte
        let byte = unsafe { self.ptr.read_volatile() };
        // update the pointer
        self.ptr = unsafe { self.ptr.add(1) };
        // update the length
        self.len -= 1;
        // return the next byte
        Some(byte)
    }
}

unsafe impl Sync for RxState {}
unsafe impl Send for RxState {}

#[derive(Clone)]
pub enum RxStatus {
    Ready,
    Done,
}

#[derive(Debug)]
pub enum RxError {
    Overflow,
}

impl fmt::Display for RxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RxError")
    }
}

impl Error for RxError {}

pub trait Packetizer {
    fn receive_byte(&self, byte: u8) -> RxStatus;
}

pub struct CrPacketizer;
impl Packetizer for CrPacketizer {
    fn receive_byte(&self, byte: u8) -> RxStatus {
        if byte != 0x0a {
            RxStatus::Done
        } else {
            RxStatus::Ready
        }
    }
}

pub struct RxState {
    // pointer to the start of the buffer
    ptr: *mut u8,
    // write index
    write_idx: u8,
    // read index
    read_idx: u8,
    // number of bytes in the buffer
    len: u8,
    // buffer capacity
    capacity: u8,
    // mask used to rollover
    rollover_mask: u8,
    // store the buffer handle to our backing memory
    _buffer: Option<BufferHandle>,
    // waker to notify data is available to read
    pub waker: Option<Waker>,
}

impl RxState {
    pub fn new(buffer: BufferHandle) -> Self {
        Self {
            ptr: buffer.ptr,
            write_idx: 0,
            read_idx: 0,
            len: 0,
            capacity: buffer.len,
            rollover_mask: buffer.len - 1,
            waker: None,
            _buffer: Some(buffer),
        }
    }

    #[inline(always)]
    pub fn free_space(&self) -> u8 {
        self.capacity - self.len
    }

    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) {
        // write the byte
        unsafe { self.ptr.add(self.write_idx as usize).write_volatile(byte) };
        // update the write index
        self.write_idx = (self.write_idx + 1) & self.rollover_mask;
        // update the length
        self.len += 1;
        // wake the waker for anyone who's listening
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    #[inline(always)]
    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.len == 0 {
            return None;
        }
        // read a byte
        let byte = unsafe { self.ptr.add(self.read_idx as usize).read_volatile() };
        // update the read index
        self.read_idx = (self.read_idx + 1) & self.rollover_mask;
        // update the length
        self.len -= 1;
        // return the next byte
        Some(byte)
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}

// Local Variables:
// jinx-local-words: "packetizer packetizing waker"
// End:
