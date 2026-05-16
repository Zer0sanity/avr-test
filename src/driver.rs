use crate::BufferHandle;
use core::{fmt, task::Waker};

pub struct TxState {
    // store the buffer handle to our backing memory
    pub buffer: Option<BufferHandle>,
    // result set by the isr when transmit is complete or errors encountered
    pub result: Option<Result<(), DriverError>>,
    // waker to notify we're free to transmit
    pub waker: Option<Waker>,
}

impl TxState {
    pub fn new(buffer: BufferHandle) -> Self {
        Self {
            buffer: Some(buffer),
            result: None,
            waker: None,
        }
    }

    pub fn error(buffer: BufferHandle, error: DriverError) -> Self {
        Self {
            buffer: Some(buffer),
            result: Some(Err(error)),
            waker: None,
        }
    }
}

pub struct RxState {
    // store the buffer handle to our backing memory
    pub buffer: Option<BufferHandle>,
    // waker to notify data is available to read
    pub waker: Option<Waker>,
    // error status
    pub error: Option<DriverError>,
}

impl RxState {
    pub fn new(buffer: BufferHandle) -> Self {
        Self {
            buffer: Some(buffer),
            waker: None,
            error: None,
        }
    }
}

#[derive(Debug)]
pub enum DriverError {
    MissingDriver,
    MissingGlobalState,
    MissingGlobalBuffer,
    MissingFutureBuffer,
    BufferEmpty,
    InsufficientSpace,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let txt = match self {
            DriverError::MissingDriver => "MissingDriver",
            DriverError::MissingGlobalState => "MissingGlobalState",
            DriverError::MissingGlobalBuffer => "MissingGlobalBuffer",
            DriverError::MissingFutureBuffer => "MissingFutureBuffer",
            DriverError::BufferEmpty => "BufferEmpty",
            DriverError::InsufficientSpace => "InsufficientSpace",
        };
        write!(f, "{}", txt)
    }
}

pub trait Driver {
    type RxFuture: Future<Output = Result<BufferHandle, DriverError>>;
    type TxFuture: Future<Output = Result<BufferHandle, DriverError>>;

    fn init(&mut self, buffer_handle: BufferHandle);
    fn read(&mut self, buffer_handle: BufferHandle) -> Self::RxFuture;
    fn write(&mut self, buffer_handle: BufferHandle) -> Self::TxFuture;
}

// Local Variables:
// jinx-local-words: "packetizer packetizing waker"
// End:
