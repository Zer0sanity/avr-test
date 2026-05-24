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
pub mod circular_buffer;
pub mod driver;
pub mod executor;
pub mod flat_buffer;
pub mod hal;
pub mod led;
pub mod timer;
pub mod usb_ft240;
pub mod wait_pin_state;

pub use buffer_handle::*;
pub use buffer_pool::*;
pub use circular_buffer::*;
pub use driver::*;
pub use executor::*;
pub use flat_buffer::*;
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

    // request a buffer for usb driver
    let handle = BufferRequest.await;
    let rx_buffer = handle.into();
    // submit it to the driver
    usb.init(rx_buffer);

    let handle = BufferRequest.await;
    let mut buffer: FlatBuffer = handle.into();

    _ = write!(buffer, "Hello, World 123{}\r\n", counter);
    let _ = usb.write(buffer).await;

    loop {
        // turn the led off
        led.off();

        // increment the counter
        counter += 1;

        // request a buffer for receiving a packet
        let rx_buffer: FlatBuffer = (BufferRequest.await).into();
        let rx_result = usb.read(rx_buffer).await;

        let tx_buffer = match rx_result {
            Ok(mut buf) => {
                let received_len = buf.len();
                let mut buffer: FlatBuffer = (BufferRequest.await).into();

                // copy in the received packet
                while let Ok(byte) = buf.read_byte() {
                    if byte == 0x0d {
                        break;
                    }

                    if !buffer.is_full() {
                        buffer.write_byte(byte);
                    }
                }

                _ = write!(
                    buffer,
                    " receive length: {}, counter: {}\r\n",
                    received_len, counter
                );
                buffer
            }
            Err(err) => {
                led.on();
                let handle = BufferRequest.await;
                let mut buffer: FlatBuffer = handle.into();
                let count = BufferRequest::free_buffers();
                _ = write!(buffer, "ERROR {}, count: {}\r\n", err, count);
                buffer
            }
        };
        // Timer::delay(50).await;
        led.on();
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
