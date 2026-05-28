pub mod io_bus_8;

use core::{cell::RefCell, task::{Context, Poll, Waker}};

use avr_device::{interrupt::Mutex};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io::{ErrorKind, ErrorType};
use embedded_io_async::{Read, Write};

use crate::{ft240x::io_bus_8::IoBus8, interrupts::At90Can128Interrupts};

// global waker for transmitting and receiving
pub static TX_WAKER: Mutex<RefCell<Option<Waker>>> = Mutex::new(RefCell::new(None));
pub static RX_WAKER: Mutex<RefCell<Option<Waker>>> = Mutex::new(RefCell::new(None));


pub struct Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
    bus: BUS,     // port used write/read from FT240
    sense: SENSE, // input to tell if USB is connected
    rxf: RXF,     // input to tell when data can be read from the FT240.
    txe: TXE,     // input to tell when the FT240 can accept data.
    rd: RD,       // output to have the FT240 put a received byte from its FIFO to the data bus
    wr: WR,       // output to have the FT240 read data byte from data bus to its transmit FIFO
    siwu: SIWU,   // output to tell the FT240 to flush its transmit FIFO buffer to the PC
    tx_pending: usize,
    rx_pending: usize,
}

impl<BUS, SENSE, RXF, TXE, RD, WR, SIWU> Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    #[rustfmt::skip]
    pub fn new(mut bus: BUS, sense: SENSE, rxf: RXF, txe: TXE, mut rd: RD, mut wr: WR, mut siwu: SIWU) -> Self {
        // initialize the bus as high-impedance (Input + Pull-up)
        let _ = bus.set_as_input();
        let _ = bus.write(0xff);
        // prepare initial states: active-low pins should start High (Off)
        let _ = rd.set_high();
        let _ = wr.set_high();
        let _ = siwu.set_high();

        // setup the interrupts
        At90Can128Interrupts::rxf_int_setup();
        At90Can128Interrupts::txe_int_setup();
        
        // initialize the structure
        Self {bus, sense, rxf, txe, rd, wr, siwu, tx_pending: 0, rx_pending: 0}
    }

    #[inline(always)]
    pub fn is_connected(&mut self) -> bool {
        self.sense.is_high().unwrap_or(false)
    }

    // when RXF is low the FT240 has data to read.
    #[inline(always)]
    pub fn can_read(&mut self) -> bool {
        self.rxf.is_low().unwrap_or(false)
    }

    // when TXE is low the FT240 can accept data.
    #[inline(always)]
    pub fn can_write(&mut self) -> bool {
        self.txe.is_low().unwrap_or(false)
    }

    // This sub will preform the required operation to read a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn read_byte(&mut self) -> u8 {
        // after every RX or TX operation we reconfigure the data bus as inputs pulled up.  therefore the ports DDR should already
        // be set properly.  all that is needed is to disable the pull-ups to allow the FT240 to drive them
        let _ = self.bus.write(0x00);
        // pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        let _ = self.rd.set_low();
        // preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
        avr_device::asm::nop();
        // read the data
        let data = self.bus.read().unwrap_or(0);
        // release the RD line since we are done with the operation
        let _ = self.rd.set_high();
        // re-enable the pull-ups
        let _ = self.bus.write(0xff);
        // return the data
        data
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn write_byte(&mut self, data: u8) {
        // the data bus should currently be configured as inputs with pull-ups enabled.
        // we first need to reconfigure the port as an output
        let _ = self.bus.set_as_output();
        // put the data onto the pins
        let _ = self.bus.write(data);
        // pull the WR line low so FT240 will sample the data bus and store it to its FIFO
        let _ = self.wr.set_low();
        // preform a nop to allow time for the FT240 to sample the data bus
        avr_device::asm::nop();
        // release the WR line since we are done with the operation
        let _ = self.wr.set_high();
        // reconfigure the data bus as an input
        let _ = self.bus.set_as_input();
        //  with pull-ups enabled
        let _ = self.bus.write(0xff);
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

    // returns a future to poll for when its clear to send data to the ft240x
    pub fn cts(&mut self) -> CtsFuture<'_, BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
        CtsFuture { ftdi: self }
    }

    // returns a future to poll for when its ok to read data from the ft240x
    pub fn rts(&mut self) -> RtsFuture<'_, BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
        RtsFuture { ftdi: self }
    }

    pub fn inc_tx_pending(&mut self)
    {
        self.tx_pending += 1;
    }

    pub fn tx_pending(&self) -> usize {
        self.tx_pending
    }

    pub fn inc_rx_pending(&mut self)
    {
        self.rx_pending += 1;
    }
    
    pub fn rx_pending(&self) -> usize {
        self.rx_pending
    }

    
}

// clear to send data to ft240x
pub struct CtsFuture<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
    pub ftdi: &'a mut Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>,
}

impl<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU> Future for CtsFuture<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
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
            if !self.ftdi.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if its clear to send
            if self.ftdi.can_write() {
                return Poll::Ready(Ok(()));
            }

            self.ftdi.inc_tx_pending();
            
            // else we cant send.  register the waker
            *TX_WAKER.borrow(cs).borrow_mut() = Some(cx.waker().clone());
            // enable interrupts
            At90Can128Interrupts::txe_int_enable();
            // poll pending
            return Poll::Pending;
        })
    }
}

// request to send from ft240x
pub struct RtsFuture<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
    pub ftdi: &'a mut Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>,
}

impl<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU> Future for RtsFuture<'a, BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
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
            if !self.ftdi.is_connected() {
                return Poll::Ready(Err(ErrorKind::NotConnected));
            }
            // now see if there is data to read
            if self.ftdi.can_read() {
                return Poll::Ready(Ok(()));
            }

            self.ftdi.inc_rx_pending();
            
            // else no data to read.  register the waker
            *RX_WAKER.borrow(cs).borrow_mut() = Some(cx.waker().clone());
            // enable interrupts
            At90Can128Interrupts::rxf_int_enable();
            // poll pending
            return Poll::Pending;
        })
    }
}


impl<BUS, SENSE, RXF, TXE, RD, WR, SIWU> ErrorType for Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>
{
    type Error = embedded_io::ErrorKind;
}


impl<BUS, SENSE, RXF, TXE, RD, WR, SIWU> Write for Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    RXF: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    TXE: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    RD: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
    WR: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
    SIWU: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        // did we get called with an empty buffer
        if buf.len() == 0 {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until the ft240 can accept data
        if let Err(kind) = self.cts().await {
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
            if !self.is_connected() {
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

impl<BUS, SENSE, RXF, TXE, RD, WR, SIWU> Read for Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    RXF: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    TXE: embedded_hal::digital::InputPin<Error = core::convert::Infallible>,
    RD: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
    WR: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
    SIWU: embedded_hal::digital::OutputPin<Error = core::convert::Infallible>,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // did we get called with a full buffer
        if buf.len() == 0 {
            return Err(ErrorKind::InvalidInput);
        }
        // wait until we can read
        if let Err(kind) = self.rts().await {
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
            if !self.is_connected() {
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
