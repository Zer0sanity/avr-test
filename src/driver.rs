use crate::BufferHandle;

pub enum TxStatus {
    NextByte(u8),
    Finished,
}

pub type TxCallback = fn() -> TxStatus;

pub trait Driver {
    fn connected(&self) -> bool;
    fn tx_submit(&mut self, buffer_handle: BufferHandle, length: u8);
}
