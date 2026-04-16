use core::{
    pin::Pin,
    sync::atomic::{AtomicU16, Ordering},
    task::{Context, Poll},
};

use avr_device::at90can128;

const CPU_FREQ: u32 = 14_745_600;
const PRESCALER: u32 = 64;
const TIMER_TARGET: u16 = (CPU_FREQ / PRESCALER / 1000) as u16 - 1;

static TICK_COUNT: AtomicU16 = AtomicU16::new(0);

pub struct Timer {
    target_tick: u16,
}

impl Timer {
    pub fn delay(ms: u16) -> Self {
        let current = critical_section::with(|_| TICK_COUNT.load(Ordering::Relaxed));
        Self {
            target_tick: current.wrapping_add(ms),
        }
    }

    pub fn init(tc: &at90can128::TC1) {
        // Prescaler 64: CS11 and CS10 are set (bits 1 and 0)
        // WGM12 (bit 3) is still set for CTC mode
        tc.tccr1b().write(|w| unsafe { w.bits(0b00001011) });
        tc.ocr1a().write(|w| unsafe { w.bits(TIMER_TARGET) }); // ~1ms @ 16MHz
        tc.timsk1().write(|w| w.ocie1a().set_bit());
    }
}

impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current = critical_section::with(|_| TICK_COUNT.load(Ordering::Relaxed));
        // Use wrapping subtraction to handle timer rollover safely
        if current.wrapping_sub(self.target_tick) < (1 << 15) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[avr_device::interrupt(at90can128)]
fn TIMER1_COMPA() {
    // Increment the tick every second
    let prev = TICK_COUNT.load(Ordering::Relaxed);
    TICK_COUNT.store(prev.wrapping_add(1), Ordering::Relaxed);
}
