use crate::driver::*;
use crate::{BufferHandle, hal::Pin};
use core::cell::RefCell;

use avr_device::at90can128;
use avr_device::interrupt::Mutex;
use avr_hal_generic::port::mode;

// bit mask for the different usb states
// pub mod usb_state {
//     pub const IDLE: u8 = 1 << 0;
//     pub const TRANSMITTING: u8 = 1 << 1;
//     pub const WAITING_FOR_SPACE: u8 = 1 << 2;
//     pub const SPACE_AVAILABLE: u8 = 1 << 3;
//     pub const DATA_RECEIVED: u8 = 1 << 4;
// }

// // static state
// static USB_STATUS: AtomicU8 = AtomicU8::new(usb_state::IDLE | usb_state::SPACE_AVAILABLE);

const TX_EXT_INT5: u8 = 1 << 5;
const RX_EXT_INT6: u8 = 1 << 6;

static USB: Mutex<RefCell<Option<UsbFT240>>> = Mutex::new(RefCell::new(None));

unsafe impl Sync for UsbFT240 {}
unsafe impl Send for UsbFT240 {}

pub struct UsbFT240 {
    siwu: Pin<mode::Output>, //SIWU Output to tell the FT240 to flush its transmit FIFO buffer to the PC
    rd: Pin<mode::Output>, // RD Output to have the FT240 put a received byte from its FIFO to the data bus
    wr: Pin<mode::Output>, // WR Output to have the FT240 read data byte from data bus to its transmit FIFO
    txe: Pin<mode::Input>, // TXE Input to tell when the FT240 can accept data.  Pin will also be setup to generate an interrupt on falling edge when transmitting data
    rxf: Pin<mode::Input>, // RXF Input to tell when data can be read from the FT240.  Pin will also be setup to generate an interrupt on falling edge for receiving data
    sense: Pin<mode::Input>, // SENSE input to tell if USB is connected
    bus_ptr: *const at90can128::portc::RegisterBlock, // BUS input/output port for read/write store as a pointer so we can preform full port read and writes
    ext_int_ptr: *const at90can128::exint::RegisterBlock,
}

impl UsbFT240 {
    // initializes hardware, set the shared instance, and return a usb driver
    pub fn init(
        siwu: Pin<mode::Output>,
        rd: Pin<mode::Output>,
        wr: Pin<mode::Output>,
        txe: Pin<mode::Input>,
        rxf: Pin<mode::Input>,
        sense: Pin<mode::Input>,
        bus_ptr: *const at90can128::portc::RegisterBlock,
        ext_int_ptr: *const at90can128::exint::RegisterBlock,
    ) -> UsbDriver {
        #[rustfmt::skip]
        // initialize the structure
        let mut usb = Self { siwu, rd, wr, txe, rxf, sense, bus_ptr, ext_int_ptr };

        // prepare initial states: active-low pins should start High (Off)
        usb.siwu.set_high();
        usb.rd.set_high();
        usb.wr.set_high();

        // initialize the bus as high-impedance (Input + Pull-up)
        unsafe {
            let bus = &*usb.bus_ptr;
            bus.ddrc().write(|w| w.bits(0x00));
            bus.portc().write(|w| w.bits(0xFF));
        }

        // set the shared instance
        avr_device::interrupt::free(|cs| *USB.borrow(cs).borrow_mut() = Some(usb));

        // a usb driver
        UsbDriver
    }

    // return the state of the sense pin
    #[inline(always)]
    fn connected(&self) -> bool {
        self.sense.is_high()
    }

    // enable receive interrupts
    #[inline(always)]
    pub fn rx_int_enable(&self) {
        // initialize receive interrupts
        let ext_int = unsafe { &*self.ext_int_ptr };
        // setup RXF(PE6/INT6) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc6().falling_edge_of_intx());
        // clear the INT6 interrupt flag by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(RX_EXT_INT6) });
        // enable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | RX_EXT_INT6) });
    }

