use core::{error::Error, fmt, mem::transmute, slice::Iter};

use crate::{BufferAllocator, BufferHandle};

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

#[derive(Debug)]
pub enum RxError {
    Overflow,
}

impl fmt::Display for RxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RxError")
    }
}

impl Error for RxError {}

pub trait Packetizer {
    fn receive_byte(&self, byte: u8) -> RxStatus;
}

pub struct CrPacketizer;
impl Packetizer for CrPacketizer {
    fn receive_byte(&self, byte: u8) -> RxStatus {
        if byte != 0x0a {
            RxStatus::Done
        } else {
            RxStatus::Ready
        }
    }
}

pub struct RxState {
    // current write position
    write_index: u8,
    // tracking packets
    packet_start_index: u8,
    // packetizer for packetizing the byte stream
    packetizer: CrPacketizer,
    packets: [Option<&'static [u8]>; 4],
    packet_allocator: BufferAllocator<4>,
    // store handle to buffer and our backing memory for receiving
    buffer: BufferHandle,
}

impl RxState {
    pub fn new(buffer: BufferHandle, packetizer: CrPacketizer) -> Self {
        Self {
            write_index: 0,
            packet_start_index: 0,
            packetizer,
            packets: [None; 4],
            packet_allocator: BufferAllocator::new(),
            buffer,
        }
    }

    pub fn try_receive(&mut self, byte: u8) -> Result<(), RxError> {
        if self.write_index == self.buffer.slice.len() as u8 {
            return Err(RxError::Overflow);
        }
        // write the byte
        self.buffer.slice[self.write_index as usize] = byte;
        // process the byte
        match self.packetizer.receive_byte(byte) {
            // still receiving bytes
            RxStatus::Ready => self.write_index = self.write_index + 1,
            // full packet received
            RxStatus::Done => {
                // some trickery to get a slice
                let ptr = self.buffer.slice[self.write_index as usize] as *const u8;
                let len = (self.write_index - self.packet_start_index + 1) as usize;
                let packet = unsafe { core::slice::from_raw_parts(ptr, len) };
                // push it
                if let Ok(index) = self.packet_allocator.try_alloc() {
                    self.packets[index as usize] = Some(packet);
                }
                // update the write index with wrap around
                self.write_index = (self.write_index + 1) % self.buffer.slice.len() as u8;
                // update the packet indexes
                self.packet_start_index = self.write_index;
            }
        }
        Ok(())
    }

    pub fn try_pop_packet(&mut self) -> Option<&'static [u8]> {
        if let Some(index) = self.packet_allocator.try_pop() {
            let packet = self.packets[index as usize].take();
            Some(packet?)
        } else {
            None
        }
    }
}

pub trait Driver {
    fn tx_submit(&mut self, buffer_handle: BufferHandle);
    fn rx_submit(&mut self, buffer_handle: BufferHandle);
}

// Local Variables:
// jinx-local-words: "packetizer packetizing"
// End:
