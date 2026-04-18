#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
use core::panic::PanicInfo;

use avr_device::at90can128;
use avr_hal_generic::port::{self, mode};
use hal::Pin;
use hal::Pins;
pub mod executor;
pub mod led;
pub mod timer;
pub mod usb_ft240;

pub use executor::*;
pub use led::*;
pub use timer::*;
pub use usb_ft240::*;

// This macro creates the 'Pins' struct and implements all the high-level logic.

pub mod hal;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let pins = Pins::new(dp.PORTB, dp.PORTC, dp.PORTE, dp.PORTG);
    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    let usb = UsbFT240::new(
        pins.pe2.into_output().downgrade(),
        pins.pe4.into_output().downgrade(),
        pins.pe7.into_output().downgrade(),
        pins.pe5.into_floating_input().downgrade().forget_imode(),
        pins.pe6.into_floating_input().downgrade().forget_imode(),
        pins.pg2.into_floating_input().downgrade().forget_imode(),
        at90can128::PORTC::ptr(),
    );

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: error_blink_task(err_led, usb),
        b: can_blink_task(can_led),
    };

    unsafe { avr_device::interrupt::enable() };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn error_blink_task(mut led: LED, mut usb: UsbFT240) {
    let on_str: &'static str = "ON\r\n";
    let off_str: &'static str = "OFF\r\n";

    led.on();

    loop {
        if led.is_on() {
            led.off();
            usb.write(off_str.as_bytes());
        } else {
            led.on();
            usb.write(on_str.as_bytes());
        }
        Timer::delay(250).await;
    }
}

// pub async fn can_blink_task(mut led: Pin<mode::Output, PB7>, usb: &UsbFT240<'_>) {
pub async fn can_blink_task(mut led: LED) {
    led.on();

    loop {
        led.toggle();
        Timer::delay(50).await;
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
