use core::{
    mem::transmute,
    slice::{self, Iter},
};

use crate::BufferHandle;

unsafe impl Sync for Transfer {}
unsafe impl Send for Transfer {}

pub struct Transfer {
    _buffer: BufferHandle,
    iter: Iter<'static, u8>,
}

impl Transfer {
    pub fn new(buffer: BufferHandle) -> Self {
        let iter = unsafe {
            transmute::<Iter<'_, u8>, Iter<'static, u8>>(
                buffer.slice[..buffer.length() as usize].iter(),
            )
        };

        Self {
            _buffer: buffer,
            iter,
        }
    }

    pub fn write_next(&mut self, byte: u8) -> Result<bool, ()> {
        if let Some(slot) = self.iter.next() {
            Ok(true)
            // *slot = byte;
            // self.buffer.write_pos += 1; // Keep handle in sync

            // // Logic to determine if packet is done (e.g., CRC check, EOP char, or Full)
            // Ok(self.is_packet_complete(byte))
        } else {
            Err(()) // Buffer overflow
        }
    }

    // fn is_packet_complete(&self, last_byte: u8) -> bool {
    //     // Your custom logic here
    //     last_byte == b'\n' || self.buffer.write_pos == self.buffer.slice.len() as u8
    // }
}

impl Iterator for Transfer {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().copied()
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}
