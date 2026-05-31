pub trait IoBus8 {
    type Error;
    fn replicate(&self) -> Self
    where
        Self: Sized;
    fn is_connected(&mut self) -> bool;
    fn set_as_output(&mut self) -> Result<(), Self::Error>;
    fn set_as_input(&mut self) -> Result<(), Self::Error>;
    fn write(&mut self, byte: u8) -> Result<(), Self::Error>;
    fn read(&self) -> Result<u8, Self::Error>;
}
