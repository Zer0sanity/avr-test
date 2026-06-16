use core::{
    cell::RefCell,
    ptr,
    task::{Context, Poll, Waker},
};

use avr_device::{
    at90can128::{EXINT, PORTC},
    interrupt::Mutex,
};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use avr_hal_generic::port::mode::{Floating, Input, Output};

use crate::{
    FlatBuffer,
    hal::{PE2, PE4, PE5, PE6, PE7, PG2, Pin},
};

// static shared ft240x interface
static FT240X: Mutex<RefCell<Ft240x<PORTC, PG2, PE4, PE6, PE7, PE5, PE2>>> =
    Mutex::new(RefCell::new(Ft240x::new()));

#[derive(Clone, Copy, PartialEq)]
enum BusState {
    Unknown,
    Input,
    Output,
}

pub struct Ft240x<BUS, SENSE, RD, RXF, WR, TXE, SIWU> {
    bus: BUS,
    state: BusState,
    sense: Pin<Input<Floating>, SENSE>, // input to tell if usb host is connected
    rd: Pin<Output, RD>, // output to have the FT240 put a received byte from its FIFO to the data bus
    rxf: Pin<Input<Floating>, RXF>, // input to tell when data can be read from the FT240.
    wr: Pin<Output, WR>, // output to have the FT240 read data byte from data bus to its transmit FIFO
    txe: Pin<Input<Floating>, TXE>, // input to tell when the FT240 can accept data.
    siwu: Pin<Output, SIWU>, // output to tell the FT240 to flush its transmit FIFO buffer to the PC
    rx_waker: Option<Waker>,
    tx_waker: Option<Waker>,
}

impl Ft240x<PORTC, PG2, PE4, PE6, PE7, PE5, PE2> {
    const RX_EXT_INT6: u8 = 1 << 6;
    const TX_EXT_INT5: u8 = 1 << 5;

    pub const fn new() -> Self {
        unsafe {
            Self {
                bus: ptr::read(ptr::dangling()),
                state: BusState::Unknown,
                sense: ptr::read(ptr::dangling()),
                rd: ptr::read(ptr::dangling()),
                rxf: ptr::read(ptr::dangling()),
                wr: ptr::read(ptr::dangling()),
                txe: ptr::read(ptr::dangling()),
                siwu: ptr::read(ptr::dangling()),
                rx_waker: None,
                tx_waker: None,
            }
        }
    }

