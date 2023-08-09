use std::io::{self, Read, Seek, SeekFrom};

use byteorder::ReadBytesExt;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Input was not consumed")]
    InputNotConsumed,
    #[error("Unknown Error")]
    Unknown,
    #[error("Input overrun")]
    InputOverrun,
    #[error("Output overrun")]
    OutputOverrun,
    #[error("Lookbehind Overrun")]
    LookbehindOverrun,

    #[error("read or write failed, source: {source}")]
    IOError {
        #[from]
        source: io::Error,
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

pub(crate) fn peek_u8<I>(reader: &mut I) -> io::Result<u8>
where
    I: Read + Seek,
{
    let pos = reader.stream_position()?;
    let ret = reader.read_u8()?;
    reader.seek(SeekFrom::Start(pos))?;
    Ok(ret)
}

pub(crate) fn read_bytes<I>(reader: &mut I, size: usize) -> io::Result<Vec<u8>>
where
    I: Read + Seek,
{
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

pub(crate) fn consume_zero_byte_length_stream<I>(reader: &mut I) -> Result<u64, crate::Error>
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
