#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
use avr_device::at90can128;
use core::panic::PanicInfo;

mod mpi104_hal;
use mpi104_hal::LED;
use mpi104_hal::Timer;
use mpi104_hal::UsbFT240;

mod executor;
use executor::{Executor, Join};

const CPU_FREQ: u32 = 14_745_600;
const PRESCALER: u32 = 64;
const TIMER_TARGET: u16 = (CPU_FREQ / PRESCALER / 1000) as u16 - 1;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let error_led = LED::new(&dp.PORTB, 6);
    let can_led = LED::new(&dp.PORTB, 7);

    let usb = UsbFT240::new(&dp);

    Timer::init(&dp.TC1, TIMER_TARGET);

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

pub async fn error_blink_task(led: &LED<'_>) {
    led.set_off();
    loop {
        led.toggle();
        Timer::delay(500).await;
    }
}

pub async fn can_blink_task(led: &LED<'_>) {
    led.set_off();
    loop {
        led.toggle();
        Timer::delay(100).await;
    }
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
