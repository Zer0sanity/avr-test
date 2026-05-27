#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
#![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use core::{fmt::Write as fmtWrite, panic::PanicInfo};

use avr_device::at90can128;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io_async::Write;
use hal::Pins;
pub mod async_queue;
mod at90can128_hal;
pub mod buffer_handle;
pub mod buffer_pool;
pub mod circular_buffer;
pub mod driver;
pub mod executor;
pub mod flat_buffer;
mod ft240x;
pub mod hal;
mod interrupts;
pub mod led;
pub mod timer;
// pub mod usb_ft240;
pub mod wait_pin_state;

pub use buffer_handle::*;
pub use buffer_pool::*;
pub use circular_buffer::*;
pub use driver::*;
pub use executor::*;
pub use flat_buffer::*;
pub use led::*;
pub use timer::*;
// pub use usb_ft240::*;
pub use wait_pin_state::*;

use crate::{
    at90can128_hal::avr_port::AvrPort2,
    ft240x::{Ft240x, io_bus_8::IoBus8},
};

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    // let pins = Pins::new(dp.PORTB, dp.PORTC, dp.PORTE, dp.PORTG);
    let pins = Pins::new(dp.PORTB, dp.PORTE, dp.PORTG);
    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    // let usb = UsbFT240::init();

    let io_bus = AvrPort2 { port: dp.PORTC };
    let sense = pins.pg2.into_floating_input().downgrade().forget_imode();
    let rxf = pins.pe6.into_floating_input().downgrade().forget_imode();
    let txe = pins.pe5.into_floating_input().downgrade().forget_imode();
    let rd = pins.pe4.into_output().downgrade();
    let wr = pins.pe7.into_output().downgrade();
    let siwu = pins.pe2.into_output().downgrade();

    let mut ft240 = Ft240x::new(io_bus, sense, rxf, txe, rd, wr, siwu);

    if ft240.is_connected() {
        let _ = ft240.can_read();
        let _ = ft240.can_write();
        ft240.write_byte(0x00);
        let _ = ft240.read_byte();
        ft240.flush();
    }

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: error_blink_task(err_led, ft240),
        b: can_blink_task(can_led),
    };

    unsafe { avr_device::interrupt::enable() };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn error_blink_task<BUS, SENSE, RXF, TXE, RD, WR, SIWU>(
    mut led: LED,
    mut usb: Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>,
) where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    let mut counter: u16 = 0;

    // request a buffer for usb driver
    // let handle = BufferRequest.await;
    // let rx_buffer = handle.into();
    // submit it to the driver
    // usb.init(rx_buffer);

    let handle = BufferRequest.await;
    let mut buffer: [u8; 20];

    let hi = "Hello, World 123{}\r\n";
    let _ = usb.write(&hi.as_bytes()).await;

    loop {
        // // turn the led off
        // led.off();

        // // increment the counter
        // counter += 1;

        // // request a buffer for receiving a packet
        // let rx_buffer: FlatBuffer = (BufferRequest.await).into();
        // let rx_result = usb.read(rx_buffer).await;

        // let tx_buffer = match rx_result {
        //     Ok(mut buf) => {
        //         let received_len = buf.len();
        //         let mut buffer: FlatBuffer = (BufferRequest.await).into();

        //         // copy in the received packet
        //         while let Ok(byte) = buf.read_byte() {
        //             if byte == 0x0d {
        //                 break;
        //             }

        //             if !buffer.is_full() {
        //                 buffer.write_byte(byte);
        //             }
        //         }

        //         _ = write!(
        //             buffer,
        //             " receive length: {}, counter: {}\r\n",
        //             received_len, counter
        //         );
        //         buffer
        //     }
        //     Err(err) => {
        //         led.on();
        //         let handle = BufferRequest.await;
        //         let mut buffer: FlatBuffer = handle.into();
        //         let count = BufferRequest::free_buffers();
        //         _ = write!(buffer, "ERROR {}, count: {}\r\n", err, count);
        //         buffer
        //     }
        // };
        // // Timer::delay(50).await;
        // led.on();
        // let _ = usb.write(tx_buffer).await;
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
