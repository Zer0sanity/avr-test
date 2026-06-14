use core::{
    cell::RefCell,
    marker::PhantomData,
    mem::transmute,
    task::{Context, Poll, Waker},
};

use avr_device::{
    at90can128::{self, EXINT, PORTC},
    interrupt::Mutex,
};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use avr_hal_generic::port::mode::{Floating, Input, Output};

use crate::{
    BufferError, FlatBuffer,
    hal::{PE2, PE4, PE5, PE6, PE7, PG2, Pin},
};

// static shared ft240 BUS
static FT240_BUS: Mutex<RefCell<Option<Ft240xBus<PORTC, PG2>>>> = Mutex::new(RefCell::new(None));
// static shared ft240 reader
static FT240_READER: Mutex<RefCell<Option<Ft240xReader<PORTC, PE4, PE6>>>> =
    Mutex::new(RefCell::new(None));
// static shared ft2401 writer
static FT240_WRITER: Mutex<RefCell<Ft240xWriter<PORTC, PE7, PE2, PE5>>> =
    Mutex::new(RefCell::new(Ft240xWriter::new()));

pub struct Ft240x;

impl Ft240x {
    pub fn init(
        bus: PORTC,
        sense: Pin<Input<Floating>, PG2>,
        rd: Pin<Input<Floating>, PE4>,
        wr: Pin<Input<Floating>, PE7>,
        siwu: Pin<Input<Floating>, PE2>,
        rxf: Pin<Input<Floating>, PE6>,
        txe: Pin<Input<Floating>, PE5>,
    ) -> (Ft240xReaderHandle, Ft240xWriterHandle) {
        // configure output pins
        let rd = rd.into_output();
        let wr = wr.into_output();
        let siwu = siwu.into_output();

        // go interrupt free while we setup bus, reader, writer
        avr_device::interrupt::free(|cs| {
            // set the bus
            *FT240_BUS.borrow(cs).borrow_mut() = Some(Ft240xBus::new(bus, sense));
            // set the reader
            *FT240_READER.borrow(cs).borrow_mut() = Some(Ft240xReader::new(rd, rxf));
            // set the writer
            // *FT240_WRITER.borrow(cs).borrow_mut() = Some(Ft240xWriter::new(wr, siwu, txe));
            // *FT240_WRITER.borrow(cs).borrow_mut() = Some(Ft240xWriter::new());
            let mut writer = FT240_WRITER.borrow(cs).borrow_mut();
            writer.init();
            // control.init(Some(sense), Some(reset), Some(defaults));
            // control.init(None, None, None);
        });
        // return handles
        (Ft240xReaderHandle, Ft240xWriterHandle)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BusState {
    Unknown,
    Input,
    Output,
}

// base struct for ft240x bus
pub struct Ft240xBus<BUS, SENSE> {
    bus: BUS,
    sense: Pin<Input<Floating>, SENSE>,
    state: BusState,
}

impl Ft240xBus<PORTC, PG2> {
    pub fn new(bus: PORTC, sense: Pin<Input<Floating>, PG2>) -> Self {
        Self {
            bus,
            sense,
            state: BusState::Unknown,
        }
    }

    // pub const fn new() -> Self {
    //     Self {
    //         bus: unsafe { transmute(()) },
    //         sense: unsafe { transmute(()) },
    //         state: BusState::Unknown,
    //     }
    // }

    #[inline(always)]
    fn is_connected(&mut self) -> bool {
        self.sense.is_high()
    }

    // set the bus to an input
    #[inline(always)]
    pub fn configure_bus_as_input(&mut self) {
        // if its not already an input
        if self.state != BusState::Input {
            // 0x00 sets all 8 pins to high-impedance input
            self.bus.ddrc().write(|w| unsafe { w.bits(0x00) });
            // 0x00 disable pull-ups
            self.bus.portc().write(|w| unsafe { w.bits(0x00) });
            // update the state
            self.state = BusState::Input;
        }
    }

    // set the bus to an output
    #[inline(always)]
    pub fn configure_bus_as_output(&mut self) {
        // if its not already an output
        if self.state != BusState::Output {
            // DDRC register controls pin direction (0xFF sets all 8 pins to output)
            self.bus.ddrc().write(|w| unsafe { w.bits(0xFF) });
            // update the state
            self.state = BusState::Output;
        }
    }
}

// reader for ft240x
pub struct Ft240xReader<BUS, RD, RXF> {
    _bus: PhantomData<BUS>,
    rd: Pin<Output, RD>, // output to have the FT240 put a received byte from its FIFO to the data bus
    rxf: Pin<Input<Floating>, RXF>, // input to tell when data can be read from the FT240.
    waker: Option<Waker>,
}

impl Ft240xReader<PORTC, PE4, PE6> {
    const RX_EXT_INT6: u8 = 1 << 6;
    pub fn new(rd: Pin<Output, PE4>, rxf: Pin<Input<Floating>, PE6>) -> Self {
        // configure interrupts
        Self::configure_rx_int();
        // initialize a new reader
        Self {
            _bus: PhantomData,
            rd,
            rxf,
            waker: None,
        }
    }

    #[inline(always)]
    pub fn can_read(&mut self) -> bool {
        self.rxf.is_low()
    }

    // This sub will preform the required operation to read a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn read_byte(&mut self, bus: &mut PORTC) -> u8 {
        // pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        self.rd.set_low();
        // preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
        avr_device::asm::nop();
        // read the data
        let data = bus.pinc().read().bits();
        // release the RD line since we are done with the operation
        self.rd.set_high();
        data
    }

    // disable receive interrupts
    #[inline(always)]
    fn configure_rx_int() {
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
                .modify(|r, w| w.int().bits(r.int().bits() | Self::RX_EXT_INT6));
        }
    }
}

