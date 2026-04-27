use core::{any::Any, cell::RefCell, task::{Context, Poll}};
use crate::{TxStatus, hal::Pin};
use crate::wait_pin_state::WaitPinState;
use crate::driver::*;

use avr_device::{at90can128, interrupt::Mutex};
use avr_hal_generic::port::{self, mode};

static USB_INSTANCE: Mutex<RefCell<Option<UsbFT240>>> = Mutex::new(RefCell::new(None));
static TX_CALLBACK: Mutex<RefCell<Option<TxCallback>>> = Mutex::new(RefCell::new(None));

pub struct UsbHandle;

impl Driver for UsbHandle{
        fn connected(&self) -> bool {
        avr_device::interrupt::free(|cs| {
            USB_INSTANCE.borrow(cs).borrow().as_ref()
                .map_or(false, |usb| usb.sense.is_high())
        })
    }

    fn init_tx(&self, tx_callback: TxCallback) {
        // Register the callback globally
        avr_device::interrupt::free(|cs| {
            if let Some(usb) = USB_INSTANCE.borrow(cs).borrow_mut().as_mut() {
                // set the callback for the isr
                TX_CALLBACK.borrow(cs).set(tx_callback);
                // get the first byte
                if let TxStatus::NextByte(data) = tx_callback(){
                    usb.init_tx(data);
                }
            }
        });
    }
}

pub struct UsbFT240 {
    siwu: Pin<mode::Output>, //SIWU Output to tell the FT240 to flush its transmit FIFO buffer to the PC
    rd: Pin<mode::Output>, // RD Output to have the FT240 put a received byte from its FIFO to the data bus
    wr: Pin<mode::Output>, // WR Output to have the FT240 read data byte from data bus to its transmit FIFO
    txe: Pin<mode::Input>, // TXE Input to tell when the FT240 can accept data.  Pin will also be setup to generate an interrupt on falling edge when transmitting data
    rxf: Pin<mode::Input>, // RXF Input to tell when data can be read from the FT240.  Pin will also be setup to generate an interrupt on falling edge for receiving data
    sense: Pin<mode::Input>, // SENSE input to tell if USB is connected
    bus_ptr: *const at90can128::portc::RegisterBlock, // BUS input/output port for read/write store as a pointer so we can preform full port read and writes
    ext_int_ptr: *const at90can128::exint::RegisterBlock,
}

impl UsbFT240 {
    #[rustfmt::skip]
    pub fn init(
        siwu: Pin<mode::Output>,
        rd: Pin<mode::Output>,
        wr: Pin<mode::Output>,
        txe: Pin<mode::Input>,
        rxf: Pin<mode::Input>,
        sense: Pin<mode::Input>,
        bus_ptr: *const at90can128::portc::RegisterBlock,
        ext_int_ptr: *const at90can128::exint::RegisterBlock,
    ) -> UsbHandle {
        // Prepare initial states: active-low pins should start High (Off)
        let mut usb = Self {
            siwu, rd, wr, txe, rxf, sense, bus_ptr,ext_int_ptr
        };

        usb.siwu.set_high();
        usb.rd.set_high();
        usb.wr.set_high();
        
        unsafe {
            // initialize the bus as high-impedance (Input + Pull-up)
            let bus = &*usb.bus_ptr;
            bus.ddrc().write(|w| w.bits(0x00));
            bus.portc().write(|w| w.bits(0xFF));
        }

        // initialize the static instance
        avr_device::interrupt::free(|cs| {
            *USB_INSTANCE.borrow(cs).borrow_mut() = Some(usb);
        });
        
        UsbHandle
    }

    // return the state of the sense pin 
    pub fn connected(&self) -> bool {
        self.sense.is_high()
    }

    // initialize receive interrupts
    pub fn init_rx(&self) {
        // initialize receive interrupts
        let ext_int = unsafe { &*self.ext_int_ptr };
        // setup RXF(PE6/INT6) to trigger on falling edges
        ext_int.eicrb().modify(|_, w| w.isc6().falling_edge_of_intx());
        // clear the interrupt flag
        ext_int.eifr().modify(|_, w| w.intf6.set_bit());
        // enable interrupts
        ext_int.eimsk().modify(|_, w| w.int6.set_bit());
    }

    // waits for TXE to go low.  initialize transmit interrupts and kick off the first byte
    pub fn init_tx<F>(&mut self, data: u8) {
        // wait for the TXE pin to go low
        while !self.txe.is_low(){}
        // initialize external interrupts
        let ext_int = unsafe { &*self.ext_int_ptr };
        // setup TXE(PE5/INT5) to trigger on falling edges
        ext_int.eicrb().modify(|_, w| w.isc5().falling_edge_of_intx());
        // clear the INT5 interrupt flags by writing it to true
        ext_int.eifr().modify(|_, w| w.intf5().set_bit());
        // enable interrupts so we get interrupted when the FT240 can accept the next byte
        ext_int.eimsk().modify(|_, w| w.int5().set_bit());
        // kick off the first one
        self.tx_byte(data);
    }

    // pub async fn init_tx<F>(&mut self, tx_callback: TxCallback) {
    //     // wait for the TXE pin to go low
    //     WaitPinState::clear(&self.txe).await;
    //     // initialize external interrupts
    //     let ext_int = unsafe { &*self.ext_int_ptr };
    //     // setup TXE(PE5/INT5) to trigger on falling edges
    //     ext_int.eicrb().modify(|_, w| w.isc5().falling_edge_of_intx());
    //     // clear the INT5 interrupt flags by writing it to true
    //     ext_int.eifr().modify(|_, w| w.intf5().set_bit());
    //     // enable interrupts so we get interrupted when the FT240 can accept the next byte
    //     ext_int.eimsk().modify(|_, w| w.int5().set_bit());
    //     // set the callback for the isr
    //     TX_CALLBACK.borrow(cs).set(tx_callback);
    //     // kick off the first one
    //     INT5();
    // }
    
