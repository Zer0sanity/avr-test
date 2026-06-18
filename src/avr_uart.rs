use core::{
    cell::RefCell,
    ptr,
    task::{Context, Poll, Waker},
};

use avr_device::{at90can128, interrupt::Mutex};
use avr_hal_generic::port::mode::{Floating, Input, Output, PullUp};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use crate::{
    ReadError, ReadStatus,
    const_circular_buffer::ConstCircularBuffer,
    hal::{PD4, PD7, PG0, PG3, PG4, Pin},
};

use at90can128::USART1;

// struct for reading the usart
pub struct UsartReader<USART, CTS, const BUFFER_CAPACITY: usize> {
    usart: USART,
    cts: Pin<Output, CTS>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
    waker: Option<Waker>,
}

impl UsartReader<USART1, PG3, BUFFER_CAPACITY> {
    // deassert when 25% available
    const DEASSERT_THRESHOLD: usize = BUFFER_CAPACITY / 4;
    // assert when 50% available
    const REASSERT_THRESHOLD: usize = BUFFER_CAPACITY / 2;
    // const initializer so this can be created in static ram
    pub const fn new() -> Self {
        unsafe {
            Self {
                usart: ptr::read(ptr::dangling()),
                cts: ptr::read(ptr::dangling()),
                waker: None,
                buffer: ConstCircularBuffer::new(),
            }
        }
    }
}

#[avr_device::interrupt(at90can128)]
fn USART1_RX() {
    // forge a token. this is safe ONLY because we are in an ISR
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the reader
    let mut reader = USART1_READER.borrow(cs).borrow_mut();
    // read the byte from the hardware
    let byte = reader.usart.udr1().read().bits();
    // write the byte.
    _ = reader.buffer.write_byte(byte);
    // manage cts
    if reader.buffer.free_space() < UsartReader::DEASSERT_THRESHOLD {
        reader.cts.set_high();
    }
    // kick the waker if its set
    if let Some(waker) = reader.waker.take() {
        waker.wake();
    }
}

// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1ReaderHandle;

impl Usart1ReaderHandle {
    pub async fn read_to(&self, term: u8, mut buf: &mut [u8]) -> Result<ReadStatus, ReadError> {
        let mut total_len = 0;
        let status = loop {
            match self.try_read_to(term, &mut buf[total_len..]).await? {
                // haven't read terminator yet, update length and continue
                ReadStatus::Partial(len) => total_len += len,
                ReadStatus::Complete(len) => {
                    total_len += len;
                    break ReadStatus::Complete(total_len);
                }
                ReadStatus::BufferFull(len) => {
                    total_len += len;
                    break ReadStatus::BufferFull(total_len);
                }
            }
        };
        Ok(status)
    }

    pub async fn try_read_to(&self, term: u8, buf: &mut [u8]) -> Result<ReadStatus, ReadError> {
        // did we get called with a full buffer
        if buf.is_empty() {
            return Err(ReadError::DestinationEmpty);
        }
        // wait until we can read
        if let Err(e) = Usart1CanRead.await {
            return Err(e);
        }
        // go interrupt free while we poll bytes
        avr_device::interrupt::free(|cs| {
            // get the reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            // walk the buffer
            let result = reader.buffer.read_to(term, buf);
            // manage cts
            if UsartReader::REASSERT_THRESHOLD < reader.buffer.free_space() {
                reader.cts.set_low();
            }
            // return the result
            result
        })
    }
}

impl ErrorType for Usart1ReaderHandle {
    type Error = ReadError;
}

impl Read for Usart1ReaderHandle {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.is_empty() {
            return Err(ReadError::DestinationEmpty);
        }
        // wait until we can read
        if let Err(e) = Usart1CanRead.await {
            return Err(e);
        }
        // go interrupt free while we poll bytes
        avr_device::interrupt::free(|cs| {
            // get the reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            // read what we can
            let result = reader.buffer.read(buf);
            // manage cts
            if UsartReader::REASSERT_THRESHOLD < reader.buffer.free_space() {
                reader.cts.set_low();
            }
            // return
            result
        })
    }
}

// a struct that implements the future trait that will wait for data available to be read from USART1_READER
struct Usart1CanRead;

