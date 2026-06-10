#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
// #![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use core::{fmt::Write, panic::PanicInfo};

use avr_device::at90can128;
use avr_hal_generic::port::mode::{AnyInput, Input};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_io_async::{Read, Write as WriteAsync};

use hal::Pins;
pub mod async_queue;
mod at90can128_hal;
pub mod buffer_handle;
pub mod buffer_pool;
pub mod circular_buffer;
// pub mod driver;
pub mod executor;
pub mod flat_buffer;
mod ft240x;
pub mod hal;
mod interrupts;
pub mod led;
pub mod timer;
// pub mod usb_ft240;
mod avr_uart;
mod const_circular_buffer;
pub mod wait_pin_state;

pub use buffer_handle::*;
pub use buffer_pool::*;
pub use circular_buffer::*;
// pub use driver::*;
pub use executor::*;
pub use flat_buffer::*;
pub use led::*;
pub use timer::*;
// pub use usb_ft240::*;
pub use avr_uart::*;
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
        /*dp.PORTA,*/ dp.PORTB, /*dp.PORTC,*/ dp.PORTD, dp.PORTE,
        /*dp.PORTF,*/ dp.PORTG,
    );

    let err_led = LED::new(pins.pb6.into_output().downgrade(), true);
    let can_led = LED::new(pins.pb7.into_output().downgrade(), true);

    // network uart (maybe make these more generic and just pass downgraded inputs/outputs and let driver configure)
    // also the sense/reset/defaults are specific to xpico so maybe don't include in uart
    let (ethernet_reader, _ethernet_writer) =
        AvrUart::init(dp.USART1, pins.pg3, pins.pg4, pins.pd7, pins.pd4, pins.pg0);

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
    let (reader, usb_writer) = ft240.split();

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: ft240_reader_task(reader, err_led),
        b: usart1_reader_task(ethernet_reader, usb_writer, can_led),
    };

    unsafe {
        avr_device::interrupt::enable();
    }

    // 3. Start the Executor
    let mut executor = Executor::new(combined_future);
    executor.run();

    loop {}
}

pub async fn usart1_reader_task<BUS, TXE, WR, SIWU>(
    reader: Usart1ReaderHandle,
    mut writer: Ft240xWriter<BUS, TXE, WR, SIWU>,
    mut led: LED,
) where
    BUS: IoBus8,
    TXE: InputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    // get some buffers
    let mut rx_buffer: FlatBuffer = BufferRequest.await.into();
    let mut tx_buffer: FlatBuffer = BufferRequest.await.into();
    // turn off the led
    led.on();

    loop {
        rx_buffer.reset();
        tx_buffer.reset();

        // preform a read
        let bytes_read = reader.read_to(0x0a, &mut rx_buffer).await;
        // see what happened
        match bytes_read {
            Ok(term_read) => {
                tx_buffer.write(&rx_buffer.as_ref()[..rx_buffer.len() - 1]);
                tx_buffer.write_byte(0x0d);
                tx_buffer.write_byte(0x0a);

                // if let Err(e)  core::write!(tx_buffer, "hi {:?}", rx_buffer.len()) {

                // if let Err(e) = core::write!(tx_buffer, "hi {:?}", rx_buffer.len()) {
                //     led.off();
                // } else {
                //     led.on();
                // }

                // let _ = tx_buffer.write(&rx_buffer[..len - 1]);
                // _ = write!(tx_buffer, "bytes: {}\r\n", rx_buffer.len());
                // let _ = write!(tx_buffer, "del {}", rx_buffer.len());
            }
            Err(e) => {
                // _ = write!(tx_buffer, "error: {} \r\n", e);
            }
        };
        // write it
        let _ = writer.write_all(tx_buffer.as_ref()).await;
        // blink the led on
        // led.toggle();
    }
}

pub async fn ft240_reader_task<BUS, RXF, RD>(mut reader: Ft240xReader<BUS, RXF, RD>, mut led: LED)
where
    BUS: IoBus8,
    RXF: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
{
    // let mut counter: u16 = 0;

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

// pub async fn ft240_writer_task<BUS, TXE, WR, SIWU>(
//     mut writer: Ft240xWriter<BUS, TXE, WR, SIWU>,
//     mut led: LED,
// ) where
//     BUS: IoBus8,
//     TXE: InputPin<Error = core::convert::Infallible>,
//     WR: OutputPin<Error = core::convert::Infallible>,
//     SIWU: OutputPin<Error = core::convert::Infallible>,
// {
//     // get a counter for fun
//     let mut counter: u16 = 0;

//     let mut buffer: FlatBuffer = BufferRequest.await.into();

//     loop {
//         // turn off the led
//         led.off();
//         // reset the read buffer
//         buffer.reset();
//         // increment the counter
//         counter += 1;
//         // write something to the buffer
//         _ = write!(
//             buffer,
//             "Hello, World 123451234512345123451234512345. count: {}\r\n",
//             counter
//         );
//         // get an immutable slice
//         let tx_buffer = buffer.as_ref();
//         // send it
//         let _ = writer.write_all(tx_buffer).await;
//         // blink the led on
//         led.on();
//     }
// }

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
