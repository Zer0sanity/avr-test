#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
// #![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use core::{fmt::Write as fmtWrite, panic::PanicInfo};

use avr_device::at90can128;
use avr_hal_generic::port::mode::{AnyInput, Input};
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
mod uart;
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
pub use uart::*;
pub use wait_pin_state::*;

use crate::{
    at90can128_hal::avr_port::{BusHandle, StaticBus},
    ft240x::{Ft240x, Ft240xReader, Ft240xWriter, io_bus_8::IoBus8},
    hal::{Dynamic, Pin},
};

static USB_BUS: StaticBus<at90can128::PORTC, Pin<Input<AnyInput>, Dynamic>> = StaticBus::empty();

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    let pins = Pins::new(
        dp.PORTA, dp.PORTB, /*dp.PORTC,*/ dp.PORTD, dp.PORTE, dp.PORTF, dp.PORTG,
    );
    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    // network uart (maybe make these more generic and just pass downgraded inputs/outputs and let driver configure)
    // also the sense/reset/defaults are specific to xpico so maybe don't include in uart

    // let usart = Uart::new(
    //     dp.USART1, pins.pd2, pins.pd3, pins.pg3, pins.pg4, pins.pd7, pins.pd4, pins.pg0,
    // );

    // ft240
    let io_bus = BusHandle::init(
        dp.PORTC,
        pins.pg2.into_floating_input().downgrade().forget_imode(),
        &USB_BUS,
    );
    let rxf = pins.pe6.into_floating_input().downgrade().forget_imode();
    let txe = pins.pe5.into_floating_input().downgrade().forget_imode();
    let rd = pins.pe4.into_output().downgrade();
    let wr = pins.pe7.into_output().downgrade();
    let siwu = pins.pe2.into_output().downgrade();
    // initialize usb
    let ft240 = Ft240x::new(io_bus, rxf, txe, rd, wr, siwu);
    // split it
    let (reader, writer) = ft240.split();

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: ft240_reader_task(reader, err_led),
        b: ft240_writer_task(writer, can_led),
    };

    unsafe {
        avr_device::interrupt::enable();
    }

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn ft240_reader_task<BUS, RXF, RD>(mut reader: Ft240xReader<BUS, RXF, RD>, mut led: LED)
where
    BUS: IoBus8,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    let mut counter: u16 = 0;

    let mut buffer: FlatBuffer = BufferRequest.await.into();

    loop {
        // turn off the led
        led.off();
        // reset the read buffer
        buffer.reset();
        // get the buffer as a mutable slice
        let rx_buffer = buffer.as_mut();
        // preform a read
        let _ = reader.read(rx_buffer).await;
        // blink the led on
        led.on();
    }
}

pub async fn ft240_writer_task<BUS, TXE, WR, SIWU>(
    mut writer: Ft240xWriter<BUS, TXE, WR, SIWU>,
    mut led: LED,
) where
    BUS: IoBus8,
    TXE: InputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    // get a counter for fun
    let mut counter: u16 = 0;

    let mut buffer: FlatBuffer = BufferRequest.await.into();

    loop {
        // turn off the led
        led.off();
        // reset the read buffer
        buffer.reset();
        // increment the counter
        counter += 1;
        // write something to the buffer
        _ = write!(
            buffer,
            "Hello, World 123451234512345123451234512345. count: {}\r\n",
            counter
        );
        // get an immutable slice
        let tx_buffer = buffer.as_ref();
        // send it
        let _ = writer.write_all(tx_buffer).await;
        // blink the led on
        led.on();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