//USB Rx Interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT6() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // take and wake the waker
    FT240_READER.borrow(cs).borrow_mut().as_mut().map(|reader| {
        if let Some(waker) = reader.waker.take() {
            waker.wake();
        }
    });
    // disable the interrupt
    Ft240xReader::rxf_int_disable();
}

pub struct Ft240xReaderHandle;

impl Ft240xReaderHandle {
    pub async fn read_to(&mut self, term: u8, buf: &mut FlatBuffer<'_>) -> Result<bool, ErrorKind> {
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
    ) -> Result<bool, ErrorKind> {
        // did we get called with a full buffer
        if buf.is_full() {
            return Err(ErrorKind::OutOfMemory);
        }
        // wait until we can read
        if let Err(kind) = Ft240xCanRead.await {
            return Err(kind);
        }

        // disable interrupts while reading hardware
        avr_device::interrupt::free(|cs| {
            // get the bus
            if let Some(bus) = FT240_BUS.borrow(cs).borrow_mut().as_mut() {
                // see if we are connected
                if bus.is_connected() {
                    // get the reader
                    if let Some(reader) = FT240_READER.borrow(cs).borrow_mut().as_mut() {
                        bus.configure_bus_as_input();
                        // we know we can read at least one, from the await above
                        loop {
                            // we know we can read at least one, from the rts await above
                            let byte = reader.read_byte(&mut bus.bus);
                            // write it to the receive buffer, we can discard the result since we checked above
                            _ = buf.write_byte(byte);
                            // did we catch the terminator
                            if byte == term {
                                return Ok(true);
                            }
                            // is the receiving buffer full
                            if buf.is_full() {
                                return Err(ErrorKind::NotConnected);
                            }
                            // make sure were connected
                            if !bus.is_connected() {
                                // this should be something different
                                return Err(ErrorKind::NotConnected);
                            }
                            // make sure the ft240x has something to read
                            if !reader.can_read() {
                                return Ok(false);
                            }
                        }
                    } else {
                        return Err(ErrorKind::NotConnected);
                    }
                } else {
                    return Err(ErrorKind::NotConnected);
                }
            } else {
                return Err(ErrorKind::NotConnected);
            }
        })
    }
}

