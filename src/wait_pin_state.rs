use core::task::{Context, Poll};

use avr_hal_generic::port::mode;

use crate::hal::Pin;

pub struct WaitPinState<'a> {
    state: bool,
    pin_to_check: &'a Pin<mode::Input>,
}

impl<'a> WaitPinState<'a> {
    pub fn clear(pin_to_check: &'a Pin<mode::Input>) -> Self {
        Self {
            state: false,
            pin_to_check,
        }
    }

    pub fn set(pin_to_check: &'a Pin<mode::Input>) -> Self {
        Self {
            state: true,
            pin_to_check,
        }
    }
}

impl<'a> Future for WaitPinState<'a> {
    type Output = ();
    fn poll(self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.pin_to_check.is_high() == self.state {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
