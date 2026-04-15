use avr_device::at90can128;

pub struct LED<'a> {
    port: &'a at90can128::portb::RegisterBlock,
    pin: u8,
}

impl<'a> LED<'a> {
    pub fn new(port: &'a at90can128::portb::RegisterBlock, pin: u8) -> Self {
        port.ddrb()
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << pin)) });
        Self { port, pin }
    }

    pub fn set_on(&self) {
        self.port
            .portb()
            .modify(|r, w| unsafe { w.bits(r.bits() & !(1 << self.pin)) });
    }

    pub fn set_off(&self) {
        self.port
            .portb()
            .modify(|r, w| unsafe { w.bits(r.bits() | (1 << self.pin)) });
    }

    pub fn is_on(&self) -> bool {
        self.port.pinb().read().bits() & (1 << self.pin) == 0
    }

    pub fn toggle(&self) {
        self.port.pinb().write(|w| unsafe { w.bits(1 << self.pin) });
    }
}
