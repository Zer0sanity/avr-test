use core::slice;

use crate::BufferHandle;

unsafe impl Sync for Transfer {}
unsafe impl Send for Transfer {}

pub struct Transfer {
    _buffer: BufferHandle,
    iter: slice::Iter<'static, u8>,
}

impl Transfer {
    pub fn new(buffer: BufferHandle) -> Self {
        let iter = unsafe {
            core::mem::transmute::<core::slice::Iter<'_, u8>, core::slice::Iter<'static, u8>>(
                buffer.slice.iter(),
            )
        };

        Self {
            _buffer: buffer,
            iter,
        }
    }
}

impl Iterator for Transfer {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().copied()
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
}
