use core::{cell::RefCell, marker::PhantomData};

use avr_device::{at90can128, interrupt::Mutex};
use avr_hal_generic::port::{
    PinOps,
    mode::{Floating, Input, Output, PullUp},
};

use crate::{
    CircularBuffer,
    hal::{PD4, PD7, PG0, PG3, PG4, Pin},
};

use at90can128::USART0;
use at90can128::USART1;

static USART1_READER: Mutex<RefCell<Option<UsartReader<USART1, PG3>>>> =
    Mutex::new(RefCell::new(None));

static USART1_WRITER: Mutex<RefCell<Option<UsartWriter<USART1, PG4>>>> =
    Mutex::new(RefCell::new(None));

pub struct UsartReader<USART, CTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    _inner_buffer: [u8; BUFFER_CAPACITY],
    cts: Pin<Output, CTS>,
    buffer: CircularBuffer,
}

pub struct UsartWriter<USART, RTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    _inner_buffer: [u8; BUFFER_CAPACITY],
    : Pin<Input<Floating>, RTS>,
    flush_char: u8,
    buffer: CircularBuffer,
}

impl<RTS, const BUFFER_CAPACITY: usize> UsartWriter<USART1, PG3, BUFFER_CAPACITY>
where
    RTS: PinOps,
{
    pub fn new(rts: Pin<Input<Floating>, RTS>, flush_char: u8) -> Self {
        // i didn't want to rewrite the circular buffer to have owned storage, first create a writer without a circular buffer
        let mut writer = Self {
            _usart: PhantomData,
            _inner_buffer: [0; BUFFER_CAPACITY],
            rts,
            flush_char,
            buffer: unsafe { core::mem::zeroed() },
        };
// put into static memory
    avr_device::interrupt::free(|cs| {
        let cell = USART1_WRITER.borrow(cs);
        
        // Move the struct off the stack and into its permanent global RAM slot
        cell.replace(Some(writer));
        
        // Step 3: Borrow a mutable reference to the moved struct *inside* the static slot.
        // It is now physically anchored in global RAM, so its address is fixed!
        if let Some(ref mut anchored_writer) = *cell.borrow_mut() {
        // get a pointer to the allocated inner buffer
        let inner_buffer_ptr = anchored_writer._inner_buffer.as_mut_ptr();
        // initialize the circular buffer
        anchored_writer.buffer = CircularBuffer::new(raw_array_ptr, BUFFER_CAPACITY);

        }
    });

        
        // return
        writer
    }

    // Step 2: Tie the pointer to the buffer AFTER it is in its final home
    // We take `&mut self` to ensure nobody can move it while we link the pointer
    pub fn init(&mut self) {
        let inner_buffer_ptr = self._inner_buffer.as_mut_ptr();
        self.buffer = CircularBuffer::new(inner_buffer_ptr, BUFFER_CAPACITY);
    }
}

// 2. Define your zero-sized Handle struct
#[derive(Clone, Copy)] // Safe and totally free to copy anywhere
pub struct UsartWriterHandle;

impl UsartWriterHandle {
    /// Safe method you can call anywhere in your application or async futures
    pub fn print_byte(&self, byte: u8) -> Result<(), ()> {
        // Enforce the critical section under the hood
        avr_device::interrupt::free(|cs| {
            if let Some(ref mut writer) = *USART1_WRITER.borrow(cs).borrow_mut() {
                // Safely push into the circular buffer anchored in global RAM
                writer.buffer.push(byte)
            } else {
                Err(()) // Global writer wasn't initialized yet
            }
        })
    }
}


// base struct for ft240x
pub struct AvrUart<USART, CTS, RTS, SENSE, RESET, DEFAULTS>
where
    CTS: PinOps,
    RTS: PinOps,
    SENSE: PinOps,
    RESET: PinOps,
    DEFAULTS: PinOps,
{
    usart: USART,
    cts: Pin<Output, CTS>,
    rts: Pin<Input<Floating>, RTS>,
    sense: Pin<Input<PullUp>, SENSE>,
    reset: Pin<Output, RESET>,
    defaults: Pin<Output, DEFAULTS>,
}

