use heapless::spsc::Queue;

// bit mask for the different usb states
pub mod usb_state {
    pub const IDLE: u8 = 1 << 0;
    pub const TRANSMITTING: u8 = 1 << 1;
    pub const WAITING_FOR_SPACE: u8 = 1 << 2;
    pub const SPACE_AVAILABLE: u8 = 1 << 3;
    pub const DATA_RECEIVED: u8 = 1 << 4;
}

// static state
static USB_STATUS: AtomicU8 = AtomicU8::new(usb_state::IDLE | usb_state::SPACE_AVAILABLE);
// if waiting on space this is the space required
static SPACE_REQUIRED: AtomicU8 = AtomicU8::new(0);

pub struct UsbEvent {
    event_mask: u8,
}

impl UsbEvent {
    pub fn idle() -> Self {
        Self {
            event_mask: usb_state::IDLE,
        }
    }

    pub fn space_available(bytes_required: u8) -> Self {
        // disable interrupts and update the static required space
        //...
        Self {
            event_mask: usb_state::SPACE_AVAILABLE,
        }
    }
}

impl Future for UsbEvent {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current_status = USB_STATUS.load(Ordering::Relaxed);

        if (current_status & self.target_mask) != 0 {
            Poll::Ready(())
        } else {
            // Signal the executor that we are waiting on a USB event
            // Using your READY_MASK pattern
            READY_MASK.fetch_or(1 << USB_EVENT_BIT, Ordering::Relaxed);
            Poll::Pending
        }
    }
}

// The data buffer

pub struct UsbDriver {
    usb: UsbFT240, // Your existing raw struct with the parallel bus ptr
    tx_bytes: Queue<u8, 255> = Queue::new()
}

impl UsbDriver {
    pub async fn write(&mut self, data: &[u8]) {
        for &byte in data {
            // 1. Wait for SPACE_AVAILABLE state
            UsbEvent::space_available(data.len() as u8).await;

            // 2. Perform the write
            self.hw.tx_byte_raw(byte);

            // 3. Update state: We just filled the hardware FIFO,
            // so space might not be available anymore.
            if self.hw.txe_is_busy() {
                USB_STATUS.fetch_and(!usb_state::SPACE_AVAILABLE, Ordering::Relaxed);
            }
        }

        // 4. Mark as IDLE once done
        USB_STATUS.fetch_or(usb_state::IDLE, Ordering::Relaxed);
    }

    //    transmitting
    //     USB_STATUS.fetch_and(!usb_state::IDLE, Ordering::Relaxed);
    // USB_STATUS.fetch_or(usb_state::TRANSMITTING, Ordering::Relaxed);

    // space available
    // USB_STATUS.fetch_or(usb_state::SPACE_AVAILABLE, Ordering::Relaxed);
    // READY_MASK.fetch_or(1 << USB_EVENT_BIT, Ordering::Relaxed);

    /// The "Worker" task that runs in your executor
    pub async fn worker_task(&mut self) {
        loop {
            // Wait for Bit 2 (Data in Queue) AND Bit 4 (Hardware Ready)
            let ready = READY_MASK.load(Ordering::Relaxed);
            if (ready & (1 << 2)) != 0 && (ready & (1 << 4)) != 0 {
                if let Some(byte) = interrupt::free(|_| unsafe { TX_QUEUE.dequeue() }) {
                    self.hw.tx_byte_raw(byte); // Fast parallel write

                    // After sending, we just made space! Signal bit 3
                    READY_MASK.fetch_or(1 << 3, Ordering::Relaxed);
                } else {
                    // Queue became empty
                    READY_MASK.fetch_and(!(1 << 2), Ordering::Relaxed);
                }
            } else {
                // Nothing to do, yield to let LEDs blink
                YieldNow::new().await;
            }
        }
    }
}
