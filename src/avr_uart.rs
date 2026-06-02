// #define MPORT_RTS                         PORTG_PORTG3
// #define MPORT_RTS_DDR                     DDRG_DDG3
// #define MPORT_CTS                         PING_PING4
// #define MPORT_CTS_PORT                    PORTG_PORTG4
// #define MPORT_CTS_DDR                     DDRG_DDG4
// #define MPORT_RADIO_RESET                 PORTD_PORTD4
// #define MPORT_RADIO_RESET_DDR             DDRD_DDD4
// #define MPORT_RAIDO_ETHERNET_DETECT       PORTD_PORTD7
// #define MPORT_RAIDO_ETHERNET_DETECT_DDR   DDRD_DDD7
// #define MPORT_RADIO_DEFAULTS              PORTG_PORTG0
// #define MPORT_RADIO_DEFAULTS_DDR          DDRG_DDG0

// //Set the RTS pin as an output, pulled down
// MPORT_RTS = PIN_PULL_DOWN;
// MPORT_RTS_DDR = PIN_FUNCTION_OUTPUT;
// //Set the CTS pin as an input pulled low
// MPORT_CTS_PORT = PIN_PULL_DOWN;
// MPORT_CTS_DDR = PIN_FUNCTION_INPUT;
// //Set the radio reset pin to an output, pulled up
// MPORT_RADIO_RESET = PIN_PULL_UP;
// MPORT_RADIO_RESET_DDR = PIN_FUNCTION_OUTPUT;
// //Set the ethernet ditect pin as in input, pulled up
// MPORT_RAIDO_ETHERNET_DETECT = PIN_PULL_UP;
// MPORT_RAIDO_ETHERNET_DETECT_DDR = PIN_FUNCTION_INPUT;
// //Defaults pin
// MPORT_RADIO_DEFAULTS = PIN_PULL_UP;
// MPORT_RADIO_DEFAULTS_DDR = PIN_FUNCTION_OUTPUT;
// //Configure the UART
// //Set the control register A
// UCSR1A_U2X1 = false;    //Double speed
// UCSR1A_MPCM1 = false;   //Multi-Processor communication mode
// //Set the control register C
// //Set the baudrate
// UBRR1L = BaudRate;
// UBRR1H = 0;

// rxtx register
// UDR1
// tx isr en
// UCSR1B_UDRIE1

use core::{
    cell::{RefCell, UnsafeCell},
    ptr::NonNull,
};

use avr_device::{at90can128, generic::Periph, interrupt::Mutex};
use avr_hal_generic::{
    hal_v0::digital::v2::InputPin,
    port::mode::{Floating, Input, Output, PullUp},
};

use crate::{
    CircularBuffer,
    hal::{PD4, PD7, PG0, PG3, PG4, Pin},
};

// struct to own the usart and sense so it can be shared
struct Usart<USART, SENSE> {
    usart: USART,
    sense: SENSE,
}

// struct for stashing usart
struct StaticUsart<USART, SENSE> {
    usart: UnsafeCell<Option<Usart<USART, SENSE>>>,
}
impl<USART, SENSE> StaticUsart<USART, SENSE> {
    pub const fn empty() -> Self {
        Self {
            usart: UnsafeCell::new(None),
        }
    }
}
unsafe impl<USART, SENSE> Sync for StaticUsart<USART, SENSE> {}
unsafe impl<USART, SENSE> Send for StaticUsart<USART, SENSE> {}

// a handle we can give to readers and writers
struct UsartHandle<USART, SENSE> {
    ptr: NonNull<Usart<USART, SENSE>>,
}
impl<USART, SENSE> Clone for UsartHandle<USART, SENSE> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}

pub struct UsartReader<USART, CTS> {
    usart: UsartHandle<USART, CTS>,
    cts: CTS,
    buffer: CircularBuffer,
}
unsafe impl<USART, CTS> Sync for UsartReader<USART, CTS> {}
unsafe impl<USART, CTS> Send for UsartReader<USART, CTS> {}

pub struct UsartWriter<USART, RTS> {
    usart: UsartHandle<USART, RTS>,
    cts: RTS,
    flush_char: u8,
    buffer: CircularBuffer,
}
unsafe impl<USART, RTS> Sync for UsartWriter<USART, RTS> {}
unsafe impl<USART, RTS> Send for UsartWriter<USART, RTS> {}

static USART1: StaticUsart<at90can128::USART1, Pin<Input<Floating>, PD7>> = StaticUsart::empty();
static USART1_READER: Mutex<RefCell<Option<UsartReader<at90can128::USART1, Pin<Output, PG3>>>>> =
    Mutex::new(RefCell::new(None));
static USART1_WRITER: Mutex<
    RefCell<Option<UsartWriter<at90can128::USART1, Pin<Input<Floating>, PG4>>>>,
> = Mutex::new(RefCell::new(None));

// base struct for ft240x
pub struct AvrUart<USART, CTS, RTS, SENSE, RESET, DEFAULTS> {
    usart: USART,
    cts: CTS,
    rts: RTS,
    sense: SENSE,
    reset: RESET,
    defaults: DEFAULTS,
}

impl
    AvrUart<
        at90can128::USART1,
        Pin<Output, PG3>,
        Pin<Input<Floating>, PG4>,
        Pin<Input<PullUp>, PD7>,
        Pin<Output, PD4>,
        Pin<Output, PG0>,
    >
{
    pub fn new(
        usart: at90can128::USART1,
        cts: Pin<Input<Floating>, PG3>,
        rts: Pin<Input<Floating>, PG4>,
        sense: Pin<Input<Floating>, PD7>,
        reset: Pin<Input<Floating>, PD4>,
        defaults: Pin<Input<Floating>, PG0>,
    ) -> Self {
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
