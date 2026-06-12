pub mod io_bus_8;

use core::{
    cell::RefCell,
    marker::PhantomData,
    task::{Context, Poll, Waker},
};

use avr_device::{
    at90can128::{self, EXINT, PORTC},
    interrupt::Mutex,
};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use crate::{BufferError, FlatBuffer, ft240x::io_bus_8::IoBus8, interrupts::At90Can128Interrupts};

// global waker for transmitting and receiving
pub static TX_WAKER: Mutex<RefCell<Option<Waker>>> = Mutex::new(RefCell::new(None));
pub static RX_WAKER: Mutex<RefCell<Option<Waker>>> = Mutex::new(RefCell::new(None));

#[derive(Clone, Copy, PartialEq)]
enum BusState {
    Unknown,
    Input,
    Output,
}

// base struct for ft240x
pub struct Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
    bus: BUS,                           // port used write/read from FT240
    sense: Pin<Input<Floating>, SENSE>, // input to tell if usb is connected to host
    rxf: Pin<Input<Floating>, RXF>,     // input to tell when data can be read from the FT240.
    txe: Pin<Input<Floating>, TXE>,     // input to tell when the FT240 can accept data.
    rd: Pin<Output, RD>, // output to have the FT240 put a received byte from its FIFO to the data bus
    wr: Pin<Output, WR>, // output to have the FT240 read data byte from data bus to its transmit FIFO
    siwu: Pin<Output, SIWU>, // output to tell the FT240 to flush its transmit FIFO buffer to the PC
    state: BusState,
}

impl Ft240x<PORTC, PG2, PE6, PE5, PE4, PE7, PE2> {
    pub fn new(
        bus: PORTC,
        sense: Pin<Input<Floating>, PG2>,
        rxf: Pin<Input<Floating>, PE6>,
        txe: Pin<Input<Floating>, PE5>,
        rd: Pin<Input<Floating>, PE4>,
        wr: Pin<Input<Floating>, PE7>,
        siwu: Pin<Input<Floating>, PE2>,
    ) -> Self {
        // configure output pins
        let rd = pins.pe4.into_output().downgrade();
        let wr = pins.pe7.into_output().downgrade();
        let siwu = pins.pe2.into_output().downgrade();

        Self {
            bus,
            sense,
            rxf,
            txe,
            rd,
            wr,
            siwu,
            state: BusState::Unknown,
        }
    }

    #[inline(always)]
    fn is_connected(&mut self) -> bool {
        self.sense.is_high()
    }

    #[inline(always)]
    pub fn can_read(&mut self) -> bool {
        self.rxf.is_low()
    }

    #[inline(always)]
    pub fn can_write(&mut self) -> bool {
        self.txe.is_low()
    }

    // This sub will preform the required operation to read a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn read_byte(&mut self) -> u8 {
        // set the bus to an input
        if self.state != State::Input {
            // 0x00 sets all 8 pins to high-impedance input
            self.bus.ddrc().write(|w| unsafe { w.bits(0x00) });
            // 0x00 disable pull-ups
            self.bus.port.portc().write(|w| w.bits(0x00));
            // update the state
            self.state = State::Input;
        }
        // pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        self.rd.set_low();
        // preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
        avr_device::asm::nop();
        // read the data
        let data = self.bus.pinc().read().bits();
        // release the RD line since we are done with the operation
        self.rd.set_high();
        data
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn write_byte(&mut self, data: u8) {
        // set the bus to an output
        if self.state != State::Output {
            // DDRC register controls pin direction (0xFF sets all 8 pins to output)
            self.bus.ddrc().write(|w| unsafe { w.bits(0xFF) });
            // update the state
            self.state = State::Output;
        }

        // put the data onto the pins
        self.bus.portc().write(|w| unsafe { w.bits(byte) });
        // pull the WR line low so FT240 will sample the data bus and store it to its FIFO
        self.wr.set_low();
        // preform a nop to allow time for the FT240 to sample the data bus
        avr_device::asm::nop();
        // release the WR line since we are done with the operation
        self.wr.set_high();
    }

    // pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    #[inline(always)]
    pub fn flush(&mut self) {
        //pull the SIWU pin low
        self.siwu.set_low();
        // preform a nop to allow time to sense the logic level change
        avr_device::asm::nop2();
        //pull the SIWU back up
        self.siwu.set_high();
    }
}

// reader for ft240x
pub struct Ft240xReader<BUS, const BUFFER_CAPACITY: usize> {
    _bus: PhantomData<BUS>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
    waker: Option<Waker>,
    reciver_idle: bool,
}

impl Ft240xReader<PORTC, BUFFER_CAPACITY> {
    const RX_EXT_INT6: u8 = 1 << 6;
    pub const fn new() -> Self {
        Self {
            _bus: PhantomData,
            buffer: ConstCircularBuffer::new(),
            waker: None,
            reciver_idle: true,
        }
    }

    pub fn init(&self) {
        unsafe {
            // setup RXF(PE6/INT6) to trigger on falling edges
            (*EXINT::ptr())
                .eicrb()
                .modify(|_, w| w.isc6().falling_edge_of_intx());
            // clear the INT6 interrupt flag by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::RX_EXT_INT6));
        }
    }

    // disable receive interrupts
    #[inline(always)]
    pub fn rxf_int_disable() {
        unsafe {
            // disable interrupts
            (*EXINT::ptr())
                .eimsk()
                .modify(|r, w| w.int().bits(r.int().bits() & !Self::RX_EXT_INT6));
            // clear the interrupt flag
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::RX_EXT_INT6));
        }
    }

    // enable receive interrupts
    #[inline(always)]
    pub fn rxf_int_enable() {
        unsafe {
            // clear the interrupt flag
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::RX_EXT_INT6));
            // enable interrupts
            (*EXINT::ptr())
                .eimsk()
                .eodify(|r, w| w.int().bits(r.int().bits() | Self::RX_EXT_INT6));
        }
    }
}

pub struct Ft240xReaderHandle;

impl<BUS, RXF, RD> Ft240xReader<BUS, RXF, RD>
where
    BUS: IoBus8,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    // when RXF is low the FT240 has data to read.
    #[inline(always)]
    pub fn can_read(&mut self) -> bool {
        self.rxf.is_low().unwrap_or(false)
    }

    pub fn data_available(&mut self) -> DataAvailableFuture<'_, BUS, RXF, RD> {
        DataAvailableFuture { reader: self }
    }

    pub async fn read_to(
        &mut self,
        term: u8,
        buf: &mut FlatBuffer<'_>,
    ) -> Result<bool, BufferError> {
        loop {
            match self.try_read_to(term, buf).await {
                // haven't read terminator yet, continue
                Ok(false) => continue,
                // anything else we out
                result => break result,
            }
        }
    }

    pub async fn try_read_to(
        &mut self,
        term: u8,
        buf: &mut FlatBuffer<'_>,
    ) -> Result<bool, BufferError> {
        // did we get called with a full buffer
        if buf.is_full() {
            return Err(BufferError::BufferEmpty);
        }
        // wait until we can read
        if let Err(kind) = self.data_available().await {
            return Err(kind.into());
        }

        // we know we can read at least one, from the rts await above
        let byte = self.read_byte();
        // write it to the receive buffer, we can discard the result since we checked above
        _ = buf.write_byte(byte);
        // did we catch the terminator
        // if byte == term {
        return Ok(true);

        // walk the buffer
        let result = loop {
            // is the receiving buffer full
            if buf.is_full() {
                break Err(BufferError::InsufficientSpace);
            }
            // we know we can read at least one, from the rts await above
            let byte = self.read_byte();
            // write it to the receive buffer, we can discard the result since we checked above
            _ = buf.write_byte(byte);
            // did we catch the terminator
            // if byte == term {
            break Ok(true);
            // }

            // make sure were connected
            if !self.bus.is_connected() {
                // this should be something different
                break Err(BufferError::InsufficientSpace);
            }
            // make sure the ft240x has something to read
            if !self.can_read() {
                break Ok(false);
            }
        };
        // return the result
        result
    }
}

