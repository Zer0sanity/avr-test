#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]

use avr_device::at90can128;
use core::future::Future;
use core::panic::PanicInfo;
use core::pin::Pin;
use core::sync::atomic::{AtomicU16, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

const CPU_FREQ: u32 = 14_745_600;
const PRESCALER: u32 = 64;
const TIMER_TARGET: u16 = (CPU_FREQ / PRESCALER / 1000) as u16 - 1;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let error_led = LED::new(&dp.PORTB, 6);
    let can_led = LED::new(&dp.PORTB, 7);

    // Prescaler 64: CS11 and CS10 are set (bits 1 and 0)
    // WGM12 (bit 3) is still set for CTC mode
    dp.TC1.tccr1b().write(|w| unsafe { w.bits(0b00001011) });
    dp.TC1.ocr1a().write(|w| unsafe { w.bits(TIMER_TARGET) }); // ~1ms @ 16MHz
    dp.TC1.timsk1().write(|w| w.ocie1a().set_bit());

    unsafe { avr_device::interrupt::enable() };

    let combined_future = Join {
        a: error_blink_task(&error_led),
        b: can_blink_task(&can_led),
    };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub struct LED<'a> {
    port: &'a at90can128::portb::RegisterBlock,
    pin: u8,
}

impl<'a> LED<'a> {
    pub fn new(port: &'a at90can128::portb::RegisterBlock, pin: u8) -> Self {
        port.ddrb()
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << pin)) });
        Self { port, pin }
    }

    pub fn set_on(&self) {
        self.port
            .portb()
            .modify(|r, w| unsafe { w.bits(r.bits() & !(1 << self.pin)) });
    }

    pub fn set_off(&self) {
        self.port
            .portb()
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << self.pin)) });
    }

    pub fn is_on(&self) -> bool {
        self.port.pinb().read().bits() & (1 << self.pin) == 0
    }

    pub fn toggle(&self) {
        self.port.pinb().write(|w| unsafe { w.bits(1 << self.pin) });
    }
}

fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(core::ptr::null(), &VTABLE)
}

pub struct Executor<F: Future> {
    future: F,
}

impl<F: Future> Executor<F> {
    pub fn new(future: F) -> Self {
        Self { future }
    }

    pub fn run(&mut self) {
        let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
        let mut cx = Context::from_waker(&waker);

        // Pin the future to the stack
        let mut pinned_future = unsafe { Pin::new_unchecked(&mut self.future) };

        loop {
            match pinned_future.as_mut().poll(&mut cx) {
                Poll::Ready(_) => break, // Task finished
                Poll::Pending => {
                    // Optional: Put the CPU to sleep here to save power
                    // avr_device::asm::sleep();
                }
            }
        }
    }
}

async fn error_blink_task(led: &LED<'_>) {
    led.set_off();
    loop {
        led.toggle();
        Timer::delay(500).await;
    }
}

async fn can_blink_task(led: &LED<'_>) {
    led.set_off();
    loop {
        led.toggle();
        Timer::delay(100).await;
    }
}

static TICK_COUNT: AtomicU16 = AtomicU16::new(0);

struct Timer {
    target_tick: u16,
}

impl Timer {
    pub fn delay(ms: u16) -> Self {
        let current = critical_section::with(|_| TICK_COUNT.load(Ordering::Relaxed));
        Self {
            target_tick: current.wrapping_add(ms),
        }
    }
}

impl core::future::Future for Timer {
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

// A simple helper to run two tasks concurrently
pub struct Join<A, B> {
    a: A,
    b: B,
}

impl<A: Future<Output = ()>, B: Future<Output = ()>> Future for Join<A, B> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: We are manually projecting pinning to the inner fields
        let (a, b) = unsafe {
            let this = self.get_unchecked_mut();
            (
                Pin::new_unchecked(&mut this.a),
                Pin::new_unchecked(&mut this.b),
            )
        };

        // Poll both tasks. We don't care about the return Poll here
        // because these tasks loop forever.
        let _ = a.poll(cx);
        let _ = b.poll(cx);

        Poll::Pending
    }
}

#[avr_device::interrupt(at90can128)]
fn TIMER1_COMPA() {
    // Increment the tick every second
    let prev = TICK_COUNT.load(Ordering::Relaxed);
    TICK_COUNT.store(prev.wrapping_add(1), Ordering::Relaxed);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// use heapless::spsc::Queue; // Single-producer, single-consumer queue

// static mut USB_BUFFER: Queue<u8, 64> = Queue::new();

// struct UsbStream<'a> {
//     consumer: Queue<u8, 64>::Consumer<'a>,
// }

// impl<'a> UsbStream<'a> {
//     async fn next_byte(&mut self) -> u8 {
//         loop {
//             if let Some(byte) = self.consumer.dequeue() {
//                 return byte;
//             }
//             // If empty, yield back to executor
//             YieldFuture.await;
//         }
//     }
// }

// #[derive(serde::Deserialize)]
// struct CanPacket {
//     id: u32,
//     data: [u8; 8],
// }

// // In your task:
// let mut raw_buf = [0u8; 16];
// // fill raw_buf from UsbStream...
// let packet: CanPacket = postcard::from_bytes(&raw_buf).unwrap();
