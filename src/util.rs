#[cfg(feature = "decompress")]
use std::io::{self, Read};

#[cfg(feature = "decompress")]
use byteorder::ReadBytesExt;

/// The various errors that can be reported by this crate.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Unknown error
    #[error("Unknown Error")]
    Unknown,

    /// Output overrun
    #[error("Output overrun")]
    OutputOverrun,

    /// A match of the compressed data reaches before the start of the output: the data is corrupt
    #[error("Lookbehind overrun")]
    LookbehindOverrun,

    /// `IOError`
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

/// Appends `size` bytes of `reader` to `out`, without a temporary buffer.
#[cfg(feature = "decompress")]
pub fn read_bytes_into<I>(reader: &mut I, out: &mut Vec<u8>, size: usize) -> io::Result<()>
where
    I: Read,
{
    let read = reader.take(size as u64).read_to_end(out)?;
    if read == size {
        Ok(())
    } else {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
}

/// Counts the zero bytes of a length extension and returns it with the non-zero byte after them.
///
/// Reads forward only: seeking back to peek discards the buffer of a `BufReader`.
#[cfg(feature = "decompress")]
pub fn consume_zero_byte_length_stream<I>(reader: &mut I) -> Result<(u64, u8), crate::Error>
where
    I: Read,
{
    let mut zeros = 0;
    loop {
        match reader.read_u8()? {
            0 => zeros += 1,
            byte => return Ok((zeros, byte)),
        }
    }
}
