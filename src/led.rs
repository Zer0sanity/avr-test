use crate::hal::Pin;
use avr_device::at90can128;
use avr_hal_generic::port::{self, mode};

pub struct LED {
    pin: Pin<mode::Output>,
    active_low: bool,
}

impl LED {
    pub fn new(pin: Pin<mode::Output>, active_low: bool) -> Self {
        Self { pin, active_low }
    }

    pub fn on(&mut self) {
        if self.active_low {
            self.pin.set_low()
        } else {
            self.pin.set_high()
        }
    }

    pub fn off(&mut self) {
        if self.active_low {
            self.pin.set_high()
        } else {
            self.pin.set_low()
        }
    }

    pub fn toggle(&mut self) {
        self.pin.toggle();
    }

    pub fn is_on(&self) -> bool {
        if self.active_low {
            self.pin.is_set_low()
        } else {
            self.pin.is_set_high()
        }
    }
}
