use avr_device::at90can128;
use avr_hal_generic::port::{self, mode};

avr_hal_generic::impl_port_traditional! {
    enum Ports {
        B: at90can128::PORTB = [0, 1, 2, 3, 4, 5, 6, 7],
        C: at90can128::PORTC = [0, 1, 2, 3, 4, 5, 6, 7],
        E: at90can128::PORTE = [0, 1, 2, 3, 4, 5, 6, 7],
        G: at90can128::PORTG = [0, 1, 2, 3, 4, 5, 6, 7],
    }
}