    pub fn disable_rx(&self)
{
    let ext_int = unsafe { &*self.ext_int_ptr };
    // disable interrupts
    ext_int.eimsk().modify(|_, w| w.int6().clear_bit());
    // clear the interrupt flag
    ext_int.eifr().modify(|_, w| w.intf6().clear_bit());
}
    
    pub fn disable_tx(&self)
{
    let ext_int = unsafe { &*self.ext_int_ptr };
    // disable interrupts
    ext_int.eimsk().modify(|_, w| w.int5().clear_bit());
    // clear the interrupt flag
    ext_int.eifr().modify(|_, w| w.intf5().clear_bit());
}

    pub fn disable(&self)
{
    self.disable_rx();
    self.disable_tx();
}
    
    


    // This sub will preform the required operation to transmit byte to the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled if interrupts are being used.
    pub fn tx_byte(&mut self, data: u8) {
        // someone is going to cringe, but real men are unsafe
        unsafe {
            // deference the data bus
            let bus = &*self.bus_ptr;
            // The data bus should currently be configured as inputs with pull-ups enabled.
            // We first need to reconfigure the port as an output
            bus.ddrc().write(|w| w.bits(0xff));
            // Put the data onto the pins
            bus.portc().write(|w| w.bits(data));
            // Pull the WR line low so FT240 will sample the data bus and store it to its FIFO
            self.wr.set_low();
            // Preform a nop to allow time for the FT240 to sample the data bus
            avr_device::asm::nop();
            // Release the WR line since we are done with the operation
            self.wr.set_high();
            // Reconfigure the data bus pins as inputs with pull-ups enabled
            bus.ddrc().write(|w| w.bits(0x00));
            bus.portc().write(|w| w.bits(0xff));
        }
    }

    // This sub will preform the required operation to receive a byte from the FT240.
    // NOTE:  This sub is not thread safe and should be called with interrupts disabled
    // if interrupts are being used.
    pub fn rx_byte(&mut self) -> u8 {
        // someone is going to cringe, but there pronouns are her/she
        unsafe {
            // deference the data bus
            let bus = &*self.bus_ptr;
            // After every RX or TX operation we reconfigure the data bus as inputs pulled up.  Therefore the ports DDR should already
            // be set properly.  All that is needed is to disable the pull-ups to allow the FT240 to drive them
            bus.portc().write(|w| w.bits(0x00));
            // Pull the RD line low so the FT240 will present a received byte from its FIFO to the data bus
            self.rd.set_low();
            // Preform a nop to allow time for the data bus port to stabilize and the FT240 to present the data
            avr_device::asm::nop();
            // Read the data
            let data = bus.pinc().read().bits();
            // Release the RD line since we are done with the operation
            self.rd.set_high();
            // Re-enable the pull-ups
            bus.portc().write(|w| w.bits(0xff));
            //Return the value
            data
        }
    }

    //Routine pulses the SIWU(Send Immediate/PC Wake-up) line to flush the FT240s Tx FIFO to the host
    pub fn flush(&mut self) {
        //Pull the SIWU pin low
        self.siwu.set_low();
        // Preform a nop to allow time to sense the logic level change
        avr_device::asm::nop();
        //Pull the SIWU back up
        self.siwu.set_high();
    }






    
    // This sub will write the bytes in the slice to the FT240 and preform a flush
    pub fn write(&mut self, bytes: &[u8]) {
        // write each byte
        bytes.into_iter().for_each(|data| {
            self.tx_byte(*data);
        });
        // flush
        self.flush();
    }

}


//USB tx interrupt.  I if there is data to send it will transmit the next byte to the FT240 chip otherwise it will pulse the SIWU
//to flush the FT240s tx FIFO to the host and disable the tx interrupts.
#[avr_device::interrupt(at90can128)]
fn INT5() {
    // forge a token. this is safe because we are in an ISR.
    let cs = unsafe { avr_device::interrupt::CriticalSection::new() };
    // try to get the usb instance
    if let Some(usb) = USB_INSTANCE.borrow(cs).borrow_mut().as_mut() {
        // now get the call back to see how to proceed
        if let Some(callback) = TX_CALLBACK.borrow(cs).get() {
            // call the callback
            match callback() {
                // send the next byte
                TxStatus::NextByte(b) => usb.tx_byte(b),
                // flush and disable transmit interrupts
                TxStatus::Finished => {
                    usb.flush();
                    usb.disable_tx();
                    TX_CALLBACK.borrow(cs).set(None);
                }
            }            
        } else {
            usb.flush();
            usb.disable_tx();
        }
    }
}


// //USB Rx Interrupt
// #pragma vector = INT6_vect
// __interrupt static void int6(void)
// {
//     //Read the data from the FT240 chip
//     USBRxStateObject->RxData = ReceiveByteFromFT240(); 
//     //Execute callback to receive the byte
//     RxAction(); 
// }


// Local Variables:
// jinx-local-words: #("isr nop tx usb" 4 7 (jinx--group "Suggestions from session" jinx--prefix #("07 " 0 3 (face jinx-key)) jinx--suffix nil))
// End:
