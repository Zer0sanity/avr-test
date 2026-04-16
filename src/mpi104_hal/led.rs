use avr_device::at90can128;

pub trait LED {
    fn on(&self);
    fn off(&self);
    fn toggle(&self);
    fn is_on(&self) -> bool;
}

pub struct ErrLED<'a> {
    port: &'a at90can128::portb::RegisterBlock,
}

impl<'a> ErrLED<'a> {
    pub fn new(port: &'a at90can128::portb::RegisterBlock) -> Self {
        port.ddrb().modify(|_, w| w.pb6().set_bit());
        Self { port }
    }
}

impl<'a> LED for ErrLED<'a> {
    fn on(&self) {
        self.port.portb().modify(|_, w| w.pb6().clear_bit());
    }

    fn off(&self) {
        self.port.portb().modify(|_, w| w.pb6().set_bit());
    }

    fn toggle(&self) {
        self.port.pinb().modify(|_, w| w.pb6().set_bit());
    }

    fn is_on(&self) -> bool {
        self.port.pinb().read().pb6().bit_is_clear()
    }
}

impl<'a> From<&'a at90can128::Peripherals> for ErrLED<'a> {
    fn from(value: &'a at90can128::Peripherals) -> Self {
        ErrLED::new(&value.PORTB)
    }
}

pub struct CanLED<'a> {
    port: &'a at90can128::portb::RegisterBlock,
}

impl<'a> CanLED<'a> {
    pub fn new(port: &'a at90can128::portb::RegisterBlock) -> Self {
        port.ddrb().modify(|_, w| w.pb7().set_bit());
        Self { port }
    }
}

impl<'a> LED for CanLED<'a> {
    fn on(&self) {
        self.port.portb().modify(|_, w| w.pb7().clear_bit());
    }

    fn off(&self) {
        self.port.portb().modify(|_, w| w.pb7().set_bit());
    }

    fn toggle(&self) {
        self.port.pinb().modify(|_, w| w.pb7().set_bit());
    }

    fn is_on(&self) -> bool {
        self.port.pinb().read().pb7().bit_is_clear()
    }
}

impl<'a> From<&'a at90can128::Peripherals> for CanLED<'a> {
    fn from(value: &'a at90can128::Peripherals) -> Self {
        CanLED::new(&value.PORTB)
    }
}
