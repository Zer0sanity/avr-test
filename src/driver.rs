pub enum TxStatus {
    NextByte(u8),
    Finished,
}

pub type TxCallback = fn() -> TxStatus;

pub trait Driver {
    fn connected() -> bool;
    fn init_tx(&self, tx_callback: TxCallback);
}
