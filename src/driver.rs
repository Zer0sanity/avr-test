use core::{error::Error, fmt, mem::transmute, slice::Iter, slice::IterMut, task::Waker};

use crate::{BufferAllocator, BufferHandle};

unsafe impl Sync for TxState {}
unsafe impl Send for TxState {}

pub struct TxState {
    // store the buffer handle to our backing memory
    pub buffer: Option<BufferHandle>,
    // waker to notify we're free to transmit
    pub waker: Option<Waker>,
}

impl TxState {
    pub fn new(buffer: BufferHandle) -> Self {
        Self {
            buffer: Some(buffer),
            waker: None,
        }
    }
}

unsafe impl Sync for RxState {}
unsafe impl Send for RxState {}

pub struct RxState {
    // store the buffer handle to our backing memory
    pub buffer: Option<BufferHandle>,
    // waker to notify data is available to read
    pub waker: Option<Waker>,
}

impl RxState {
    pub fn new(buffer: BufferHandle) -> Self {
        Self {
            buffer: Some(buffer),
            waker: None,
        }
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}

// Local Variables:
// jinx-local-words: "packetizer packetizing waker"
// End:
