use avr_device::at90can128;

use crate::ft240x::RX_WAKER;

//USB tx interrupt.  all this is doing is waking the waker and disabling interrupts
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // take and wake the waker
    if let Some(waker) = RX_WAKER.borrow(cs).borrow_mut().take() {
        waker.wake();
    }
    // disable the interrupt
    At90Can128Interrupts::txe_int_disable();
}

pub struct At90Can128Interrupts;

impl At90Can128Interrupts {
    const EXT_INT_PTR: *const at90can128::exint::RegisterBlock = at90can128::EXINT::ptr();
    const TX_EXT_INT5: u8 = 1 << 5;
    const RX_EXT_INT6: u8 = 1 << 6;

    #[inline(always)]
    pub fn rxf_int_setup() {
        // initialize receive interrupts
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // setup RXF(PE6/INT6) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc6().falling_edge_of_intx());
        // clear the INT6 interrupt flag by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(Self::RX_EXT_INT6) });
    }

    // disable receive interrupts
    #[inline(always)]
    pub fn rxf_int_disable() {
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !Self::RX_EXT_INT6) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(Self::RX_EXT_INT6) });
    }

    // enable receive interrupts
    #[inline(always)]
    pub fn rxf_int_enable() {
        // initialize receive interrupts
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // enable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | Self::RX_EXT_INT6) });
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn txe_int_setup() {
        // initialize external interrupts
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // setup TXE(PE5/INT5) to trigger on falling edges
        ext_int
            .eicrb()
            .modify(|_, w| w.isc5().falling_edge_of_intx());
        // clear the INT5 interrupt flags by writing it to 1
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(Self::TX_EXT_INT5) });
    }

    // enable transmit interrupts
    #[inline(always)]
    pub fn txe_int_enable() {
        // initialize external interrupts
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // enable interrupts so we get interrupted when the FT240 can accept the next byte
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() | Self::TX_EXT_INT5) });
    }

    // disable transmit interrupts
    #[inline(always)]
    pub fn txe_int_disable() {
        let ext_int = unsafe { &*Self::EXT_INT_PTR };
        // disable interrupts
        ext_int
            .eimsk()
            .modify(|r, w| unsafe { w.int().bits(r.int().bits() & !Self::TX_EXT_INT5) });
        // clear the interrupt flag
        ext_int
            .eifr()
            .write(|w| unsafe { w.intf().bits(Self::TX_EXT_INT5) });
    }
}
