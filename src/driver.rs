use crate::{BufferHandle, CircularBuffer, FlatBuffer};
use core::{fmt, task::Waker};

pub struct TxState {
    // store the buffer handle to our backing memory
    pub buffer: FlatBuffer,
    // result set by the isr when transmit is complete or errors encountered
    pub result: Option<Result<(), DriverError>>,
    // waker to notify we're free to transmit
    pub waker: Option<Waker>,
}

impl TxState {
    pub fn new(buffer: FlatBuffer) -> Self {
        Self {
            buffer: buffer,
            result: None,
            waker: None,
        }
    }

    pub fn error(buffer: FlatBuffer, error: DriverError) -> Self {
        Self {
            buffer: buffer,
            result: Some(Err(error)),
            waker: None,
        }
    }
}

pub struct RxState {
    // store the buffer handle to our backing memory
    pub buffer: CircularBuffer,
    // waker to notify data is available to read
    pub waker: Option<Waker>,
    // error status
    pub error: Option<DriverError>,
}

impl RxState {
    pub fn new(buffer: CircularBuffer) -> Self {
        Self {
            buffer: buffer.into(),
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
    type RxFuture: Future<Output = Result<FlatBuffer, DriverError>>;
    type TxFuture: Future<Output = Result<FlatBuffer, DriverError>>;

    fn init(&mut self, buffer_handle: CircularBuffer);
    fn read(&mut self, buffer_handle: FlatBuffer) -> Self::RxFuture;
    fn write(&mut self, buffer_handle: FlatBuffer) -> Self::TxFuture;
}

// Local Variables:
// jinx-local-words: "packetizer packetizing waker"
// End:
