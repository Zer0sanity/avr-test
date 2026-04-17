use avr_device::at90can128;

pub struct UsbFT240<'a> {
    ctl_port: &'a at90can128::porte::RegisterBlock,
    sense_port: &'a at90can128::portg::RegisterBlock,
    data_bus: &'a at90can128::portc::RegisterBlock,
}

impl<'a> UsbFT240<'a> {
    //Output 
    #[rustfmt::skip]
    pub fn new(periphals: &'a at90can128::Peripherals) -> Self {
        // control signals
        let ctl_port = &periphals.PORTE;
        ctl_port.ddre().modify(|_, w| { w
            .pe2().set_bit() // SIWU Output to tell the FT240 to flush its transmit FIFO buffer to the PC
            .pe4().set_bit() // RD Output to have the FT240 put a received byte from its FIFO to the data bus
            .pe7().set_bit() // WR Output to have the FT240 read data byte from data bus to its transmit FIFO
            .pe5().clear_bit() // TXE Input to tell when the FT240 can accept data.  Pin will also be setup to generate an interrupt on falling edge when transmitting data
            .pe6().clear_bit() // RXF Input to tell when data can be read from the FT240.  Pin will also be setup to generate an interrupt on falling edge for receiving data 
        });
        
        // pull up the outputs
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
        // periphals.EXINT.eicrb().modify(|_, w| { w
        //     .isc5().falling_edge_of_intx() // TXE(PE5/INT5)
        //     .isc6().falling_edge_of_intx() // RXF(PE6/INT6)
        // });

        // // clear flags: On AVR, write a '1' to the flag bit to clear it.
        // periphals.EXINT.eifr().write(|w| unsafe { w.bits((1 << 5) | (1 << 6)) });

        Self { ctl_port, sense_port, data_bus }
    }

    // This sub will preform the required operation to transmit byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used. 
    pub fn tx_byte(&self, data: u8)
    {
        // The data bus should currently be configured as inputs with pull-ups enabled.
        // We first need to reconfigure the port as an output
        self.data_bus.ddrc().write(|w| unsafe { w.bits(0xff) });
        // Put the data onto the pins
        self.data_bus.portc().write(|w| unsafe { w.bits(data) });
        // Pull the WR line low so FT240 will sample the data bus and store it to its FIFO
        self.ctl_port.porte().modify(|_, w| w.pe7().clear_bit());
        // Preform a nop to allow time for the FT240 to sample the data bus
        avr_device::asm::nop();
        // Release the WR line since we are done with the operation
        self.ctl_port.porte().modify(|_, w| w.pe7().set_bit());
        // Reconfigure the data bus pins as inputs with pull-ups enabled
        self.data_bus.ddrc().write(|w| unsafe { w.bits(0x00) });
        self.data_bus.portc().write(|w| unsafe { w.bits(0xFF) });        
    }

    // This sub will preform the required operation to receive a byte from the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled
    // if interrupts are being used. 
    pub fn rx_byte(&self) -> u8
    {
        // After ever RX or TX operation we reconfigure the data bus as inputs pulled up.  Therefore the ports DDR should already
        // be set properly.  All that is needed is to disable the pull-ups to allow the FT240 to drive them
        self.data_bus.portc().write(|w| unsafe { w.bits(0x00) });        
        // Pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        self.ctl_port.porte().modify(|_, w| w.pe4().clear_bit());
        // Preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data 
        avr_device::asm::nop();
        // Read the data
        let data = self.data_bus.pinc().read().bits();
        // Release the RD line since we are done with the operation
        self.ctl_port.porte().modify(|_, w| w.pe4().set_bit());
        // Re-enable the pull-ups
        self.data_bus.portc().write(|w| unsafe { w.bits(0xff) });        
        //Return the value
        data
    }

    //Routine pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    pub fn flush(&self)
    {
        //Pull the SIWU pin low
        self.ctl_port.porte().modify(|_,w| w.pe2().clear_bit());
        //Pull the SIWU back up
        self.ctl_port.porte().modify(|_,w| w.pe2().set_bit());
    }

}




// use heapless::spsc::Queue; // Single-producer, single-consumer queue

// static mut USB_BUFFER: Queue<u8, 64> = Queue::new();

// struct UsbStream<'a> {
//     consumer: Queue<u8, 64>::Consumer<'a>,
// }

// impl<'a> UsbStream<'a> {
//     async fn next_byte(&mut self) -> u8 {
//         loop {
//             if let Some(byte) = self.consumer.dequeue() {
//                 return byte;
//             }
//             // If empty, yield back to executor
//             YieldFuture.await;
//         }
//     }
// }

// #[derive(serde::Deserialize)]
// struct CanPacket {
//     id: u32,
//     data: [u8; 8],
// }

// // In your task:
// let mut raw_buf = [0u8; 16];
// // fill raw_buf from UsbStream...
// let packet: CanPacket = postcard::from_bytes(&raw_buf).unwrap();

// Local Variables:
// jinx-local-words: "nop"
// End:
