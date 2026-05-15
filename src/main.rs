#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
use core::{fmt::Write, panic::PanicInfo};

use avr_device::at90can128;
use hal::Pins;
pub mod driver;
pub mod executor;
pub mod hal;
pub mod led;
pub mod timer;
pub mod wait_pin_state;
// pub mod usb_driver;
pub mod async_queue;
pub mod buffer_pool;
pub mod usb_ft240;

pub use executor::*;
pub use led::*;
pub use timer::*;
pub use wait_pin_state::*;
// pub use usb_driver::*;
pub use buffer_pool::*;
pub use driver::*;
pub use usb_ft240::*;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let pins = Pins::new(dp.PORTB, dp.PORTC, dp.PORTE, dp.PORTG);
    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    let usb = UsbFT240::init(
        pins.pe2.into_output().downgrade(),
        pins.pe4.into_output().downgrade(),
        pins.pe7.into_output().downgrade(),
        pins.pe5.into_floating_input().downgrade().forget_imode(),
        pins.pe6.into_floating_input().downgrade().forget_imode(),
        pins.pg2.into_floating_input().downgrade().forget_imode(),
        at90can128::PORTC::ptr(),
        at90can128::EXINT::ptr(),
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

pub async fn error_blink_task(mut led: LED, mut usb: UsbDriver) {
    let counter: u16 = 0;

    led.on();

    // request a buffer for usb driver
    let rx_buffer = BufferRequest.await;
    // submit it to the driver
    usb.init(rx_buffer);

    // let mut hello = BufferRequest.await;
    // _ = write!(hello, "Hello, World {}\r\n", counter);
    // _ = usb.write(hello).await;

    loop {
        // Timer::delay(1000).await;
        led.off();

        // request a buffer for receiving a packet
        let packet_buffer = BufferRequest.await;
        led.on();

        let packet_result = usb.read(packet_buffer).await;

        if let Ok(mut buffer) = packet_result {
            // send it
            buffer.write_byte(0x0d);
            buffer.reset();
            let _ = usb.write(buffer).await;
        }
        // let mut buffer = BufferRequest.await;

        // if led.is_on() {
        //     led.off();
        //     // _ = write!(buffer, "OFF: {}\r\n", counter);
        //     // usb.tx_submit(buffer);
        // } else {
        //     led.on();
        //     // _ = write!(buffer, "ON: {}\r\n", counter);
        //     // usb.tx_submit(buffer);
        // }
        // Timer::delay(250).await;

        // QUEUE.push(5).await;
    }
}

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