pub struct DataAvailableFuture<'a, BUS, RXF, RD> {
    pub reader: &'a mut Ft240xReader<BUS, RXF, RD>,
}

impl<'a, BUS, RXF, RD> Future for DataAvailableFuture<'_, BUS, RXF, RD>
where
    BUS: IoBus8,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        /*
        go interrupt free while we check.  if we have already registered the waker and the interrupt
        fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        not this one, and we may never get woken up again
        */
        avr_device::interrupt::free(|cs| {
            // see if we are connected
            if !self.reader.bus.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if there is data to read
            if self.reader.can_read() {
                return Poll::Ready(Ok(()));
            }
            // else no data to read.  register the waker
            *RX_WAKER.borrow(cs).borrow_mut() = Some(cx.waker().clone());
            // sanity check in case the edge was triggered while we were setting up the waker
            if self.reader.can_read() {
                // unregister the waker
                *RX_WAKER.borrow(cs).borrow_mut() = None;
                // return ready
                Poll::Ready(Ok(()))
            } else {
                // enable interrupts
                At90Can128Interrupts::rxf_int_enable();
                // return pending
                Poll::Pending
            }
        })
    }
}

impl<BUS, RXF, RD> ErrorType for Ft240xReader<BUS, RXF, RD> {
    type Error = embedded_io::ErrorKind;
}

impl<BUS, RXF, RD> Read for Ft240xReader<BUS, RXF, RD>
where
    BUS: IoBus8,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until we can read
        if let Err(kind) = self.data_available().await {
            return Err(kind);
        }
        // initialize the number of bytes read
        let mut bytes = 0;
        // walk the buffer
        for byte in buf.iter_mut() {
            // we know we can read at least one, from the rts await above
            *byte = self.read_byte();
            // increment the number of bytes read
            bytes += 1;
            // make sure were connected
            if !self.bus.is_connected() {
                break;
            }
            // make sure the ft240x has something to read
            if !self.can_read() {
                break;
            }
        }
        // here we read at least one so, return the number of bytes read
        Ok(bytes)
    }
}

// writer for ft240x
pub struct Ft240xWriter<BUS, const BUFFER_CAPACITY: usize> {
    _bus: PhantomData<BUS>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
    waker: Option<Waker>,
    transceiver_idle: bool,
}

impl Ft240xWriter<PORTC, BUFFER_CAPACITY> {
    const TX_EXT_INT5: u8 = 1 << 5;

    pub const fn new() -> Self {
        Self {
            _bus: PhantomData,
            buffer: ConstCircularBuffer::new(),
            waker: None,
            transceiver_idle: true,
        }
    }

    pub fn init(&self) {
        unsafe {
            // setup TXE(PE5/INT5) to trigger on falling edges
            (*EXINT::ptr())
                .eicrb()
                .modify(|_, w| w.isc5().falling_edge_of_intx());
            // clear the INT5 interrupt flags by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| unsafe { w.intf().bits(Self::TX_EXT_INT5) });
        }
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn txe_int_enable() {
        unsafe {
            // clear the INT5 interrupt flags by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::TX_EXT_INT5));
            // enable interrupts so we get interrupted when the FT240 can accept the next byte
            (*EXINT::ptr())
                .eimsk()
                .modify(|r, w| w.int().bits(r.int().bits() | Self::TX_EXT_INT5));
        }
    }

    // disable transmit interrupts
    #[inline(always)]
    pub fn txe_int_disable() {
        unsafe {
            // disable interrupts
            (*EXINT::ptr())
                .eimsk()
                .modify(|r, w| w.int().bits(r.int().bits() & !Self::TX_EXT_INT5));
            // clear the interrupt flag
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::TX_EXT_INT5));
        }
    }
}

//USB tx interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // take and wake the waker
    if let Some(waker) = TX_WAKER.borrow(cs).borrow_mut().take() {
        waker.wake();
    }
    // disable the interrupt
    At90Can128Interrupts::txe_int_disable();
}

