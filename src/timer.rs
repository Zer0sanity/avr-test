use core::{
    pin::Pin,
    task::{Context, Poll},
};
use portable_atomic::{AtomicU8, Ordering};

use avr_device::at90can128;

use avr_device::interrupt::Mutex;
use core::cell::Cell;

const CPU_FREQ: u32 = 14_745_600;
const PRESCALER: u32 = 64;
const TIMER_TARGET: u16 = (CPU_FREQ / PRESCALER / 1000) as u16 - 1;

static TICK_COUNT: Mutex<Cell<u16>> = Mutex::new(Cell::new(0));
static READY_MASK: AtomicU8 = AtomicU8::new(0);
static NEXT_READY_MASK_BIT_INDEX: Mutex<Cell<u8>> = Mutex::new(Cell::new(0));

pub struct Timer {
    target_tick: u16,
    ready_mask_flag: u8,
}

impl Timer {
    pub fn init(tc: &at90can128::TC1) {
        tc.tccr1b().write(|w| unsafe { w.bits(0b00001011) });
        tc.ocr1a().write(|w| unsafe { w.bits(TIMER_TARGET) }); // ~1ms @ 16MHz
        tc.timsk1().write(|w| w.ocie1a().set_bit());
    }

    pub fn delay(delay: u16) -> Self {
        // get the tick count and ready mask bit index
        let (tick_counter, ready_mask_bit_index) = avr_device::interrupt::free(|cs| {
            let count = TICK_COUNT.borrow(cs).get();
            let bit_index = NEXT_READY_MASK_BIT_INDEX.borrow(cs).get();
            NEXT_READY_MASK_BIT_INDEX
                .borrow(cs)
                .set((bit_index + 1) % 8);
            (count, bit_index)
        });
        // adjust to get the target tick when delay completes
        let target_tick = tick_counter.wrapping_add(delay);
        // formulate the ready mask flag
        let ready_mask_flag = 1 << ready_mask_bit_index;
        // we're good here
        Self {
            target_tick,
            ready_mask_flag,
        }
    }
}

impl Future for Timer {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // do the light stuff, to avoid disabling interrupts every time through
        let ready_mask = READY_MASK.load(Ordering::Relaxed);
        if (ready_mask & self.ready_mask_flag) == 0 {
            return Poll::Pending;
        }
        // do the heavy stuff, clear our ready mask flag and get the current counter
        let tick_counter = avr_device::interrupt::free(|cs| {
            // clear our ready mask flag
            READY_MASK.fetch_and(!self.ready_mask_flag, Ordering::Relaxed);
            // return the tick counter
            TICK_COUNT.borrow(cs).get()
        });
        // adjust with a wrapping subtract to detect rollover
        let expired = tick_counter.wrapping_sub(self.target_tick);
        // evaluate
        if expired < (1 << 15) {
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
    READY_MASK.store(0xff, Ordering::Relaxed);
}
