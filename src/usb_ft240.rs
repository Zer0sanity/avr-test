use crate::CircularBuffer;
use crate::FlatBuffer;
use crate::driver::*;
use core::cell::RefCell;
use core::task::{Context, Poll};

use avr_device::at90can128;
use avr_device::interrupt::Mutex;

#[repr(C)]
pub struct AvrPort {
    pub pin: u8,  // Offset 0x00 (e.g., PINC)
    pub ddr: u8,  // Offset 0x01 (e.g., DDRC)
    pub port: u8, // Offset 0x02 (e.g., PORTC)
}

pub trait FT240Conf {
    //SIWU Output to tell the FT240 to flush its transmit FIFO buffer to the PC
    // RD Output to have the FT240 put a received byte from its FIFO to the data bus
    // WR Output to have the FT240 read data byte from data bus to its transmit FIFO
    // TXE Input to tell when the FT240 can accept data.  Pin will also be setup to generate an interrupt on falling edge when transmitting data
    // RXF Input to tell when data can be read from the FT240.  Pin will also be setup to generate an interrupt on falling edge for receiving data
    // SENSE input to tell if USB is connected
}
pub struct UsbFT240;

impl UsbFT240 {
    const BUS_PTR: *mut AvrPort = 0x26 as *mut AvrPort;
    const EXT_INT_PTR: *const at90can128::exint::RegisterBlock = at90can128::EXINT::ptr();
    const TX_EXT_INT5: u8 = 1 << 5;
    const RX_EXT_INT6: u8 = 1 << 6;

    // initializes hardware, set the shared instance, and return a usb driver
    pub fn init() -> UsbDriver {
        #[rustfmt::skip]

        // prepare initial states: active-low pins should start High (Off)
        UsbFT240::siwu_into_output_with_pullup();
        UsbFT240::rd_into_output_with_pullup();
        UsbFT240::wr_into_output_with_pullup();

        // setup txe, rxf, and sense as inputs floating
        UsbFT240::txe_into_input_floating();
        UsbFT240::rxf_into_input_floating();
        UsbFT240::sense_into_input_floating();

        // initialize the bus as high-impedance (Input + Pull-up)
        UsbFT240::configure_bus_as_input();
        UsbFT240::enable_bus_pullups();

        // setup the interrupts
        UsbFT240::rx_int_setup();
        UsbFT240::tx_int_setup();

        // a usb driver
        UsbDriver
    }

    // return the state of the sense pin
    #[allow(unused)]
    #[inline(always)]
    fn connected(&self) -> bool {
        UsbFT240::sense_is_high()
    }

    // setup receive interrupts
    #[inline(always)]
    pub fn rx_int_setup() {
        // initialize receive interrupts
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // setup RXF(PE6/INT6) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc6().falling_edge_of_intx());
        // clear the INT6 interrupt flag by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(UsbFT240::RX_EXT_INT6) });
    }

    // enable receive interrupts
    #[inline(always)]
    pub fn rx_int_enable() {
        // initialize receive interrupts
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // enable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | UsbFT240::RX_EXT_INT6) });
    }

