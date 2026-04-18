use core::{
    pin::Pin,
    sync::atomic::{AtomicU16, Ordering},
    task::{Context, Poll},
};

use avr_device::at90can128;

use avr_device::interrupt::Mutex;
use core::cell::Cell;

const CPU_FREQ: u32 = 14_745_600;
const PRESCALER: u32 = 64;
const TIMER_TARGET: u16 = (CPU_FREQ / PRESCALER / 1000) as u16 - 1;

static TICK_COUNT: Mutex<Cell<u16>> = Mutex::new(Cell::new(0));
static WAKE_MASK: Mutex<Cell<u16>> = Mutex::new(Cell::new(0));

pub struct Timer {
    target_tick: u16,
}

impl Timer {
    pub fn init(tc: &at90can128::TC1) {
        tc.tccr1b().write(|w| unsafe { w.bits(0b00001011) });
        tc.ocr1a().write(|w| unsafe { w.bits(TIMER_TARGET) }); // ~1ms @ 16MHz
        tc.timsk1().write(|w| w.ocie1a().set_bit());
    }

    pub fn delay(delay: u16) -> Self {
        // get the tick count
        let tick_counter = Self::get_tick_counter();
        // adjust to get the target tick when delay completes
        let target_tick = tick_counter.wrapping_add(delay);
        Self { target_tick }
    }

    pub fn get_tick_counter() -> u16 {
        avr_device::interrupt::free(|cs| TICK_COUNT.borrow(cs).get())
    }
}

impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // get the tick counter
        let tick_counter = Self::get_tick_counter();
        // adjust with a wrapping subtract to detect rollover
        let expired = tick_counter.wrapping_sub(self.target_tick);
        // evaluate
        if tick_counter.wrapping_sub(self.target_tick) < (1 << 15) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[avr_device::interrupt(at90can128)]
fn TIMER1_COMPA() {
    // Forge a token. This is safe ONLY because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    TICK_COUNT.borrow(cs).update(|counter| counter + 1);
    WAKE_MASK.borrow(cs).set(0xff);
}
