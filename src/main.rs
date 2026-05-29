#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
// #![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use core::{fmt::Write as fmtWrite, panic::PanicInfo};

use avr_device::at90can128;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io_async::{Read, Write};
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
    ft240x::{Ft240x, Ft240xReader, Ft240xWriter, io_bus_8::IoBus8},
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

    // initialize usb
    let ft240 = Ft240x::new(io_bus, sense, rxf, txe, rd, wr, siwu);
    // split it
    let (reader, writer) = ft240.split();

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: ft240_reader_task(reader, err_led),
        b: ft240_writer_task(writer, can_led),
    };

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn ft240_reader_task<'a, BUS, SENSE, RXF, RD>(
    mut reader: Ft240xReader<'a, BUS, SENSE, RXF, RD>,
    mut led: LED,
) where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    let mut counter: u16 = 0;

    let mut buffer: FlatBuffer = BufferRequest.await.into();

    led.off();

    loop {
        buffer.reset();

        let rx_buffer = buffer.as_mut();
        let _ = reader.read(rx_buffer).await;

        counter += 1;

        if counter % 5 == 0 {
            Timer::delay(50).await;
        }
    }
}

pub async fn ft240_writer_task<'a, BUS, SENSE, TXE, WR, SIWU>(
    mut writer: Ft240xWriter<'a, BUS, SENSE, TXE, WR, SIWU>,
    mut led: LED,
) where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    let mut counter: u16 = 0;

    let mut buffer: FlatBuffer = BufferRequest.await.into();

    led.off();

    loop {
        buffer.reset();

        counter += 1;

        _ = write!(
            buffer,
            "Hello, World 123451234512345123451234512345. count: {}\r\n",
            counter
        );

        let tx_buffer = buffer.as_ref();

        let _ = writer.write(tx_buffer).await;

        if counter % 5 == 0 {
            Timer::delay(50).await;
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