impl<CTS, RTS, SENSE, RESET, DEFAULTS> AvrUart<USART1, CTS, RTS, SENSE, RESET, DEFAULTS>
where
    CTS: PinOps,
    RTS: PinOps,
    SENSE: PinOps,
    RESET: PinOps,
    DEFAULTS: PinOps,
{
    pub fn split(
        usart: at90can128::USART1,
        cts: Pin<Input<Floating>, CTS>,
        rts: Pin<Input<Floating>, RTS>,
        sense: Pin<Input<Floating>, SENSE>,
        reset: Pin<Input<Floating>, RESET>,
        defaults: Pin<Input<Floating>, DEFAULTS>,
    ) -> (UsartReader<USART1, CTS>, UsartWriter<USART1, RTS>) {
        // control register a
        usart.ucsr1a().modify(|_, w| {
            // disable double speed
            w.u2x1().clear_bit();
            // disable multi-Processor communication mode
            w.mpcm1().clear_bit()
        });
        // control register b
        usart.ucsr1b().modify(|_, w| {
            // enable rx complete interrupt enable
            w.rxcie1().set_bit();
            // disable tx complete interrupt enable
            w.txcie1().clear_bit();
            // disable data register empty interrupt enable (we will enable this later when we transmit data)
            w.udrie1().clear_bit();
            // receiver enable
            w.rxen1().set_bit();
            // transmitter enable
            w.txen1().set_bit();
            // character size.  ucsr1b_ucsz12, ucsr1c_ucsz11 and ucsr1c_ucsz10 define the character size.  for 8-bit ucsz12 = 0, ucsz11 = 1, and ucsz10 = 1
            w.ucsz12().clear_bit()
        });
        // control register c
        usart.ucsr1c().modify(|_, w| {
            // async mode
            w.umsel1().usart_async();
            // parity mode 1 (0,0 = no parity)
            w.upm1().disabled();
            // stop bit select (0 = 1 stop-bit)
            w.usbs1().stop1();
            // character size.  ucsr1b_ucsz12, ucsr1c_ucsz11 and ucsr1c_ucsz10 define the character size.  for 8-bit ucsz12 = 0, ucsz11 = 1, and ucsz10 = 1
            w.ucsz1().chr8();
            // clock polarity (0 = transmit on rising edge & sample on falling, 1 = transmit on falling edge & sample on rising
            w.ucpol1().rising_edge()
        });
        // set the baud-rate (0x0f = 57.6k in current configuration )
        usart.ubrr1().modify(|_, w| w.set(0x0f));

        let cts = cts.into_output();
        let rts = rts.into_floating_input();
        let sense = sense.into_pull_up_input();
        let reset = reset.into_output_high();
        let defaults = defaults.into_output_high();

        Self {
            usart,
            cts,
            rts,
            sense,
            reset,
            defaults,
        }
    }
}

// // struct to own the usart and sense so it can be shared
// struct Usart<USART, SENSE_PIN> {
//     usart: USART,
//     sense: Pin<Input, SENSE_PIN>,
// }

// // struct for stashing usart
// struct StaticUsart<USART, SENSE_PIN> {
//     usart: UnsafeCell<Option<Usart<USART, SENSE_PIN>>>,
// }
// impl<USART, SENSE_PIN> StaticUsart<USART, SENSE_PIN> {
//     pub const fn empty() -> Self {
//         Self {
//             usart: UnsafeCell::new(None),
//         }
//     }
// }

// // a handle we can give to readers and writers
// struct UsartHandle<USART, SENSE_PIN> {
//     ptr: NonNull<Usart<USART, SENSE_PIN>>,
// }
// impl<USART, SENSE_PIN> Clone for UsartHandle<USART, SENSE_PIN> {
//     fn clone(&self) -> Self {
//         Self { ptr: self.ptr }
//     }
// }
// // why is this needed?  is it bad
// unsafe impl<USART, SENSE> Sync for StaticUsart<USART, SENSE> {}
// // static handle to the usart1 once its initialized
// static USART1: StaticUsart<at90can128::USART1, PD7> = StaticUsart::empty();
