pub mod io_bus_8;

use embedded_hal::digital::{InputPin, OutputPin};

use crate::ft240x::io_bus_8::IoBus8;

pub struct Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU> {
    bus: BUS,     // port used write/read from FT240
    sense: SENSE, // input to tell if USB is connected
    rxf: RXF,     // input to tell when data can be read from the FT240.
    txe: TXE,     // input to tell when the FT240 can accept data.
    rd: RD,       // output to have the FT240 put a received byte from its FIFO to the data bus
    wr: WR,       // output to have the FT240 read data byte from data bus to its transmit FIFO
    siwu: SIWU,   // output to tell the FT240 to flush its transmit FIFO buffer to the PC
}

impl<BUS, SENSE, RXF, TXE, RD, WR, SIWU> Ft240x<BUS, SENSE, RXF, TXE, RD, WR, SIWU>
where
    BUS: IoBus8,
    SENSE: InputPin<Error = core::convert::Infallible>,
    RXF: InputPin<Error = core::convert::Infallible>,
    TXE: InputPin<Error = core::convert::Infallible>,
    RD: OutputPin<Error = core::convert::Infallible>,
    WR: OutputPin<Error = core::convert::Infallible>,
    SIWU: OutputPin<Error = core::convert::Infallible>,
{
    pub fn new(
        mut bus: BUS,
        sense: SENSE,
        rxf: RXF,
        txe: TXE,
        mut rd: RD,
        mut wr: WR,
        mut siwu: SIWU,
    ) -> Self {
        // initialize the bus as high-impedance (Input + Pull-up)
        let _ = bus.set_as_input();
        let _ = bus.write(0xff);
        // prepare initial states: active-low pins should start High (Off)
        let _ = rd.set_high();
        let _ = wr.set_high();
        let _ = siwu.set_high();

        // initialize the structure
        Self {
            bus,
            sense,
            rxf,
            txe,
            rd,
            wr,
            siwu,
        }
    }

    #[inline(always)]
    pub fn connected(&mut self) -> bool {
        self.sense.is_high().unwrap_or(false)
    }

    // when RXF is low the FT240 has data to read.
    #[inline(always)]
    pub fn can_read(&mut self) -> bool {
        self.rxf.is_low().unwrap_or(false)
    }

    // when TXE is low the FT240 can accept data.
    #[inline(always)]
    pub fn can_write(&mut self) -> bool {
        self.txe.is_low().unwrap_or(false)
    }

    // This sub will preform the required operation to read a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn read_byte(&mut self) -> u8 {
        // after every RX or TX operation we reconfigure the data bus as inputs pulled up.  therefore the ports DDR should already
        // be set properly.  all that is needed is to disable the pull-ups to allow the FT240 to drive them
        let _ = self.bus.write(0x00);
        // pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
        let _ = self.rd.set_low();
        // preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
        avr_device::asm::nop();
        // read the data
        let data = self.bus.read().unwrap_or(0);
        // release the RD line since we are done with the operation
        let _ = self.rd.set_high();
        // re-enable the pull-ups
        let _ = self.bus.write(0xff);
        // return the data
        data
    }

    // This sub will preform the required operation to transmit a byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    // TODO: HOT make this one inline assembly block
    #[inline(always)]
    pub fn write_byte(&mut self, data: u8) {
        // the data bus should currently be configured as inputs with pull-ups enabled.
        // we first need to reconfigure the port as an output
        let _ = self.bus.set_as_output();
        // put the data onto the pins
        let _ = self.bus.write(data);
        // pull the WR line low so FT240 will sample the data bus and store it to its FIFO
        let _ = self.wr.set_low();
        // preform a nop to allow time for the FT240 to sample the data bus
        avr_device::asm::nop();
        // release the WR line since we are done with the operation
        let _ = self.wr.set_high();
        // reconfigure the data bus as an input
        let _ = self.bus.set_as_input();
        //  with pull-ups enabled
        let _ = self.bus.write(0xff);
    }

    // pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    #[inline(always)]
    pub fn flush(&mut self) {
        //pull the SIWU pin low
        let _ = self.siwu.set_low();
        // preform a nop to allow time to sense the logic level change
        avr_device::asm::nop2();
        //pull the SIWU back up
        let _ = self.siwu.set_high();
    }
}
