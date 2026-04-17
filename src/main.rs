#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
use avr_device::at90can128;
use core::panic::PanicInfo;

pub mod executor;
pub mod led;
pub mod timer;
pub mod usb_ft240;

pub use executor::*;
pub use led::*;
pub use timer::*;
pub use usb_ft240::*;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let err_led = ErrLED::from(&dp);

    let can_led = CanLED::from(&dp);

    let usb = UsbFT240::new(&dp);

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: error_blink_task(&err_led, &usb),
        b: can_blink_task(&can_led, &usb),
    };

    unsafe { avr_device::interrupt::enable() };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn error_blink_task(led: &ErrLED<'_>, usb: &UsbFT240<'_>) {
    let on_str: &'static str = "ON\r\n";
    let off_str: &'static str = "OFF\r\n";

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
        usb.flush();

        Timer::delay(250).await;
    }
}

pub async fn can_blink_task(led: &CanLED<'_>, usb: &UsbFT240<'_>) {
    led.off();
    loop {
        if led.is_on() {
            led.off();
        } else {
            led.on();
        }

        let t = Timer::delay(50);
        t.await;
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