pub struct Ft240xCanRead;

impl Future for Ft240xCanRead {
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        /*
        go interrupt free while we check.  if we have already registered the waker and the interrupt
        fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        not this one, and we may never get woken up again
        */
        avr_device::interrupt::free(|cs| {
            // get the bus
            let mut bus = FT240_BUS.borrow(cs).borrow_mut();
            // see if we are connected
            if bus.as_mut().is_none_or(|b| !b.is_connected()) {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // get the reader
            let mut reader = FT240_READER.borrow(cs).borrow_mut();
            // do we have a reader
            if reader.is_none() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if there is data to read
            if reader.as_mut().is_some_and(|r| r.can_read()) {
                return Poll::Ready(Ok(()));
            }
            // else no data to read.  register the waker
            reader.as_mut().map(|r| r.waker = Some(cx.waker().clone()));
            // enable interrupts
            Ft240xReader::rxf_int_enable();
            // return pending
            Poll::Pending
        })
    }
}

impl ErrorType for Ft240xReaderHandle {
    type Error = embedded_io::ErrorKind;
}

impl Read for Ft240xReaderHandle {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.is_empty() {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until we can read
        if let Err(kind) = Ft240xCanRead.await {
            return Err(kind);
        }
        // disable interrupts while reading hardware
        avr_device::interrupt::free(|cs| {
            // get the bus
            if let Some(bus) = FT240_BUS.borrow(cs).borrow_mut().as_mut() {
                // see if we are connected
                if bus.is_connected() {
                    // get the reader
                    if let Some(reader) = FT240_READER.borrow(cs).borrow_mut().as_mut() {
                        bus.configure_bus_as_input();
                        // we know we can read at least one, from the await above
                        // initialize the number of bytes read
                        let mut bytes = 0;
                        // walk the buffer
                        for byte in buf.iter_mut() {
                            // we know we can read at least one, from the rts await above
                            *byte = reader.read_byte(&mut bus.bus);
                            // increment the number of bytes read
                            bytes += 1;
                            // make sure were connected
                            if !bus.is_connected() {
                                // this should be something different
                                return Err(ErrorKind::NotConnected);
                            }
                            // make sure the ft240x has something to read
                            if !reader.can_read() {
                                return Ok(bytes);
                            }
                        }
                        return Ok(bytes);
                    } else {
                        return Err(ErrorKind::NotConnected);
                    }
                } else {
                    return Err(ErrorKind::NotConnected);
                }
            } else {
                return Err(ErrorKind::NotConnected);
            }
        })
    }
}

// writer for ft240x
pub struct Ft240xWriter<BUS, WR, SIWU, TXE> {
    bus: BUS,
    wr: Pin<Output, WR>, // output to have the FT240 read data byte from data bus to its transmit FIFO
    siwu: Pin<Output, SIWU>, // output to tell the FT240 to flush its transmit FIFO buffer to the PC
    txe: Pin<Input<Floating>, TXE>, // input to tell when the FT240 can accept data.
    waker: Option<Waker>,
    // _bus: PhantomData<BUS>,
    // wr: Pin<Output, WR>, // output to have the FT240 read data byte from data bus to its transmit FIFO
    // siwu: Pin<Output, SIWU>, // output to tell the FT240 to flush its transmit FIFO buffer to the PC
    // txe: Pin<Input<Floating>, TXE>, // input to tell when the FT240 can accept data.
    // waker: Option<Waker>,
}

impl Ft240xWriter<PORTC, PE7, PE2, PE5> {
    const TX_EXT_INT5: u8 = 1 << 5;

