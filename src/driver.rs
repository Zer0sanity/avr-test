use core::{
    mem::transmute,
    slice::{self, Iter, IterMut},
};

use crate::BufferHandle;

unsafe impl Sync for TxState {}
unsafe impl Send for TxState {}

pub struct TxState {
    _buffer: BufferHandle,
    iter: Iter<'static, u8>,
}

impl TxState {
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
}

impl Iterator for TxState {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().copied()
    }
}

unsafe impl Sync for RxState {}
unsafe impl Send for RxState {}

#[derive(Clone)]
pub enum RxStatus {
    Ready,
    Done,
}

#[derive(Clone)]
pub enum RxError {
    Overflow,
}

pub struct RxState {
    iter: IterMut<'static, u8>,
    status: Result<RxStatus, RxError>,
    _buffer: BufferHandle,
}

impl RxState {
    pub fn new(buffer: BufferHandle) -> Self {
        let iter =
            unsafe { transmute::<IterMut<'_, u8>, IterMut<'static, u8>>(buffer.slice.iter_mut()) };

        Self {
            iter,
            status: Ok(RxStatus::Ready),
            _buffer: buffer,
        }
    }

    pub fn status(&self) -> Result<RxStatus, RxError> {
        if self.iter.len() == 0 {
            return Ok(RxStatus::Done);
        }
        self.status.clone()
    }

    fn done(&self, byte: u8) -> RxStatus {
        if byte == 0x0a || self.iter.len() == 0 {
            RxStatus::Done
        } else {
            RxStatus::Ready
        }
    }

    pub fn try_receive(&mut self, byte: u8) -> Result<RxStatus, RxError> {
        let status = self
            .iter
            .next()
            .map(|slot| {
                *slot = byte;
                self.done(byte)
            })
            .ok_or(RxError::Overflow);

        self.status = status;

        return self.status.clone();
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}
