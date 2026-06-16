#![no_std]
#![no_main]
#![feature(abi_avr_interrupt)]
// #![cfg_attr(target_arch = "avr", feature(asm_experimental_arch))]
use avr_device::at90can128;
use core::fmt::Write as _;
use core::panic::PanicInfo;
use embedded_io_async::{Read as _, Write as _};

use hal::Pins;
pub mod async_queue;
pub mod buffer_handle;
pub mod buffer_pool;
pub mod circular_buffer;
// pub mod driver;
pub mod executor;
pub mod flat_buffer;
mod ft240x;
pub mod hal;
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

use crate::ft240x::{Ft240x, Ft240xReaderHandle, Ft240xWriterHandle};

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
    let cts = pins.pg3.into_output();
    let rts = pins.pg4.into_floating_input();
    let sense = pins.pd7.into_pull_up_input();
    let reset = pins.pd4.into_output_high();
    let defaults = pins.pg0.into_output_high();

    let (ethernet_reader, ethernet_writer) =
        AvrUart::init(dp.USART1, cts, rts, sense, reset, defaults);

    // initialize usb
    let bus = dp.PORTC;
    let sense = pins.pg2;
    let rd = pins.pe4.into_output_high();
    let rxf = pins.pe6;
    let wr = pins.pe7.into_output_high();
    let txe = pins.pe5;
    let siwu = pins.pe2.into_output_high();
    let (usb_reader, usb_writer) = Ft240x::init(bus, sense, rd, rxf, wr, txe, siwu);

    Timer::init(&dp.TC1);

    let combined_future = Join {
        a: ft240_reader_task(usb_reader, ethernet_writer, err_led),
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

pub async fn usart1_reader_task(
    reader: Usart1ReaderHandle,
    mut writer: Ft240xWriterHandle,
    mut led: LED,
) {
    // get some buffers
    let mut rx_buffer: FlatBuffer = BufferRequest.await.into();
    let mut tx_buffer: FlatBuffer = BufferRequest.await.into();

    let _ = writer.write("uart reader starting\r\n".as_bytes()).await;

    // turn off the led
    led.on();

    loop {
        rx_buffer.reset();
        tx_buffer.reset();

        // preform a read
        let packet_received = reader.read_to(0x0a, &mut rx_buffer).await;

        let len = rx_buffer.len();
        // see what happened
        match packet_received {
            Ok(_) => {
                let _ = tx_buffer.write_all(&rx_buffer.as_ref()[..rx_buffer.len() - 1]);
                let _ = tx_buffer.write_str(" bytes: ");
                let _ = tx_buffer.write_byte(len as u8 + 0x30);
                let _ = tx_buffer.write_str("\r\n");
            }
            Err(e) => {
                // _ = write!(tx_buffer, "error: {} \r\n", e);
            }
        };
        // write it
        let _ = writer.write(tx_buffer.as_ref()).await;
        // blink the led on
        led.toggle();
    }
}

pub async fn ft240_reader_task(
    mut reader: Ft240xReaderHandle,
    mut writer: Usart1WriterHandle,
    mut led: LED,
) {
    // get some buffers
    let mut rx_buffer: FlatBuffer = BufferRequest.await.into();
    let mut tx_buffer: FlatBuffer = BufferRequest.await.into();
    // turn off the led
    led.on();

    loop {
        rx_buffer.reset();
        // tx_buffer.reset();

        // // preform a read
        // let packet_received = reader.read_to(0x0a, &mut rx_buffer).await;

        let mut rx_slice = rx_buffer.as_mut();
        // let packet_received = reader.read(&mut rx_slice).await;
        // // get the buffer as a mutable slice
        // // let rx_buffer1 = rx_buffer.as_mut();
        // // preform a read
        // // let mut fuck: [u8; 30] = [0; 30];
        // // let rx_result = reader.read(&mut fuck).await;
        // // see what happened
        // led.toggle();
        // match packet_received {
        //     Ok(len) => {
        //         let _ = tx_buffer.write_all(&rx_slice[..len]);
        //         let _ = tx_buffer.write_str(" bytes: ");
        //         let _ = tx_buffer.write_byte(len as u8 + 0x30);
        //         let _ = tx_buffer.write_str("\r\n");
        //     }
        //     Err(_) => {
        //         // _ = write!(tx_buffer, "error: {} \r\n", e);
        //     }
        // };
        // write it
        // let _ = writer.write(tx_buffer.as_ref()).await;
        // let _ = writer.flush().await;

        Timer::delay(1000).await;
        // blink the led on
        led.toggle();
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
