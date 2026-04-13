#![no_std]
#![no_main]

use avr_device::at90can128;
use panic_halt as _; // You'll need to add `panic-halt = "0.2"` to Cargo.toml

#[avr_device::entry]
fn main() -> ! {
    let dp = at90can128::Peripherals::take().unwrap();

    // Example: Set Pin 0 of Port B as output
    // Note: Register names match the AT90CAN128 datasheet
    dp.PORTB.ddrb().write(|w| w.pb0().set_bit());

    loop {
        // Your logic here
        dp.PORTB.portb().modify(|_, w| w.pb0().set_bit());
    }
}