    // pub fn new(
    //     wr: Pin<Output, PE7>,
    //     siwu: Pin<Output, PE2>,
    //     txe: Pin<Input<Floating>, PE5>,
    // ) -> Self {
    //     // configure interrupts
    //     Self::configure_tx_int();
    //     // initialize a writer
    //     Self {
    //         _bus: PhantomData,
    //         wr,
    //         siwu,
    //         txe,
    //         waker: None,
    //     }
    // }

    pub const fn new() -> Self {
        unsafe {
            Self {
                bus: transmute(()),
                wr: transmute(()),
                siwu: transmute(()),
                txe: transmute(()),
                waker: None,
            }
        }
    }

    pub fn init(&self) {
        self.configure_tx_int();
    }

    #[inline(always)]
    pub fn can_write(&mut self) -> bool {
        self.txe.is_low()
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    // pub fn write_byte(&mut self, bus: &mut PORTC, byte: u8) {
    pub fn write_byte(&mut self, byte: u8) {
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

    #[inline(always)]
    fn configure_tx_int(&self) {
        unsafe {
            // setup TXE(PE5/INT5) to trigger on falling edges
            (*EXINT::ptr())
                .eicrb()
                .modify(|_, w| w.isc5().falling_edge_of_intx());
            // clear the INT5 interrupt flags by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::TX_EXT_INT5));
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
    if let Some(waker) = FT240_WRITER.borrow(cs).borrow_mut().waker.take() {
        waker.wake();
    }
    // disable the interrupt
    Ft240xWriter::txe_int_disable();
}

pub struct Ft240xWriterHandle;

impl ErrorType for Ft240xWriterHandle {
    type Error = embedded_io::ErrorKind;
}

impl Write for Ft240xWriterHandle {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.len() == 0 {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until the ft240 can accept data
        if let Err(kind) = Ft240xCanWrite.await {
            return Err(kind);
        }
        // disable interrupts while reading hardware
        avr_device::interrupt::free(|cs| {
            // get the bus
            if let Some(bus) = FT240_BUS.borrow(cs).borrow_mut().as_mut() {
                // see if we are connected
                if bus.is_connected() {
                    // get the reader
                    // if let Some(writer) = FT240_WRITER.borrow(cs).borrow_mut().as_mut() {
                    let mut writer = FT240_WRITER.borrow(cs).borrow_mut();

                    bus.configure_bus_as_output();
                    // initialize the number of bytes written
                    let mut bytes = 0;
                    // walk the buffer
                    for byte in buf.iter() {
                        // we know we can write at least one, from the cts await above
                        let _ = writer.write_byte(*byte);
                        // increment the number of bytes written
                        bytes += 1;
                        // make sure were connected
                        if !bus.is_connected() {
                            break;
                        }
                        // make sure the ft240x can accept
                        if !writer.can_write() {
                            break;
                        }
                    }
                    return Ok(bytes);
                } else {
                    //     return Err(ErrorKind::NotConnected);
                    // }                } else {
                    return Err(ErrorKind::NotConnected);
                }
            } else {
                return Err(ErrorKind::NotConnected);
            }
        })
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.flush();
        Ok(())
    }
}

pub struct Ft240xCanWrite;

impl Future for Ft240xCanWrite {
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free while we check.  if we have already registered the waker and the interrupt
        // fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        // not this one, and we may never get woken up again
        avr_device::interrupt::free(|cs| {
            // get the bus
            let mut bus = FT240_BUS.borrow(cs).borrow_mut();
            // see if we are connected
            if bus.as_mut().is_none_or(|b| !b.is_connected()) {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // get the writer
            let mut writer = FT240_WRITER.borrow(cs).borrow_mut();
            // now see if its clear to send
            if writer.can_write() {
                return Poll::Ready(Ok(()));
            }
            // else we cant send.  register the waker
            writer.waker = Some(cx.waker().clone());
            // enable interrupts
            Ft240xWriter::txe_int_enable();
            // return pending
            Poll::Pending
        })
    }
}