//USB Rx Interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT6() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // take and wake the waker
    if let Some(waker) = RX_WAKER.borrow(cs).borrow_mut().take() {
        waker.wake();
    }
    // disable the interrupt
    At90Can128Interrupts::rxf_int_disable();
}

impl<BUS, TXE, WR, SIWU> Ft240xWriter<BUS, TXE, WR, SIWU>
where
    BUS: IoBus8,
    TXE: InputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    // when TXE is low the FT240 can accept data.
    #[inline(always)]
    pub fn can_write(&mut self) -> bool {
        self.txe.is_low().unwrap_or(false)
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn write_byte(&mut self, data: u8) {
        // set the bus to an output
        let _ = self.bus.set_as_output();
        // put the data onto the pins
        let _ = self.bus.write(data);
        // pull the WR line low so FT240 will sample the data bus and store it to its FIFO
        let _ = self.wr.set_low();
        // preform a nop to allow time for the FT240 to sample the data bus
        avr_device::asm::nop();
        // release the WR line since we are done with the operation
        let _ = self.wr.set_high();
    }

    // pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    #[inline(always)]
    pub fn flush(&mut self) {
        //pull the SIWU pin low
        let _ = self.siwu.set_low();
        // preform a nop to allow time to sense the logic level change
        avr_device::asm::nop2();
        //pull the SIWU back up
        let _ = self.siwu.set_high();
    }

    pub fn data_can_be_written(&mut self) -> DataCanBeWrittenFuture<'_, BUS, TXE, WR, SIWU> {
        DataCanBeWrittenFuture { writer: self }
    }
}

pub struct DataCanBeWrittenFuture<'a, BUS, TXE, WR, SIWU> {
    pub writer: &'a mut Ft240xWriter<BUS, TXE, WR, SIWU>,
}

impl<'a, BUS, TXE, WR, SIWU> Future for DataCanBeWrittenFuture<'_, BUS, TXE, WR, SIWU>
where
    BUS: IoBus8,
    TXE: InputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free while we check.  if we have already registered the waker and the interrupt
        // fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        // not this one, and we may never get woken up again
        avr_device::interrupt::free(|cs| {
            // see if we are connected
            if !self.writer.bus.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if its clear to send
            if self.writer.can_write() {
                return Poll::Ready(Ok(()));
            }
            // else we cant send.  register the waker
            *TX_WAKER.borrow(cs).borrow_mut() = Some(cx.waker().clone());
            // sanity check in case the edge was triggered while we were setting up the waker
            if self.writer.can_write() {
                // unregister the waker
                *TX_WAKER.borrow(cs).borrow_mut() = None;
                // return ready
                Poll::Ready(Ok(()))
            } else {
                // enable interrupts
                At90Can128Interrupts::txe_int_enable();
                // return pending
                Poll::Pending
            }
        })
    }
}

impl<BUS, TXE, WR, SIWU> ErrorType for Ft240xWriter<BUS, TXE, WR, SIWU> {
    type Error = embedded_io::ErrorKind;
}

impl<BUS, TXE, WR, SIWU> Write for Ft240xWriter<BUS, TXE, WR, SIWU>
where
    BUS: IoBus8,
    TXE: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    WR: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
    SIWU: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.len() == 0 {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until the ft240 can accept data
        if let Err(kind) = self.data_can_be_written().await {
            return Err(kind);
        }
        // initialize the number of bytes written
        let mut bytes = 0;
        // walk the buffer
        for byte in buf.iter() {
            // we know we can write at least one, from the cts await above
            let _ = self.write_byte(*byte);
            // increment the number of bytes written
            bytes += 1;
            // make sure were connected
            if !self.bus.is_connected() {
                break;
            }
            // make sure the ft240x can accept
            if !self.can_write() {
                break;
            }
        }
        // here we sent at least one, so return the number of bytes written
        Ok(bytes)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.flush();
        Ok(())
    }
}