    // disable receive interrupts
    #[inline(always)]
    pub fn rx_int_disable() {
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !UsbFT240::RX_EXT_INT6) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(UsbFT240::RX_EXT_INT6) });
    }

    // check receive interrupts
    #[inline(always)]
    pub fn rx_int_enabled() -> bool {
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // disable interrupts
        ext_int.eimsk().read().int().bits() & UsbFT240::RX_EXT_INT6 != 0
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn tx_int_setup() {
        // initialize external interrupts
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // setup TXE(PE5/INT5) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc5().falling_edge_of_intx());
        // clear the INT5 interrupt flags by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(UsbFT240::TX_EXT_INT5) });
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn tx_int_enable() {
        // initialize external interrupts
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // enable interrupts so we get interrupted when the FT240 can accept the next byte
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | UsbFT240::TX_EXT_INT5) });
    }

    // disable transmit interrupts
    #[inline(always)]
    pub fn tx_int_disable() {
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !UsbFT240::TX_EXT_INT5) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(UsbFT240::TX_EXT_INT5) });
    }

    // check transmit interrupts
    #[inline(always)]
    pub fn tx_int_enabled() -> bool {
        let ext_int = unsafe { &*UsbFT240::EXT_INT_PTR };
        // disable interrupts
        ext_int.eimsk().read().int().bits() & UsbFT240::TX_EXT_INT5 != 0
    }

    #[inline(always)]
    pub fn configure_bus_as_output() {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*UsbFT240::BUS_PTR).ddr), 0xff);
        }
    }

    #[inline(always)]
    pub fn configure_bus_as_input() {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*UsbFT240::BUS_PTR).ddr), 0x00);
        }
    }

    #[inline(always)]
    pub fn disable_bus_pullups() {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*UsbFT240::BUS_PTR).port), 0x00);
        }
    }

    #[inline(always)]
    pub fn enable_bus_pullups() {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*UsbFT240::BUS_PTR).port), 0xff);
        }
    }

    #[inline(always)]
    pub fn rd_into_output_with_pullup() {
        unsafe {
            core::arch::asm!(
                "sbi 0x0D, 4",
                "sbi 0x0E, 4",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn deassert_rd() {
        unsafe {
            core::arch::asm!("cbi 0x0E, 4", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn assert_rd() {
        unsafe {
            core::arch::asm!("sbi 0x0E, 4", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn wr_into_output_with_pullup() {
        unsafe {
            core::arch::asm!(
                "sbi 0x0D, 7",
                "sbi 0x0E, 7",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn deassert_wr() {
        unsafe {
            core::arch::asm!("cbi 0x0E, 7", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn assert_wr() {
        unsafe {
            core::arch::asm!("sbi 0x0E, 7", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn read_bus() -> u8 {
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*UsbFT240::BUS_PTR).pin)) }
    }

    #[inline(always)]
    pub fn write_bus(data: u8) {
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*UsbFT240::BUS_PTR).port), data);
        }
    }

    #[inline(always)]
    pub fn siwu_into_output_with_pullup() {
        unsafe {
            core::arch::asm!(
                "sbi 0x0D, 2",
                "sbi 0x0E, 2",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn deassert_siwu() {
        unsafe {
            core::arch::asm!("cbi 0x0E, 2", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn assert_siwu() {
        unsafe {
            core::arch::asm!("sbi 0x0E, 2", options(nostack, nomem, preserves_flags));
        }
    }

    #[inline(always)]
    pub fn txe_into_input_floating() {
        unsafe {
            core::arch::asm!(
                "cbi 0x0D, 5",
                "cbi 0x0E, 5",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn txe_is_high() -> bool {
        let result: u8;
        unsafe {
            core::arch::asm!(
                "ldi {out}, 0",
                "sbic 0x0C, 5",
                "ldi {out}, 1",
                out = out(reg) result,
                options(nostack, nomem, preserves_flags)
            );
        }
        result != 0
    }

    #[inline(always)]
    pub fn rxf_into_input_floating() {
        unsafe {
            core::arch::asm!(
                "cbi 0x0D, 6",
                "cbi 0x0E, 6",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn rxf_is_high() -> bool {
        let result: u8;
        unsafe {
            core::arch::asm!(
                "ldi {out}, 0",
                "sbic 0x0C, 6",
                "ldi {out}, 1",
                out = out(reg) result,
                options(nostack, nomem, preserves_flags)
            );
        }
        result != 0
    }

    #[inline(always)]
    pub fn sense_into_input_floating() {
        unsafe {
            core::arch::asm!(
                "cbi 0x14, 2",
                "cbi 0x15, 2",
                options(nostack, nomem, preserves_flags)
            );
        }
    }

    #[inline(always)]
    pub fn sense_is_high() -> bool {
        let result: u8;
        unsafe {
            core::arch::asm!(
                "ldi {out}, 0",
                "sbic 0x13, 2",
                "ldi {out}, 1",
                out = out(reg) result,
                options(nostack, nomem, preserves_flags)
            );
        }
        result != 0
    }

    // This sub will preform the required operation to read a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn read_byte() -> u8 {
        // After every RX or TX operation we reconfigure the data bus as inputs pulled up.  Therefore the ports DDR should already
        // be set properly.  All that is needed is to disable the pull-ups to allow the FT240 to drive them
        UsbFT240::disable_bus_pullups();
        // Pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        UsbFT240::deassert_rd();
        // Preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
        avr_device::asm::nop();
        // Read the data
        let data = UsbFT240::read_bus();
        // Release the RD line since we are done with the operation
        UsbFT240::assert_rd();
        // Re-enable the pull-ups
        UsbFT240::enable_bus_pullups();
        //return the value
        data
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn write_byte(data: u8) {
        // someone is going to cringe, but real men are unsafe
        unsafe {
            // The data bus should currently be configured as inputs with pull-ups enabled.
            // We first need to reconfigure the port as an output
            UsbFT240::configure_bus_as_output();
            // Put the data onto the pins
            UsbFT240::write_bus(data);
            // Pull the WR line low so FT240 will sample the data bus and store it to its FIFO
            UsbFT240::deassert_wr();
            // Preform a nop to allow time for the FT240 to sample the data bus
            avr_device::asm::nop();
            // Release the WR line since we are done with the operation
            UsbFT240::assert_wr();
            // Reconfigure the data bus pins as inputs with pull-ups enabled
            UsbFT240::configure_bus_as_input();
            UsbFT240::enable_bus_pullups();
        }
    }

    // This sub will write the byte slice to the FT240 and preform a flush
    #[inline(always)]
    pub fn write(bytes: &[u8]) {
        // write each byte
        bytes.into_iter().for_each(|data| {
            UsbFT240::write_byte(*data);
        });
        // flush
        UsbFT240::flush();
    }

    //Routine pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    #[inline(always)]
    pub fn flush() {
        //Pull the SIWU pin low
        UsbFT240::deassert_siwu();
        // Preform a nop to allow time to sense the logic level change
        avr_device::asm::nop();
        //Pull the SIWU back up
        UsbFT240::assert_siwu();
    }
}

static TX_STATE: Mutex<RefCell<Option<TxState>>> = Mutex::new(RefCell::new(None));
static RX_STATE: Mutex<RefCell<Option<RxState>>> = Mutex::new(RefCell::new(None));

pub struct UsbDriver;

pub struct UsbRxFuture {
    buffer: Option<FlatBuffer>,
}

impl Future for UsbRxFuture {
    type Output = Result<FlatBuffer, DriverError>;
    fn poll(mut self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free while checking for packet
        avr_device::interrupt::free(|cs| {
            // get the state
            let mut state_lock = RX_STATE.borrow(cs).borrow_mut();
            // get the rx state
            let rx_state = match state_lock.as_mut() {
                Some(state) => state,
                None => return Poll::Ready(Err(DriverError::MissingGlobalState)),
            };
            // its possible that we could get polled again after returning ready.  so, ensure we have
            // buffer to receive bytes into. since we take and return the buffer after detecting a packet
            // it will be none.
            let rx_buffer = match self.buffer.as_mut() {
                Some(buffer) => buffer,
                None => return Poll::Ready(Err(DriverError::MissingFutureBuffer)),
            };
            // while there are bytes to read and we haven't read a packer
            let mut packet_detected = false;
            // do somthing with this.  how do we want to handle overflow of receive buffer
            // let mut _over_flow = false;
            // loop while there are bytes to get or a packet was detected
            while let Some(byte) = rx_state.buffer.read_byte() {
                // process the byte
                if let Some(slot) = rx_buffer.next_write_slot() {
                    unsafe { (slot as *mut u8).write_volatile(byte) };
                    packet_detected = true;
                    // if a packet was read
                    if byte == 0x0d {
                        // set the packet found flag
                        packet_detected = true;
                        // exit the loop
                        break;
                    }
                }
            }
            // ensure interrupts are enabled
            UsbFT240::rx_int_enable();
            // finish up
            if packet_detected {
                Poll::Ready(Ok(self.buffer.take().unwrap()))
            } else {
                // register the waker
                rx_state.waker = Some(cx.waker().clone());
                // poll pending
                Poll::Pending
            }
        })
    }
}

pub struct UsbTxFuture;

impl Future for UsbTxFuture {
    type Output = Result<FlatBuffer, DriverError>;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // go interrupt free while checking for packet
        avr_device::interrupt::free(|cs| {
            // get the state
            let mut state_lock = TX_STATE.borrow(cs).borrow_mut();
            // get the tx state
            let tx_state = match state_lock.as_mut() {
                Some(state) => state,
                None => return Poll::Ready(Err(DriverError::MissingGlobalState)),
            };
            // if the result is set the transmission completed or had errors
            if tx_state.result.is_some() {
                // take the state
                let mut state = state_lock.take().unwrap();
                // take the result
                let result = state.result.take().unwrap();
                // check the result
                if result.is_ok() {
                    // take the buffer
                    Poll::Ready(Ok(state.buffer))
                } else {
                    return Poll::Ready(Err(result.unwrap_err()));
                }
            } else {
                //register the waker
                tx_state.waker = Some(cx.waker().clone());
                // poll pending
                return Poll::Pending;
            }
        })
    }
}

impl Driver for UsbDriver {
    type RxFuture = UsbRxFuture;
    type TxFuture = UsbTxFuture;

    fn init(&mut self, buffer: CircularBuffer) {
        // go interrupt free while updating state
        avr_device::interrupt::free(|cs| {
            // setup the tx state
            let state = RxState::new(buffer);
            // set the state
            *RX_STATE.borrow(cs).borrow_mut() = Some(state);
            // enable receive interrupts
            UsbFT240::rx_int_enable();
        });
    }

    fn read(&mut self, buffer: FlatBuffer) -> Self::RxFuture {
        // submits a buffer for the async rx future to receive bytes into
        UsbRxFuture {
            buffer: Some(buffer),
        }
    }

    fn write(&mut self, mut buffer: FlatBuffer) -> Self::TxFuture {
        // go interrupt free while updating state
        avr_device::interrupt::free(|cs| {
            // grab the first byte
            match buffer.read_byte() {
                // if its some
                Some(byte) => {
                    // set the state
                    *TX_STATE.borrow(cs).borrow_mut() = Some(TxState::new(buffer));
                    // enable the transmit interrupts
                    UsbFT240::tx_int_enable();
                    // kick off the first one
                    UsbFT240::write_byte(byte);
                }
                // no byte found
                None => {
                    // set the state to an error so the future will complete
                    *TX_STATE.borrow(cs).borrow_mut() =
                        Some(TxState::error(buffer, DriverError::BufferEmpty));
                    // disable transmit interrupts
                    UsbFT240::tx_int_disable()
                }
            }
            // return a future
            UsbTxFuture
        })
    }
}

//USB tx interrupt.  I if there is data to send it will transmit the next byte to the FT240 chip otherwise it will pulse the SIWU
//to flush the FT240s tx FIFO to the host and disable the tx interrupts.
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // Forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the state
    let mut state_lock = TX_STATE.borrow(cs).borrow_mut();
    // get the tx state
    let tx_state = match state_lock.as_mut() {
        Some(state) => state,
        None => {
            // we have no state, disable interrupts
            UsbFT240::tx_int_disable();
            return;
        }
    };
    // grab the next byte
    match tx_state.buffer.read_byte() {
        // write it
        Some(byte) => UsbFT240::write_byte(byte),
        // all done
        None => {
            // flush the data to the host
            UsbFT240::flush();
            // disable transmit interrupts
            UsbFT240::tx_int_disable();
            // update the state that we are done
            tx_state.result = Some(Ok(()));
            // kick the waker if its set
            if let Some(waker) = tx_state.waker.take() {
                waker.wake();
            }
        }
    }
}

//USB Rx Interrupt
#[avr_device::interrupt(at90can128)]
fn INT6() {
    // Forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // get the state
    let mut state_lock = RX_STATE.borrow(cs).borrow_mut();
    // get the rx state
    let rx_state = match state_lock.as_mut() {
        Some(state) => state,
        None => {
            // we have no state, disable interrupts
            UsbFT240::rx_int_disable();
            return;
        }
    };
    // if the buffer is full, leave byte in usb hardware buffer and disable Rx interrupts.
    // when the reader re-enable interrupts after reading bytes, the data will be waiting.
    // TODO there is a optimization here to read more then one byte at a time.
    // if rx_state.buffer.free_space() != 0 {
    //     // read byte off usb hardware
    //     let byte = UsbFT240::read_byte();
    //     // write it to the state buffer
    //     rx_state.buffer.write_byte(byte);
    // } else {
    if let Some(slot) = rx_state.buffer.next_write_slot() {
        // read byte off usb hardware
        let byte = UsbFT240::read_byte();
        unsafe { (slot as *mut u8).write_volatile(byte) };
    } else {
        // set an error
        rx_state.error = Some(DriverError::InsufficientSpace);
        // disable interrupts
        UsbFT240::rx_int_disable();
    }
    // kick the waker if its set
    if let Some(waker) = rx_state.waker.take() {
        waker.wake();
    }
}

// Local Variables:
// jinx-local-words: "isr nop rx tx txe usb"
// End:
