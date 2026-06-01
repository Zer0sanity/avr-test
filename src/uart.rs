// #define MPORT_PC_TX                       PORTD_PORTD3
// #define MPORT_PC_TX_DDR                   DDRD_DDD3
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

// //Configure the pins used by the radio module
// MPORT_PC_RX = PIN_PULL_DOWN;
// MPORT_PC_RX_DDR = PIN_FUNCTION_INPUT;
// //Set the TX pin as an output, pulled down
// MPORT_PC_TX = PIN_PULL_DOWN;
// MPORT_PC_TX_DDR = PIN_FUNCTION_OUTPUT;
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
// //Set the control register B
// UCSR1B_RXCIE1 = true;   //RX Complete Interrupt Enable
// UCSR1B_TXCIE1 = false;  //TX Complete Interrupt Enable
// UCSR1B_UDRIE1 = false;  //Data Register Empty Interrupt Enable (We will enable this later when we transmit data)
// UCSR1B_RXEN1 = true;    //Receiver Enable
// UCSR1B_TXEN1 = true;    //Transmitter Enable
// UCSR1B_UCSZ12 = 0;      //Character Size.  UCSR1B_UCSZ12, UCSR1C_UCSZ11 and UCSR1C_UCSZ10 define the character size.  For 8-bit UCSZ12 = 0, UCSZ11 = 1, and UCSZ10 = 1
// //Set the control register C
// UCSR1C_UMSEL1 = false;  //USART1 Mode Select Synchronous
// UCSR1C_UPM11 = 0;       //Parity Mode 1 (0,0 = No parity)
// UCSR1C_UPM10 = 0;       //Parity Mode 0
// UCSR1C_USBS1 = 0;       //Stop bit select (0 = 1 stop-bit)
// UCSR1C_UCSZ11 = 1;      //Character Size.  UCSR1B_UCSZ12, UCSR1C_UCSZ11 and UCSR1C_UCSZ10 define the character size.  For 8-bit UCSZ12 = 0, UCSZ11 = 1, and UCSZ10 = 1
// UCSR1C_UCSZ10 = 1;      //Character Size.  UCSR1B_UCSZ12, UCSR1C_UCSZ11 and UCSR1C_UCSZ10 define the character size.  For 8-bit UCSZ12 = 0, UCSZ11 = 1, and UCSZ10 = 1
// UCSR1C_UCPOL1 = 0;      //Clock Polarity (0 = Transmit on rising edge & sample on falling, 1 = Transmit on falling edge & sample on rising
// //Set the baudrate
// UBRR1L = BaudRate;
// UBRR1H = 0;
// rxtx register
// UDR1
// tx isr en
// UCSR1B_UDRIE1

use avr_device::{at90can128, generic::Periph};
use avr_hal_generic::{
    hal_v0::digital::v2::InputPin,
    port::mode::{Floating, Input, Output, PullUp},
};

use crate::hal::{PD2, PD3, PD4, PD7, PG0, PG3, PG4, Pin};

// base struct for ft240x
pub struct Uart<PORT, RX, TX, RTS, CTS, SENSE, RESET, DEFAULTS> {
    port: PORT,
    rx: RX,
    tx: TX,
    rts: RTS,
    cts: CTS,
    sense: SENSE,
    reset: RESET,
    defaults: DEFAULTS,
}

impl
    Uart<
        at90can128::USART1,
        Pin<Input<Floating>, PD2>,
        Pin<Output, PD3>,
        Pin<Output, PG3>,
        Pin<Input<Floating>, PG4>,
        Pin<Input<PullUp>, PD7>,
        Pin<Output, PD4>,
        Pin<Output, PG0>,
    >
{
    pub fn new(
        port: at90can128::USART1,
        rx: Pin<Input<Floating>, PD2>,
        tx: Pin<Input<Floating>, PD3>,
        rts: Pin<Input<Floating>, PG3>,
        cts: Pin<Input<Floating>, PG4>,
        sense: Pin<Input<Floating>, PD7>,
        reset: Pin<Input<Floating>, PD4>,
        defaults: Pin<Input<Floating>, PG0>,
    ) -> Self {
        let rx = rx.into_floating_input();
        let tx = tx.into_output();
        let rts = rts.into_output();
        let cts = cts.into_floating_input();
        let sense = sense.into_pull_up_input();
        let reset = reset.into_output_high();
        let defaults = defaults.into_output_high();

        Self {
            port,
            rx,
            tx,
            rts,
            cts,
            sense,
            reset,
            defaults,
        }
    }
}
