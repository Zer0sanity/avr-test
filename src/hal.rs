use avr_device::at90can128;
use avr_hal_generic::port::mode;
// this macro creates the 'Pins' struct and implements all the high-level logic.
avr_hal_generic::impl_port_traditional! {
    enum Ports {
        A: at90can128::PORTA = [0, 1, 2, 3, 4, 5, 6, 7],
        B: at90can128::PORTB = [0, 1, 2, 3, 4, 5, 6, 7],
       // C: at90can128::PORTC = [0, 1, 2, 3, 4, 5, 6, 7],
        D: at90can128::PORTD = [0, 1, 2, 3, 4, 5, 6, 7],
        E: at90can128::PORTE = [0, 1, 2, 3, 4, 5, 6, 7],
        F: at90can128::PORTF = [0, 1, 2, 3, 4, 5, 6, 7],
        G: at90can128::PORTG = [0, 1, 2, 3, 4],
    }
}
