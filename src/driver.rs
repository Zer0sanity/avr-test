use crate::BufferHandle;

unsafe impl Sync for Transfer {}
unsafe impl Send for Transfer {}

#[derive(Copy, Clone)]
pub struct Transfer {
    pub data_ptr: *const u8,
    pub length: u8,
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
}