    // disable receive interrupts
    #[inline(always)]
    pub fn rx_int_disable(&self) {
        let ext_int = unsafe { &*self.ext_int_ptr };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !RX_EXT_INT6) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(RX_EXT_INT6) });
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn tx_int_enable(&self) {
        // initialize external interrupts
        let ext_int = unsafe { &*self.ext_int_ptr };
        // setup TXE(PE5/INT5) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc5().falling_edge_of_intx());
        // clear the INT5 interrupt flags by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(TX_EXT_INT5) });
        // enable interrupts so we get interrupted when the FT240 can accept the next byte
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | TX_EXT_INT5) });
    }

    // disable transmit interrupts
    #[inline(always)]
    pub fn tx_int_disable(&self) {
        let ext_int = unsafe { &*self.ext_int_ptr };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !TX_EXT_INT5) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(TX_EXT_INT5) });
    }

    // This sub will preform the required operation to transmit byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    pub fn write_byte(&mut self, data: u8) {
        // someone is going to cringe, but real men are unsafe
        unsafe {
            // deference the data bus
            let bus = &*self.bus_ptr;
            // The data bus should currently be configured as inputs with pull-ups enabled.
            // We first need to reconfigure the port as an output
            bus.ddrc().write(|w| w.bits(0xff));
            // Put the data onto the pins
            bus.portc().write(|w| w.bits(data));
            // Pull the WR line low so FT240 will sample the data bus and store it to its FIFO
            self.wr.set_low();
            // Preform a nop to allow time for the FT240 to sample the data bus
            avr_device::asm::nop();
            // Release the WR line since we are done with the operation
            self.wr.set_high();
            // Reconfigure the data bus pins as inputs with pull-ups enabled
            bus.ddrc().write(|w| w.bits(0x00));
            bus.portc().write(|w| w.bits(0xff));
        }
    }

    // This sub will preform the required operation to receive a byte from the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled
    // if interrupts are being used.
    pub fn read_byte(&mut self) -> u8 {
        // someone is going to cringe, but there pronouns are her/she
        unsafe {
            // deference the data bus
            let bus = &*self.bus_ptr;
            // After every RX or TX operation we reconfigure the data bus as inputs pulled up.  Therefore the ports DDR should already
            // be set properly.  All that is needed is to disable the pull-ups to allow the FT240 to drive them
            bus.portc().write(|w| w.bits(0x00));
            // Pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
            self.rd.set_low();
            // Preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
            avr_device::asm::nop();
            // Read the data
            let data = bus.pinc().read().bits();
            // Release the RD line since we are done with the operation
            self.rd.set_high();
            // Re-enable the pull-ups
            bus.portc().write(|w| w.bits(0xff));
            //Return the value
            data
        }
    }

    // This sub will write the byte slice to the FT240 and preform a flush
    #[inline(always)]
    pub fn write(&mut self, bytes: &[u8]) {
        // write each byte
        bytes.into_iter().for_each(|data| {
            self.write_byte(*data);
        });
        // flush
        self.flush();
    }

    //Routine pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    #[inline(always)]
    pub fn flush(&mut self) {
        //Pull the SIWU pin low
        self.siwu.set_low();
        // Preform a nop to allow time to sense the logic level change
        avr_device::asm::nop();
        //Pull the SIWU back up
        self.siwu.set_high();
    }
}

static TX_STATE: Mutex<RefCell<Option<Transfer>>> = Mutex::new(RefCell::new(None));
static RX_STATE: Mutex<RefCell<Option<Transfer>>> = Mutex::new(RefCell::new(None));

pub struct UsbDriver;

impl Driver for UsbDriver {
    fn tx_submit(&mut self, buffer: BufferHandle) {
        // we should be awaiting an active transfer as well, but go get things going.  get the usb and wait
        // wait for the TXE pin to go low

        // go interrupt free while updating state
        avr_device::interrupt::free(|cs| {
            // get the usb reference
            if let Some(usb) = USB.borrow(cs).borrow_mut().as_mut() {
                // Simple busy check
                if TX_STATE.borrow(cs).borrow().is_some() {
                    return;
                }
                // setup the tx state
                let mut state = Transfer::new(buffer);
                // wait for txe to go low
                while !usb.txe.is_low() {}
                // get the first byte and start the transfer
                match state.next() {
                    None => usb.tx_int_disable(),
                    Some(byte) => {
                        // set the state
                        *TX_STATE.borrow(cs).borrow_mut() = Some(state);
                        // enable the transmit interrupts
                        usb.tx_int_enable();
                        // kick off the first one
                        usb.write_byte(byte);
                    }
                }
            }
        });
    }

    fn rx_submit(&mut self, buffer: BufferHandle) {
        // we should be awaiting an active transfer as well, but go get things going.  get the usb and wait
        // and set the state if its not already set

        // go interrupt free while updating state
        avr_device::interrupt::free(|cs| {
            // get the usb reference
            if let Some(usb) = USB.borrow(cs).borrow_mut().as_mut() {
                // Simple busy check
                if RX_STATE.borrow(cs).borrow().is_some() {
                    return;
                }
                // setup the tx state
                let mut state = Transfer::new(buffer);
                // set the state
                *RX_STATE.borrow(cs).borrow_mut() = Some(state);
                // enable rx interrupts
                usb.rx_int_enable();
            }
        });
    }
}

//USB tx interrupt.  I if there is data to send it will transmit the next byte to the FT240 chip otherwise it will pulse the SIWU
//to flush the FT240s tx FIFO to the host and disable the tx interrupts.
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // Forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get at our static reference
    if let Some(usb) = USB.borrow(cs).borrow_mut().as_mut() {
        // get the next byte
        let data = TX_STATE
            .borrow(cs)
            .borrow_mut()
            .as_mut()
            .and_then(|state| state.next());

        // get the tx state
        match data {
            Some(byte) => {
                // write the data
                usb.write_byte(byte);
            }
            None => {
                // flush the data to the host
                usb.flush();
                // disable transmit interrupts
                usb.tx_int_disable();
                // take/drop the transfer buffer
                let _state = TX_STATE.borrow(cs).take();
            }
        }
    }
}

// //USB Rx Interrupt
#[avr_device::interrupt(at90can128)]
fn INT6() {
    // Forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get at our static reference
    if let Some(usb) = USB.borrow(cs).borrow_mut().as_mut() {
        // get the transfer to write the byte into
        // if the transfer doesn't exit, we can disable the rx interrupt and leave the byte there
        // otherwise read the byte
        // write it to the transfer
        // figure out the next steps

        // get the next byte
        let data = RX_STATE
            .borrow(cs)
            .borrow_mut()
            .as_mut()
            .and_then(|state| state.next());

        // read the byte

        // get the tx state
        match data {
            Some(byte) => {
                // write the data
                usb.write_byte(byte);
            }
            None => {
                // flush the data to the host
                usb.flush();
                // disable transmit interrupts
                usb.tx_int_disable();
                // take/drop the transfer buffer
                let _state = TX_STATE.borrow(cs).take();
            }
        }
    }
}

// Local Variables:
// jinx-local-words: "isr nop tx txe usb"
// End:
