use core::{
    cell::RefCell,
    marker::PhantomData,
    task::{Context, Poll, Waker},
};

use avr_device::{at90can128, interrupt::Mutex};
use avr_hal_generic::port::{
    PinOps,
    mode::{Floating, Input, Output, PullUp},
};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use crate::{
    const_circular_buffer::ConstCircularBuffer,
    hal::{PD4, PD7, PG0, PG3, PG4, Pin},
};

use at90can128::USART1;

// struct for reading the usart
pub struct UsartReader<USART, CTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    cts_opt: Option<Pin<Output, CTS>>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
    waker: Option<Waker>,
}

impl UsartReader<USART1, PG3, BUFFER_CAPACITY> {
    // deassert when 25% available
    const DEASSERT_THRESHOLD: usize = BUFFER_CAPACITY / 4;
    // assert when 50% available
    const ASSERT_THRESHOLD: usize = BUFFER_CAPACITY / 2;
    // const initializer so this can be created in static ram
    pub const fn new() -> Self {
        Self {
            _usart: PhantomData,
            cts_opt: None,
            waker: None,
            buffer: ConstCircularBuffer::new(),
        }
    }
    // initializer to set the cts signal and initialize circular buffer pointers
    pub fn init(&mut self, cts_opt: Option<Pin<Output, PG3>>) {
        // set the cts pin option
        self.cts_opt = cts_opt;
        // initialize the buffer
        self.buffer.init();
        // enable rx interrupts
        unsafe {
            (*USART1::ptr())
                .ucsr1b()
                .modify(|_, w| w.rxcie1().set_bit());
        }
    }
    // write a byte to the buffer, handle the cts line, kick the waker if its set
    #[inline(always)]
    pub fn write_byte(&mut self, byte: u8) {
        // get the free space
        let free_space = self.buffer.free_space();
        // write the byte.
        if free_space > 0 {
            _ = self.buffer.write_byte(byte);
        }
        // manage cts
        if free_space <= Self::DEASSERT_THRESHOLD {
            self.cts_opt.as_mut().map(|cts| cts.set_low());
        }
        // kick the waker if its set
        if let Some(waker) = self.waker.take() {
            waker.wake();
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
    let byte = unsafe { (*USART1::ptr()).udr1().read().bits() };
    // write it to the reader
    reader.write_byte(byte);
}

// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1ReaderHandle;
impl ErrorType for Usart1ReaderHandle {
    type Error = embedded_io::ErrorKind;
}

impl Read for Usart1ReaderHandle {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // did we get called with a full buffer
        if buf.len() == 0 {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until we can read
        if let Err(kind) = Usart1DataAvailable.await {
            return Err(kind);
        }
        // initialize the number of bytes read
        let mut bytes = 0;
        // go interrupt free while we poll bytes
        avr_device::interrupt::free(|cs| {
            // get the reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            // walk the buffer
            for slot in buf.iter_mut() {
                // we know we can read at least one, from the data available await above
                if let Ok(byte) = reader.buffer.read_byte() {
                    *slot = byte;
                    // increment the number of bytes read
                    bytes += 1;
                } else {
                    break;
                }
            }
        });
        // here we read at least one so, return the number of bytes read
        Ok(bytes)
    }
}

// a struct that implements the future trait that will wait for data available to be read from USART1_READER
struct Usart1DataAvailable;

impl Future for Usart1DataAvailable {
    type Output = Result<(), embedded_io::ErrorKind>;

    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free for the check
        avr_device::interrupt::free(|cs| {
            // get the reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            // see if there is data available
            if reader.buffer.len() > 0 {
                return Poll::Ready(Ok(()));
            }
            // we can check if we are connected and throw an error
            let mut control = USART1_CONTROL.borrow(cs).borrow_mut();
            let connected = if let Some(sense) = control.sense_opt.as_mut() {
                sense.is_high()
            } else {
                true
            };
            // if not connected return an error
            if !connected {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // else no data to read.  register the waker
            reader.waker = Some(cx.waker().clone());
            // return pending
            Poll::Pending
        })
    }
}

// struct for writing usart
pub struct UsartWriter<USART, RTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    rts_opt: Option<Pin<Input<Floating>, RTS>>,
    flush_char: Option<u8>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
}

impl UsartWriter<USART1, PG4, BUFFER_CAPACITY> {
    pub const fn new() -> Self {
        Self {
            _usart: PhantomData,
            rts_opt: None,
            flush_char: None,
            buffer: ConstCircularBuffer::new(),
        }
    }
    pub fn init(&mut self, rts_opt: Option<Pin<Input<Floating>, PG4>>) {
        self.rts_opt = rts_opt;
        self.buffer.init();
    }

