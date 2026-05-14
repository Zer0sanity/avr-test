use crate::BufferHandle;
use core::task::Waker;

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

#[derive(Debug)]
pub enum DriverError {
    MissingDriver,
    MissingGlobalState,
    MissingGlobalBuffer,
    MissingFutureBuffer,
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}

// Local Variables:
// jinx-local-words: "packetizer packetizing waker"
// End:
