use core::{cell::UnsafeCell, ptr::NonNull};

use avr_device::at90can128;
use embedded_hal::digital::InputPin;

use crate::ft240x::io_bus_8::IoBus8;

#[derive(Clone, Copy, PartialEq)]
enum State {
    Unknown,
    Input,
    Output,
}

struct InnerBus<PORT, SENSE> {
    port: PORT,
    sense: SENSE,
    state: State,
}

pub struct StaticBus<PORT, SENSE>(UnsafeCell<Option<InnerBus<PORT, SENSE>>>);
unsafe impl<PORT, SENSE> Sync for StaticBus<PORT, SENSE> {}

impl<PORT, SENSE> StaticBus<PORT, SENSE> {
    pub const fn empty() -> Self {
        Self(UnsafeCell::new(None))
    }
}

pub struct BusHandle<PORT, SENSE> {
    ptr: NonNull<InnerBus<PORT, SENSE>>,
}

impl<PORT, SENSE> BusHandle<PORT, SENSE> {
    pub fn init(port: PORT, sense: SENSE, slot: &'static StaticBus<PORT, SENSE>) -> Self {
        unsafe {
            let slot_ptr = slot.0.get();
            *slot_ptr = Some(InnerBus {
                port,
                sense,
                state: State::Unknown,
            });
            Self {
                ptr: NonNull::new_unchecked((*slot_ptr).as_mut().unwrap()),
            }
        }
    }
}

impl<PORT, SENSE> Clone for BusHandle<PORT, SENSE> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

impl<SENSE> IoBus8 for BusHandle<at90can128::PORTC, SENSE>
where
    SENSE: InputPin<Error = core::convert::Infallible>,
{
    type Error = core::convert::Infallible;

    fn replicate(&self) -> Self {
        (*self).clone()
    }

    #[inline(always)]
    fn is_connected(&mut self) -> bool {
        unsafe {
            // get the bus handle
            let bus = self.ptr.as_ptr();
            // read the pin
            (*bus).sense.is_high().unwrap_or(false)
        }
    }

    #[inline(always)]
    fn set_as_output(&mut self) -> Result<(), Self::Error> {
        unsafe {
            // get the bus handle
            let bus = self.ptr.as_ptr();
            // do we need to reconfigure
            if (*bus).state != State::Output {
                // DDRC register controls pin direction (0xFF sets all 8 pins to output)
                (*bus).port.ddrc().write(|w| w.bits(0xFF));
                // update the state
                (*bus).state = State::Output;
            }
        }
        return Ok(());
    }

    #[inline(always)]
    fn set_as_input(&mut self) -> Result<(), Self::Error> {
        unsafe {
            // get the bus handle
            let bus = self.ptr.as_ptr();
            // do we need to reconfigure
            if (*bus).state != State::Input {
                // 0x00 sets all 8 pins to high-impedance input
                (*bus).port.ddrc().write(|w| w.bits(0x00));
                // 0x00 disable pull-ups
                (*bus).port.portc().write(|w| w.bits(0x00));
                // update the state
                (*bus).state = State::Input;
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        unsafe {
            // get the bus handle
            let bus = self.ptr.as_ptr();
            // drive the parallel bus data out onto the PORTC physical lines
            (*bus).port.portc().write(|w| w.bits(byte));
        }
        Ok(())
    }

    #[inline(always)]
    fn read(&self) -> Result<u8, Self::Error> {
        unsafe {
            // get the bus handle
            let bus = self.ptr.as_ptr();
            // Read the actual electrical logic levels from the physical PINC register
            Ok((*bus).port.pinc().read().bits())
        }
    }
}
