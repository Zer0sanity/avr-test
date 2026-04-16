#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
use avr_device::at90can128;
use core::panic::PanicInfo;

mod mpi104_hal;
pub use crate::mpi104_hal::timer::*;
use mpi104_hal::Timer;
use mpi104_hal::UsbFT240;
use mpi104_hal::{CanLED, ErrLED, LED};

mod executor;
use executor::{Executor, Join};

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let err_led = ErrLED::from(&dp);

    let can_led = CanLED::from(&dp);

    let usb = UsbFT240::new(&dp);

    Timer::init(&dp.TC1);

    unsafe { avr_device::interrupt::enable() };

    let combined_future = Join {
        a: error_blink_task(&err_led, &usb),
        b: can_blink_task(&can_led),
    };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn error_blink_task(led: &ErrLED<'_>, usb: &UsbFT240<'_>) {
    let on_str: &'static str = "ON";
    let off_str: &'static str = "OFF";

    led.off();

    loop {
        if led.is_on() {
            led.off();
            off_str.bytes().for_each(|data| {
                usb.tx_byte(data);
            });
        } else {
            led.on();
            on_str.bytes().for_each(|data| {
                usb.tx_byte(data);
            });
        }
        Timer::delay(500).await;
    }
}

pub async fn can_blink_task(led: &CanLED<'_>) {
    led.off();
    loop {
        if led.is_on() {
            led.off();
        } else {
            led.on();
        }
        Timer::delay(100).await;
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
