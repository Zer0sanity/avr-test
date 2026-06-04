use core::{cell::RefCell, marker::PhantomData};

use avr_device::{at90can128, interrupt::Mutex};
use avr_hal_generic::port::{
    PinOps,
    mode::{Floating, Input, Output, PullUp},
};

use crate::{
    const_circular_buffer::ConstCircularBuffer,
    hal::{PD4, PD7, PG0, PG3, PG4, Pin},
};

use at90can128::USART1;

// struct for reading the usart
pub struct UsartReader<USART, CTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    cts_opt: Option<Pin<Output, CTS>>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
}

impl UsartReader<USART1, PG3, BUFFER_CAPACITY> {
    pub const fn new() -> Self {
        Self {
            _usart: PhantomData,
            cts_opt: None,
            buffer: ConstCircularBuffer::new(),
        }
    }
    pub fn init(&mut self, cts_opt: Option<Pin<Output, PG3>>) {
        self.cts_opt = cts_opt;
        self.buffer.init();
    }
}

// struct for writing usart
pub struct UsartWriter<USART, RTS, const BUFFER_CAPACITY: usize> {
    _usart: PhantomData<USART>,
    rts_opt: Option<Pin<Input<Floating>, RTS>>,
    flush_char: Option<u8>,
    buffer: ConstCircularBuffer<BUFFER_CAPACITY>,
}

impl UsartWriter<USART1, PG4, BUFFER_CAPACITY> {
    pub const fn new() -> Self {
        Self {
            _usart: PhantomData,
            rts_opt: None,
            flush_char: None,
            buffer: ConstCircularBuffer::new(),
        }
    }
    pub fn init(&mut self, rts_opt: Option<Pin<Input<Floating>, PG4>>) {
        self.rts_opt = rts_opt;
        self.buffer.init();
    }

    #[inline(always)]
    pub fn write(&mut self, byte: u8) {
        unsafe {
            (*USART1::ptr()).udr1().as_ptr().write_volatile(byte);
        }
    }
}

// define buffer size for interrupt buffers
const BUFFER_CAPACITY: usize = 64;
// static shared usart1 reader
static USART1_READER: Mutex<RefCell<UsartReader<USART1, PG3, BUFFER_CAPACITY>>> =
    Mutex::new(RefCell::new(UsartReader::new()));
// static shared usart1 writer
static USART1_WRITER: Mutex<RefCell<UsartWriter<USART1, PG4, BUFFER_CAPACITY>>> =
    Mutex::new(RefCell::new(UsartWriter::new()));
// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1ReaderHandle;
// usart1 handle we can give out to operate on the static shared usart1 writer
pub struct Usart1WriterHandle;
impl Usart1WriterHandle {
    pub fn write(&self, byte: u8) -> Result<(), ()> {
        // Enforce the critical section under the hood
        avr_device::interrupt::free(|cs| {
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            (*writer).write(byte);
        });
        Ok(())
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

impl AvrUart<USART1, PG3, PG4, PD7, PD4, PG0> {
    pub fn init(
        usart: at90can128::USART1,
        cts: Pin<Input<Floating>, PG3>,
        rts: Pin<Input<Floating>, PG4>,
        sense: Pin<Input<Floating>, PD7>,
        reset: Pin<Input<Floating>, PD4>,
        defaults: Pin<Input<Floating>, PG0>,
    ) -> (Usart1ReaderHandle, Usart1WriterHandle) {
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
        let _sense = sense.into_pull_up_input();
        let _reset = reset.into_output_high();
        let _defaults = defaults.into_output_high();

        // put into static memory
        avr_device::interrupt::free(|cs| {
            let mut reader = USART1_READER.borrow(cs).borrow_mut();
            reader.init(Some(cts));
            let mut writer = USART1_WRITER.borrow(cs).borrow_mut();
            writer.init(Some(rts));
        });

        // return
        (Usart1ReaderHandle, Usart1WriterHandle)
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
