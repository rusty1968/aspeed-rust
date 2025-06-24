use core::fmt::Debug;  
use proposed_traits_latest::block_device::{BlockAddress, BlockDevice, BlockRange, Error as BlockDeviceError, ErrorKind as BlockDeviceErrorKind, ErrorType as BlockDeviceErrorType};

use crate::spi::norflash::SpiNorDevice;  
  
/// It wraps the underlying device's error type `E` and adds logical errors.  
#[derive(Debug)]  
pub enum Error<E: Debug> {  
    /// An error occurred during a read operation.  
    Read(E),  
    /// An error occurred during a program (write) operation.  
    Program(E),  
    /// An error occurred during an erase operation.  
    Erase(E),  
    /// The requested operation is outside the bounds of the device.  
    OutOfBounds,  
    /// The provided data buffer has a size that is not a multiple of the program size.  
    InvalidProgramSize,  
}  
  
impl<E: Debug> BlockDeviceError for Error<E> {  
    fn kind(&self) -> BlockDeviceErrorKind {  
        match self {  
            Error::Read(_) => BlockDeviceErrorKind::ReadError,  
            Error::Program(_) => BlockDeviceErrorKind::ProgramError,  
            Error::Erase(_) => BlockDeviceErrorKind::EraseError,  
            Error::OutOfBounds => BlockDeviceErrorKind::OutOfBounds,  
            Error::InvalidProgramSize => BlockDeviceErrorKind::ProgramError,  
        }  
    }  
}  
  
/// An adapter to expose a `SpiNorDevice` as a `block_device::BlockDevice`.  
pub struct SpiNorBlockDevice<D: SpiNorDevice> {  
    /// The underlying SPI NOR flash device.  
    nor: D,  
    /// The size of an erasable sector in bytes.  
    sector_size: usize,  
    /// The size of a programmable page in bytes.  
    page_size: usize,  
    /// The total capacity of the flash device in bytes.  
    capacity: usize,  
}  
  
impl<D: SpiNorDevice> SpiNorBlockDevice<D> {  
    /// Creates a new `SpiNorBlockDevice` adapter.  
    ///  
    /// # Arguments  
    ///  
    /// * `nor`: The underlying `SpiNorDevice` implementation.  
    /// * `sector_size`: The size of an erase sector in bytes.  
    /// * `page_size`: The size of a program page in bytes.  
    /// * `capacity`: The total device capacity in bytes.  
    pub fn new(nor: D, sector_size: usize, page_size: usize, capacity: usize) -> Self {  
        Self {  
            nor,  
            sector_size,  
            page_size,  
            capacity,  
        }  
    }  
}  
  
impl<D: SpiNorDevice> BlockDeviceErrorType for SpiNorBlockDevice<D>
where
    D::Error: Debug,
{
    type Error = Error<D::Error>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SpiNorBlockDeviceAddress(u32);

// Implement the marker trait `BlockAddress` for `SpiNorBlockDeviceAddress` so we can use it.
impl BlockAddress for SpiNorBlockDeviceAddress {}

impl<D: SpiNorDevice> BlockDevice for SpiNorBlockDevice<D>
where
    D::Error: Debug,
{
  
    fn read_size(&self) -> usize {  
        // The smallest readable unit is defined as a page.  
        self.page_size  
    }  
  
    fn program_size(&self) -> usize {  
        // The smallest programmable unit is a page.  
        self.page_size  
    }  
  
    fn erase_size(&self) -> usize {  
        // The smallest erasable unit is a sector.  
        self.sector_size  
    }  
  
    fn capacity(&self) -> usize {  
        self.capacity  
    }  
  
    fn read(&mut self, address: Self::Address, data: &mut [u8]) -> Result<(), Self::Error> {  
        let byte_address = (address.0 as usize) * self.read_size();  
        if byte_address + data.len() > self.capacity {  
            return Err(Error::OutOfBounds);  
        }  
  
        self.nor  
            .nor_read_data(byte_address as u32, data)  
            .map_err(Error::Read)  
    }  
  
    fn program(&mut self, address: Self::Address, data: &[u8]) -> Result<(), Self::Error> {  
        if data.len() % self.program_size() != 0 {  
            return Err(Error::InvalidProgramSize);  
        }  
  
        let mut byte_address = (address.0 as usize) * self.program_size();  
        if byte_address + data.len() > self.capacity {  
            return Err(Error::OutOfBounds);  
        }  
  
        for chunk in data.chunks(self.program_size()) {  
            self.nor.nor_write_enable().map_err(Error::Program)?;  
            self.nor  
                .nor_page_program(byte_address as u32, chunk)  
                .map_err(Error::Program)?;  
            self.nor.nor_wait_until_ready().map_err(Error::Program)?;  
            byte_address += chunk.len();  
        }  
  
        Ok(())  
    }  
  
    fn erase(&mut self, range: BlockRange<Self::Address>) -> Result<(), Self::Error> {  
        let start_byte_address = (range.start.0 as usize) * self.erase_size();  
        let end_byte_address = start_byte_address + range.count * self.erase_size();  
  
        if end_byte_address > self.capacity {  
            return Err(Error::OutOfBounds);  
        }  
  
        for i in 0..range.count {  
            let current_byte_address = start_byte_address + i * self.erase_size();  
            self.nor.nor_write_enable().map_err(Error::Erase)?;  
            self.nor  
                .nor_sector_erase(current_byte_address as u32)  
                .map_err(Error::Erase)?;  
            self.nor.nor_wait_until_ready().map_err(Error::Erase)?;  
        }  
  
        Ok(())  
    }

    type Address = SpiNorBlockDeviceAddress;
}