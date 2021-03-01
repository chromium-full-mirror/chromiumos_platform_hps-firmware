use hal::serial::{BasicConfig, Serial};

pub struct Usart {
    pub serial: Serial<hal::stm32::USART2, BasicConfig>,
}

impl ufmt_write::uWrite for Usart {
    type Error = ();

    fn write_str(&mut self, s: &str) -> Result<(), Self::Error> {
        use core::fmt::Write;
        self.serial.write_str(s).map_err(drop)
    }
}
