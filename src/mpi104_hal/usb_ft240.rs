use avr_device::at90can128;

pub struct UsbFT240<'a> {
    ctl_port: &'a at90can128::porte::RegisterBlock,
    sense_port: &'a at90can128::portg::RegisterBlock,
    data_bus: &'a at90can128::portc::RegisterBlock,
}

impl<'a> UsbFT240<'a> {
    #[rustfmt::skip]
    pub fn new(periphals: &'a at90can128::Peripherals) -> Self {
        // control signals
        let ctl_port = &periphals.PORTE;
        ctl_port.ddre().modify(|_, w| { w
            .pe2().set_bit() // SIWU Output
            .pe4().set_bit() // RD Output
            .pe7().set_bit() // WR Output
            .pe5().clear_bit() // TXE Input
            .pe6().clear_bit() // RXF Input
        });

        ctl_port.porte().modify(|_, w| { w
            .pe2().set_bit() // Active Low Pull-up
            .pe4().set_bit() // Active Low Pull-up
            .pe7().set_bit() // Active Low Pull-up
        });

        // USB sense
        let sense_port = &periphals.PORTG;
        // To disable pull-up/set floating on AVR, we clear the PORT bit
        sense_port.ddrg().modify(|_, w| w.pg2().clear_bit());
        sense_port.ping().modify(|_, w| w.pg2().clear_bit());

        // data bus
        let data_bus = &periphals.PORTC;
        data_bus.ddrc().write(|w| unsafe { w.bits(0x00) }); // All Inputs
        data_bus.portc().write(|w| unsafe { w.bits(0xFF) }); // All Pull-up

        // external interrupts
        periphals.EXINT.eicrb().modify(|_, w| { w
            .isc5().falling_edge_of_intx() // TXE(PE5/INT5)
            .isc6().falling_edge_of_intx() // RXF(PE6/INT6)
        });

        // clear flags: On AVR, write a '1' to the flag bit to clear it.
        periphals.EXINT.eifr().write(|w| unsafe { w.bits((1 << 5) | (1 << 6)) });

        Self { ctl_port, sense_port, data_bus }
    }
}
