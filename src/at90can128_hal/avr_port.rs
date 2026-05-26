use avr_device::at90can128;

use crate::ft240x::io_bus_8::IoBus8;

pub struct AvrPort2<PORT> {
    pub port: PORT,
}

impl IoBus8 for AvrPort2<at90can128::PORTC> {
    type Error = core::convert::Infallible;

    fn set_as_output(&mut self) -> Result<(), Self::Error> {
        // DDRC register controls pin direction (0xFF sets all 8 pins to output)
        self.port.ddrc().write(|w| unsafe { w.bits(0xFF) });
        Ok(())
    }

    fn set_as_input(&mut self) -> Result<(), Self::Error> {
        // 0x00 sets all 8 pins to high-impedance input
        self.port.ddrc().write(|w| unsafe { w.bits(0x00) });
        Ok(())
    }

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        // Drive the parallel bus data out onto the PORTC physical lines
        self.port.portc().write(|w| unsafe { w.bits(byte) });
        Ok(())
    }

    fn read(&self) -> Result<u8, Self::Error> {
        // Read the actual electrical logic levels from the physical PINC register
        Ok(self.port.pinc().read().bits())
    }
}
