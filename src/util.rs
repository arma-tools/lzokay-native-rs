use std::io::{self, BufRead, Seek, SeekFrom};

use byteorder::ReadBytesExt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LzokayError {
    #[error("Input was not consumed")]
    InputNotConsumed, // = 1,
    #[error("Error")]
    Error, // = -1,
    #[error("Input overrun")]
    InputOverrun, // = -2,
    #[error("Output overrun")]
    OutputOverrun, // = -3,
    #[error("Lookbehind Overrun")]
    LookbehindOverrun, // = -4,

    #[error("read or write failed, source: {source}; Backtrace:")]
    IOError {
        #[from]
        source: io::Error,
        // #[backtrace]
        // backtrace: Backtrace,
    },
}

// pub(crate) static mut MAX_255_COUNT: usize = ((!0) as usize / 255 - 2) as usize;
pub(crate) static mut M1_MAX_OFFSET: u32 = 0x400;
pub(crate) static mut M2_MAX_OFFSET: u32 = 0x800;
pub(crate) static mut M3_MAX_OFFSET: u32 = 0x4000;
pub(crate) static mut M2_MIN_LEN: u32 = 3;
pub(crate) static mut M2_MAX_LEN: u32 = 8;
pub(crate) static mut M3_MAX_LEN: u32 = 33;
pub(crate) static mut M4_MAX_LEN: u32 = 9;
pub(crate) static mut M1_MARKER: u32 = 0;
pub(crate) const M3_MARKER: u32 = 0x20;
pub(crate) const M4_MARKER: u32 = 0x10;

// pub(crate) unsafe fn get_le16(p: *const u8) -> u16 {
//     *(p as *const u16)
// }

pub(crate) fn peek_u8<I>(reader: &mut I) -> io::Result<u8>
where
    I: BufRead + Seek,
{
    let pos = reader.seek(SeekFrom::Current(0))?;
    let ret = reader.read_u8()?;
    reader.seek(SeekFrom::Start(pos))?;
    Ok(ret)
}

pub(crate) fn read_bytes<I>(reader: &mut I, size: usize) -> io::Result<Vec<u8>>
where
    I: BufRead + Seek,
{
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

pub(crate) fn consume_zero_byte_length_stream<I>(reader: &mut I) -> Result<u64, LzokayError>
where
    I: BufRead + Seek,
{
    let old_pos = reader.seek(SeekFrom::Current(0))?;

    while peek_u8(reader)? == 0 {
        reader.seek(SeekFrom::Current(1))?;
    }

    let offset = reader.seek(SeekFrom::Current(0))? - old_pos;

    Ok(offset)
}
