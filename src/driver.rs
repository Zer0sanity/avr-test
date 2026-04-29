use crate::BufferHandle;

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle, length: u8);
}