    // since we transmute out io port/pins in the const new(), ownership of these port/pins so they cant be used elsewhere.
    // configure the output pins and interrupts
    pub fn init(
        _bus: PORTC,
        _sense: Pin<Input<Floating>, PG2>,
        _rd: Pin<Output, PE4>,
        _rxf: Pin<Input<Floating>, PE6>,
        _wr: Pin<Output, PE7>,
        _txe: Pin<Input<Floating>, PE5>,
        _siwu: Pin<Output, PE2>,
    ) -> (Ft240xReaderHandle, Ft240xWriterHandle) {
        // configure the interrupts
        unsafe {
            // setup RXF(PE6/INT6) to trigger on falling edges
            (*EXINT::ptr())
                .eicrb()
                .modify(|_, w| w.isc6().falling_edge_of_intx());
            // clear the INT6 interrupt flag by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::RX_EXT_INT6));
            // setup TXE(PE5/INT5) to trigger on falling edges
            (*EXINT::ptr())
                .eicrb()
                .modify(|_, w| w.isc5().falling_edge_of_intx());
            // clear the INT5 interrupt flags by writing it to 1
            (*EXINT::ptr())
                .eifr()
                .write(|w| w.intf().bits(Self::TX_EXT_INT5));
        }
        // return handles
        (Ft240xReaderHandle, Ft240xWriterHandle)
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
        // if its not already an input
        if self.state != BusState::Input {
            // 0x00 sets all 8 pins to high-impedance input
            self.bus.ddrc().write(|w| unsafe { w.bits(0x00) });
            // 0x00 disable pull-ups
            self.bus.portc().write(|w| unsafe { w.bits(0x00) });
            // update the state
            self.state = BusState::Input;
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
    pub fn write_byte(&mut self, byte: u8) {
        // if its not already an output
        if self.state != BusState::Output {
            // DDRC register controls pin direction (0xFF sets all 8 pins to output)
            self.bus.ddrc().write(|w| unsafe { w.bits(0xFF) });
            // update the state
            self.state = BusState::Output;
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

    // disable receive interrupts
    #[inline(always)]
    pub fn rxf_int_disable(&self) {
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
    pub fn rxf_int_enable(&self) {
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

    // enable transmit interrupts
    #[inline(always)]
    pub fn txe_int_enable(&self) {
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
    pub fn txe_int_disable(&self) {
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

//USB Rx Interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT6() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the ft240x interface
    let mut usb = FT240X.borrow(cs).borrow_mut();
    // take and wake the waker
    if let Some(waker) = usb.rx_waker.take() {
        waker.wake();
    }
    // disable the interrupt
    usb.rxf_int_disable();
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
            // get the bus the ft240x interface
            let mut usb = FT240X.borrow(cs).borrow_mut();
            // are we connected
            if !usb.is_connected() {
                return Err(ErrorKind::NotConnected);
            }
            // we know we can read at least one, from the await above
            loop {
                // we know we can read at least one, from the rts await above
                let byte = usb.read_byte();
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
                if !usb.is_connected() {
                    // this should be something different
                    return Err(ErrorKind::NotConnected);
                }
                // make sure the ft240x has something to read
                if !usb.can_read() {
                    return Ok(false);
                }
            }
        })
    }
}

pub struct Ft240xCanRead;

impl Future for Ft240xCanRead {
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        /*
        go interrupt free while we check.  if we have already registered the waker and the interrupt
        fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        not this one, and we may never get woken up again
        */
        avr_device::interrupt::free(|cs| {
            // get the bus
            let mut usb = FT240X.borrow(cs).borrow_mut();
            // see if we are connected
            if !usb.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if there is data to read
            if usb.can_read() {
                return Poll::Ready(Ok(()));
            }
            // else no data to read.  register the waker
            usb.rx_waker = Some(cx.waker().clone());
            // enable interrupts
            usb.rxf_int_enable();
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
            // get the bus the ft240x interface
            let mut usb = FT240X.borrow(cs).borrow_mut();
            // are we connected
            if !usb.is_connected() {
                return Err(ErrorKind::NotConnected);
            }
            // we know we can read at least one, from the await above
            // initialize the number of bytes read
            let mut bytes = 0;
            // walk the buffer
            for byte in buf.iter_mut() {
                // we know we can read at least one, from the rts await above
                *byte = usb.read_byte();
                // increment the number of bytes read
                bytes += 1;
                // make sure were connected
                if !usb.is_connected() {
                    // this should be something different
                    return Err(ErrorKind::NotConnected);
                }
                // make sure the ft240x has something to read
                if !usb.can_read() {
                    return Ok(bytes);
                }
            }
            return Ok(bytes);
        })
    }
}

//USB tx interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the ft240x interface
    let mut usb = FT240X.borrow(cs).borrow_mut();
    // take and wake the waker
    if let Some(waker) = usb.tx_waker.take() {
        waker.wake();
    }
    // disable the interrupt
    usb.txe_int_disable();
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
        // index into buffer
        let mut idx = 0;
        // while we have bytes to send or we can't
        loop {
            let byte = buf[idx];

            // disable interrupts we write
            avr_device::interrupt::free(|cs| {
                // get the ft240x interface
                let mut usb = FT240X.borrow(cs).borrow_mut();
                // write the byte
                usb.write_byte(buf[idx]);
            });
            // increment the number of bytes written
            idx += 1;
            // can we still send
            let done = avr_device::interrupt::free(|cs| {
                // get the ft240x interface
                let mut usb = FT240X.borrow(cs).borrow_mut();
                idx == buf.len() || !usb.can_write() || !usb.is_connected()
            });
            // are we done
            if done {
                break Ok(idx);
            }
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        // disable interrupts while flushing
        avr_device::interrupt::free(|cs| {
            // get the ft240x interface
            let mut usb = FT240X.borrow(cs).borrow_mut();
            // see if we are connected
            if !usb.is_connected() {
                return Err(ErrorKind::NotConnected);
            }
            // pulse the flush signal
            usb.flush();
            return Ok(());
        })
    }
}

pub struct Ft240xCanWrite;

impl Future for Ft240xCanWrite {
    type Output = Result<(), embedded_io::ErrorKind>;
    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free while we check.  if we have already registered the waker and the interrupt
        // fires between checking pins and registering the waker.  the interrupt will wake the waker, but
        // not this one, and we may never get woken up again
        avr_device::interrupt::free(|cs| {
            // get the ft240x interface
            let mut usb = FT240X.borrow(cs).borrow_mut();
            // see if we are connected
            if !usb.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if its clear to send
            if usb.can_write() {
                return Poll::Ready(Ok(()));
            }
            // else we cant send.  register the waker
            usb.tx_waker = Some(cx.waker().clone());
            // enable interrupts
            usb.txe_int_enable();
            // return pending
            return Poll::Pending;
        })
    }
}
