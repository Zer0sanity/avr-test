#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
#![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use core::{fmt::Write, panic::PanicInfo};

use avr_device::at90can128;
use hal::Pins;
pub mod async_queue;
pub mod buffer_handle;
pub mod buffer_pool;
pub mod driver;
pub mod executor;
pub mod hal;
pub mod led;
pub mod timer;
pub mod usb_ft240;
pub mod wait_pin_state;

pub use buffer_handle::*;
pub use buffer_pool::*;
pub use driver::*;
pub use executor::*;
pub use led::*;
pub use timer::*;
pub use usb_ft240::*;
pub use wait_pin_state::*;

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let pins = Pins::new(dp.PORTB, dp.PORTC, dp.PORTE, dp.PORTG);
    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    let usb = UsbFT240::init();

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
    let mut counter: u16 = 0;

    led.on();

    // request a buffer for usb driver
    let rx_buffer = BufferRequest.await;
    // submit it to the driver
    usb.init(rx_buffer);

    let mut hello = BufferRequest.await;
    _ = write!(hello, "Hello, World 123{}\r\n", counter);
    let _ = usb.write(hello).await;

    loop {
        // request a buffer for receiving a packet
        led.off();
        let rx_buffer = BufferRequest.await;
        let rx_result = usb.read(rx_buffer).await;

        led.on();

        let tx_buffer = match rx_result {
            Ok(mut buf) => {
                counter += 1;

                let mut buffer = BufferRequest.await;
                buffer.write(&buf.as_slice()[..buf.len() - 1]);
                // _ = write!(buffer, "{}", buf.as_slice());
                _ = write!(buffer, "count: {}\r\n", buf.len());
                _ = write!(buffer, "count: {}\r\n", buf.len());

                buffer
            }
            Err(err) => {
                led.on();
                let mut buffer = BufferRequest.await;
                let count = BufferRequest::free_buffers();
                _ = write!(buffer, "ERROR {}, count: {}\r\n", err, count);
                buffer
            }
        };
        // Timer::delay(50).await;
        let _ = usb.write(tx_buffer).await;
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
