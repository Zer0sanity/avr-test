use core::{error::Error, fmt, mem::transmute, slice::Iter, slice::IterMut, task::Waker};

use crate::{BufferAllocator, BufferHandle};

unsafe impl Sync for TxState {}
unsafe impl Send for TxState {}

pub struct TxState {
    _buffer: BufferHandle,
    iter: Iter<'static, u8>,
}

impl TxState {
    pub fn new(buffer: BufferHandle) -> Self {
        let iter = unsafe {
            transmute::<Iter<'_, u8>, Iter<'static, u8>>(
                buffer.slice[..buffer.length() as usize].iter(),
            )
        };

        Self {
            _buffer: buffer,
            iter,
        }
    }
}

impl Iterator for TxState {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().copied()
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
    // iterator for writing
    write_iter: IterMut<'static, u8>,
    // current read position
    read_iter: Iter<'static, u8>,
    // number bytes in buffer
    len: u8,
    // waker to notify data is available to read
    pub waker: Option<Waker>,
    // store handle to buffer and our backing memory for receiving
    buffer: BufferHandle,
}

impl RxState {
    pub fn new(buffer: BufferHandle) -> Self {
        // get a write iterator
        let write_iter =
            unsafe { transmute::<IterMut<'_, u8>, IterMut<'static, u8>>(buffer.slice.iter_mut()) };
        // get a read iterator
        let read_iter =
            unsafe { transmute::<Iter<'_, u8>, Iter<'static, u8>>(buffer.slice.iter()) };
        // setup self
        Self {
            write_iter,
            read_iter,
            len: 0,
            waker: None,
            buffer,
        }
    }

    pub fn is_full(&self) -> bool {
        self.len == self.buffer.slice.len() as u8
    }

    pub fn write_byte(&mut self, byte: u8) {
        // do we need to rollover
        if self.write_iter.len() == 0 {
            self.write_iter = unsafe {
                transmute::<IterMut<'_, u8>, IterMut<'static, u8>>(self.buffer.slice.iter_mut())
            };
        }
        // write the byte
        self.write_iter.next().map(|slot| {
            *slot = byte;
        });
        // update the length
        self.len += 1;
        // wake the waker for anyone who's listening
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        // is there anything to read
        if self.len > 0 {
            return None;
        }
        // do we need to rollover
        if self.read_iter.len() == 0 {
            self.read_iter =
                unsafe { transmute::<Iter<'_, u8>, Iter<'static, u8>>(self.buffer.slice.iter()) };
        }
        // update the length
        self.len -= 1;
        // return the next byte
        self.read_iter.next().copied()
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}

// Local Variables:
// jinx-local-words: "packetizer packetizing"
// End:
