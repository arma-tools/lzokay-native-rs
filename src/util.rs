#[cfg(feature = "decompress")]
use std::io::{self, Read, Seek, SeekFrom};

#[cfg(feature = "decompress")]
use byteorder::ReadBytesExt;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unknown Error")]
    Unknown,
    #[error("Output overrun")]
    OutputOverrun,

    #[error("read or write failed, source: {0}")]
    IOError(#[from] std::io::Error),
}

// pub(crate) static mut MAX_255_COUNT: usize = ((!0) as usize / 255 - 2) as usize;
#[cfg(feature = "compress")]
pub static mut M1_MAX_OFFSET: u32 = 0x400;
#[cfg(feature = "compress")]
pub static mut M2_MAX_OFFSET: u32 = 0x800;
#[cfg(feature = "compress")]
pub static mut M3_MAX_OFFSET: u32 = 0x4000;
#[cfg(feature = "compress")]
pub static mut M2_MIN_LEN: u32 = 3;
#[cfg(feature = "compress")]
pub static mut M2_MAX_LEN: u32 = 8;
#[cfg(feature = "compress")]
pub static mut M3_MAX_LEN: u32 = 33;
#[cfg(feature = "compress")]
pub static mut M4_MAX_LEN: u32 = 9;
#[cfg(feature = "compress")]
pub static mut M1_MARKER: u32 = 0;
#[cfg(any(feature = "compress", feature = "decompress"))]
pub const M3_MARKER: u32 = 0x20;
#[cfg(any(feature = "compress", feature = "decompress"))]
pub const M4_MARKER: u32 = 0x10;

#[cfg(feature = "decompress")]
pub fn peek_u8<I>(reader: &mut I) -> io::Result<u8>
where
    I: Read + Seek,
{
    let pos = reader.stream_position()?;
    let ret = reader.read_u8()?;
    reader.seek(SeekFrom::Start(pos))?;
    Ok(ret)
}

#[cfg(feature = "decompress")]
pub fn read_bytes<I>(reader: &mut I, size: usize) -> io::Result<Vec<u8>>
where
    I: Read + Seek,
{
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(feature = "decompress")]
pub fn consume_zero_byte_length_stream<I>(reader: &mut I) -> Result<u64, crate::Error>
where
    I: Read + Seek,
{
    let old_pos = reader.stream_position()?;

    while peek_u8(reader)? == 0 {
        reader.seek(SeekFrom::Current(1))?;
    }

    let offset = reader.stream_position()? - old_pos;

    Ok(offset)
}