    #[inline(always)]
    pub fn write(&mut self, byte: u8) {
        unsafe {
            (*USART1::ptr()).udr1().as_ptr().write_volatile(byte);
        }
    }
}

// struct to hold shared control ports
pub struct UsartControlSignals<SENSE, RESET, DEFAULTS> {
    sense_opt: Option<Pin<Input<PullUp>, SENSE>>,
    reset_opt: Option<Pin<Output, RESET>>,
    defaults_opt: Option<Pin<Output, DEFAULTS>>,
}

impl UsartControlSignals<PD7, PD4, PG0> {
    pub const fn new() -> Self {
        Self {
            sense_opt: None,
            reset_opt: None,
            defaults_opt: None,
        }
    }
    pub fn init(
        &mut self,
        sense_opt: Option<Pin<Input<PullUp>, PD7>>,
        reset_opt: Option<Pin<Output, PD4>>,
        defaults_opt: Option<Pin<Output, PG0>>,
    ) {
        self.sense_opt = sense_opt;
        self.reset_opt = reset_opt;
        self.defaults_opt = defaults_opt;
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

// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1WriterHandle;
impl Usart1WriterHandle {
    pub fn write(&self, byte: u8) -> Result<(), ()> {
        // Enforce the critical section under the hood
        avr_device::interrupt::free(|cs| {
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            (*writer).write(byte);
        });
        Ok(())
    }
}

// base struct for ft240x
pub struct AvrUart<USART, CTS, RTS, SENSE, RESET, DEFAULTS>
where
    CTS: PinOps,
    RTS: PinOps,
    SENSE: PinOps,
    RESET: PinOps,
    DEFAULTS: PinOps,
{
    usart: USART,
    cts: Pin<Output, CTS>,
    rts: Pin<Input<Floating>, RTS>,
    sense: Pin<Input<PullUp>, SENSE>,
    reset: Pin<Output, RESET>,
    defaults: Pin<Output, DEFAULTS>,
}

impl AvrUart<USART1, PG3, PG4, PD7, PD4, PG0> {
    pub fn init(
        usart: at90can128::USART1,
        cts: Pin<Input<Floating>, PG3>,
        rts: Pin<Input<Floating>, PG4>,
        sense: Pin<Input<Floating>, PD7>,
        reset: Pin<Input<Floating>, PD4>,
        defaults: Pin<Input<Floating>, PG0>,
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
            // disable rx complete interrupt enable (we will enable it when the reader is initialized)
            w.rxcie1().clear_bit();
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

        let cts = cts.into_output();
        let rts = rts.into_floating_input();
        let sense = sense.into_pull_up_input();
        let reset = reset.into_output_high();
        let defaults = defaults.into_output_high();

        // put into static memory
        avr_device::interrupt::free(|cs| {
            // reader
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            reader.init(Some(cts));
            // writer
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            writer.init(Some(rts));
            // control pins
            let mut control = USART1_CONTROL.borrow(cs).borrow_mut();
            control.init(Some(sense), Some(reset), Some(defaults));
        });

        // return
        (Usart1ReaderHandle, Usart1WriterHandle)
    }
}