impl Future for Usart1CanRead {
    type Output = Result<(), ReadError>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free for the check
        avr_device::interrupt::free(|cs| {
            // get the reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            // see if there is data available
            if reader.buffer.len() > 0 {
                return Poll::Ready(Ok(()));
            }
            // check if we are connected
            // let control = USART1_CONTROL.borrow(cs).borrow_mut();
            // if not connected return an error
            // if !control.sense.is_high() {
            //     return Poll::Ready(Err(ReadError::Disconnected));
            // }
            // else no data to read.  register the waker
            reader.waker = Some(cx.waker().clone());
            // return pending
            Poll::Pending
        })
    }
}

// struct for writing usart
pub struct UsartWriter<USART, RTS, const BUFFER_CAPACITY: usize> {
    usart: USART,
    rts: Pin<Input<Floating>, RTS>,
    flush_char_opt: Option<u8>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
    waker_opt: Option<Waker>,
    tx_idle: bool,
}

impl UsartWriter<USART1, PG4, BUFFER_CAPACITY> {
    pub const fn new() -> Self {
        unsafe {
            Self {
                usart: ptr::read(core::ptr::dangling()),
                rts: ptr::read(core::ptr::dangling()),
                flush_char_opt: None,
                buffer: ConstCircularBuffer::new(),
                waker_opt: None,
                tx_idle: true,
            }
        }
    }
}

#[avr_device::interrupt(at90can128)]
fn USART1_TX() {
    // forge a token. this is safe ONLY because we are in an ISR
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the writer
    let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
    // check if we can send
    let rts_ok = writer.rts.is_low();
    // if we can send try to read a byte from writer
    if rts_ok && let Ok(byte) = writer.buffer.read_byte() {
        // write it to the hardware
        writer.usart.udr1().write(|w| unsafe { w.bits(byte) });
    } else {
        // disable interrupts
        writer.usart.ucsr1b().write(|w| w.udrie1().clear_bit());
        // set the tx_idle flag
        writer.tx_idle = true;
    }
    // are we returning a byte and the waker is set wake it
    if let Some(waker) = writer.waker_opt.take() {
        waker.wake();
    }
}

// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1WriterHandle;
impl ErrorType for Usart1WriterHandle {
    type Error = embedded_io::ErrorKind;
}

impl Write for Usart1WriterHandle {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // did we get called with a empty buffer
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until we can write
        if let Err(kind) = Usart1CanWrite.await {
            return Err(kind);
        }
        // go interrupt free while we write bytes
        let write_result = avr_device::interrupt::free(|cs| {
            // get the writer
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            // write the data
            let write_result = writer.buffer.write(buf);
            // see if we need to enable interrupts
            if writer.tx_idle {
                // enable interrupts
                writer.usart.ucsr1b().write(|w| w.udrie1().set_bit());
                // clear the tx_idle flag
                writer.tx_idle = false;
            }
            // return the write result
            write_result
        });
        // map the error
        write_result.map_err(|e| e.into())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        // going to need another future to wait until the buffer is empty.
        // wait until we can write
        if let Err(kind) = Usart1CanWrite.await {
            return Err(kind);
        }
        // go interrupt free while we write the flush character
        avr_device::interrupt::free(|cs| {
            // get the writer
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            // write the flush character
            writer
                .flush_char_opt
                .map(|flush_char| writer.buffer.write_byte(flush_char));
            // see if we need to enable interrupts
            if writer.tx_idle {
                // enable interrupts
                writer.usart.ucsr1b().write(|w| w.udrie1().set_bit());
                // clear the tx_idle flag
                writer.tx_idle = false;
            }
        });
        // wait until the buffer is empty
        return Usart1TxBufferEmpty.await;
    }
}

// a struct that implements the future trait that will wait for space available in USART1_WRITER buffer
struct Usart1CanWrite;

impl Future for Usart1CanWrite {
    type Output = Result<(), embedded_io::ErrorKind>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free for the check
        avr_device::interrupt::free(|cs| {
            // get the writer
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            // see if there is space to write
            if !writer.buffer.is_full() {
                return Poll::Ready(Ok(()));
            }
            // // check if we are connected
            // let control = USART1_CONTROL.borrow(cs).borrow_mut();
            // // if not connected return an error
            // if !control.sense.is_high() {
            //     return Poll::Ready(Err(ErrorKind::NotConnected));
            // }
            // else no space available to write.  register the waker
            writer.waker_opt = Some(cx.waker().clone());
            // return pending
            Poll::Pending
        })
    }
}

struct Usart1TxBufferEmpty;

impl Future for Usart1TxBufferEmpty {
    type Output = Result<(), embedded_io::ErrorKind>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free for the check
        avr_device::interrupt::free(|cs| {
            // get the writer
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            // see if its empty
            if writer.buffer.is_empty() {
                return Poll::Ready(Ok(()));
            }
            // // check if we are connected
            // let control = USART1_CONTROL.borrow(cs).borrow_mut();
            // // if not connected return an error
            // if !control.sense.is_high() {
            //     return Poll::Ready(Err(ErrorKind::NotConnected));
            // }
            // else no space available to write.  register the waker
            writer.waker_opt = Some(cx.waker().clone());
            // return pending
            Poll::Pending
        })
    }
}

// struct to hold shared control ports
pub struct UsartControlSignals<SENSE, RESET, DEFAULTS> {
    sense: Pin<Input<PullUp>, SENSE>,
    reset: Pin<Output, RESET>,
    defaults: Pin<Output, DEFAULTS>,
}

impl UsartControlSignals<PD7, PD4, PG0> {
    pub const fn new() -> Self {
        unsafe {
            Self {
                sense: ptr::read(ptr::dangling()),
                reset: ptr::read(ptr::dangling()),
                defaults: ptr::read(ptr::dangling()),
            }
        }
    }
}

// define buffer size for interrupt buffers
const BUFFER_CAPACITY: usize = 64;
// static shared usart1 reader
static USART1_READER: Mutex<RefCell<UsartReader<USART1, PG3, BUFFER_CAPACITY>>> =
    Mutex::new(RefCell::new(UsartReader::new()));
// static shared usart1 writer
static USART1_WRITER: Mutex<RefCell<UsartWriter<USART1, PG4, BUFFER_CAPACITY>>> =
    Mutex::new(RefCell::new(UsartWriter::new()));
// control signals for usart 1 (xpico and matchport)
static USART1_CONTROL: Mutex<RefCell<UsartControlSignals<PD7, PD4, PG0>>> =
    Mutex::new(RefCell::new(UsartControlSignals::new()));

// base struct for ft240x
pub struct AvrUart;

impl AvrUart {
    pub fn init(
        usart: at90can128::USART1,
        _cts: Pin<Output, PG3>,
        _rts: Pin<Input<Floating>, PG4>,
        _sense: Pin<Input<PullUp>, PD7>,
        _reset: Pin<Output, PD4>,
        _defaults: Pin<Output, PG0>,
    ) -> (Usart1ReaderHandle, Usart1WriterHandle) {
        // control register a
        usart.ucsr1a().modify(|_, w| {
            // disable double speed
            w.u2x1().clear_bit();
            // disable multi-Processor communication mode
            w.mpcm1().clear_bit()
        });
        // control register b
        usart.ucsr1b().modify(|_, w| {
            // enable rx complete interrupt enable (we will enable it when the reader is initialized)
            w.rxcie1().set_bit();
            // disable tx complete interrupt enable
            w.txcie1().clear_bit();
            // disable data register empty interrupt enable (we will enable this later when we transmit data)
            w.udrie1().clear_bit();
            // receiver enable
            w.rxen1().set_bit();
            // transmitter enable
            w.txen1().set_bit();
            // character size.  ucsr1b_ucsz12, ucsr1c_ucsz11 and ucsr1c_ucsz10 define the character size.  for 8-bit ucsz12 = 0, ucsz11 = 1, and ucsz10 = 1
            w.ucsz12().clear_bit()
        });
        // control register c
        usart.ucsr1c().modify(|_, w| {
            // async mode
            w.umsel1().usart_async();
            // parity mode 1 (0,0 = no parity)
            w.upm1().disabled();
            // stop bit select (0 = 1 stop-bit)
            w.usbs1().stop1();
            // character size.  ucsr1b_ucsz12, ucsr1c_ucsz11 and ucsr1c_ucsz10 define the character size.  for 8-bit ucsz12 = 0, ucsz11 = 1, and ucsz10 = 1
            w.ucsz1().chr8();
            // clock polarity (0 = transmit on rising edge & sample on falling, 1 = transmit on falling edge & sample on rising
            w.ucpol1().rising_edge()
        });
        // set the baud-rate (0x0f = 57.6k in current configuration )
        usart.ubrr1().modify(|_, w| w.set(0x0f));
        // return
        (Usart1ReaderHandle, Usart1WriterHandle)
    }
}
