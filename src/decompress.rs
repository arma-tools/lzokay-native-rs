use std::io::{Read, Seek, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::util::{M3_MARKER, M4_MARKER, consume_zero_byte_length_stream, peek_u8, read_bytes};

/// Most bytes reserved up front for the output, whatever size the caller expects.
const MAX_INITIAL_CAPACITY: usize = 1 << 20;

/// Decompresses a lzo-compressed reader and returns the result as a new [`Vec<u8>`].
///
/// # Arguments
/// * `reader` - Reader with compressed data
/// * `expected_size` - Expected size of output. This is used as an allocation hint.
///
/// # Errors
/// Will return [`Err`] if there was any kind of I/O error while reading from `reader` or if an unexpected byte is encountered.
///
/// # Example
/// ```rust
/// # use std::fs::File;
/// # let file_path = "./test-data/compressed/fields.c.lzo";
/// let mut file = File::open(file_path).unwrap(); // file implements std::io::Read
///
/// let decompressed = lzokay_native::decompress(&mut file, None);
/// ```
///
pub fn decompress<I>(reader: &mut I, expected_size: Option<usize>) -> Result<Vec<u8>, crate::Error>
where
    I: Read + Seek,
{
    decompress_inner(reader, expected_size, None)
}

/// Decompresses a lzo-compressed reader and returns the result as a new [`Vec<u8>`].
///
/// This function fails with [`Error::OutputOverrun`](crate::Error::OutputOverrun)
/// if the decompressed output would exceed `max_size`.
///
/// # Arguments
/// * `reader` - Reader with compressed data
/// * `max_size` - Maximum allowed size of output
///
/// # Errors
/// See [`decompress`] for details on possible errors.
///
/// # Example
/// ```rust
/// # use std::fs::File;
/// # let file_path = "./test-data/compressed/fields.c.lzo";
/// let mut file = File::open(file_path).unwrap(); // file implements std::io::Read
///
/// let decompressed = lzokay_native::decompress_bounded(&mut file, 11_150);
/// ```
///
pub fn decompress_bounded<I>(reader: &mut I, max_size: usize) -> Result<Vec<u8>, crate::Error>
where
    I: Read + Seek,
{
    decompress_inner(reader, None, Some(max_size))
}

const fn ensure_output_size(
    current_size: usize,
    additional_size: usize,
    max_size: Option<usize>,
) -> Result<(), crate::Error> {
    if let Some(max_size) = max_size {
        match current_size.checked_add(additional_size) {
            Some(next_size) if next_size <= max_size => {}
            _ => return Err(crate::Error::OutputOverrun),
        }
    }

    Ok(())
}

fn write_read_bytes<I>(
    reader: &mut I,
    result: &mut Vec<u8>,
    len: usize,
    max_size: Option<usize>,
) -> Result<(), crate::Error>
where
    I: Read + Seek,
{
    ensure_output_size(result.len(), len, max_size)?;
    result.write_all(&read_bytes(reader, len)?)?;
    Ok(())
}

fn decompress_inner<I>(
    reader: &mut I,
    expected_size: Option<usize>,
    max_size: Option<usize>,
) -> Result<Vec<u8>, crate::Error>
where
    I: Read + Seek,
{
    // A hint only: a corrupt or hostile size must not allocate more than the input can produce.
    let mut result = Vec::<u8>::with_capacity(
        expected_size
            .unwrap_or_default()
            .min(max_size.unwrap_or(usize::MAX))
            .min(MAX_INITIAL_CAPACITY),
    );

    let mut distance: usize;
    let mut lblen: usize;
    let mut state: usize = 0;
    let mut n_state: usize;

    /* First byte encoding */
    if peek_u8(reader)? >= 22 {
        /* 22..255 : copy literal string
         *           length = (byte - 17) = 4..238
         *           state = 4 [ don't copy extra literals ]
         *           skip byte
         */
        let len: usize = (reader.read_u8()? - 17) as usize;
        write_read_bytes(reader, &mut result, len, max_size)?;
        state = 4;
    } else if peek_u8(reader)? >= 18 {
        /* 18..21 : copy 0..3 literals
         *          state = (byte - 17) = 0..3  [ copy <state> literals ]
         *          skip byte
         */
        n_state = (reader.read_u8()? - 17) as usize;
        state = n_state;
        write_read_bytes(reader, &mut result, n_state, max_size)?;
    }
    loop
    /* 0..17 : follow regular instruction encoding, see below. It is worth
     *         noting that codes 16 and 17 will represent a block copy from
     *         the dictionary which is empty, and that they will always be
     *         invalid at this place.
     */
    {
        let inst = reader.read_u8()?;
        if (u32::from(inst) & 0xc0) != 0 {
            /* [M2]
             * 1 L L D D D S S  (128..255)
             *   Copy 5-8 bytes from block within 2kB distance
             *   state = S (copy S literals after this block)
             *   length = 5 + L
             * Always followed by exactly one byte : H H H H H H H H
             *   distance = (H << 3) + D + 1
             *
             * 0 1 L D D D S S  (64..127)
             *   Copy 3-4 bytes from block within 2kB distance
             *   state = S (copy S literals after this block)
             *   length = 3 + L
             * Always followed by exactly one byte : H H H H H H H H
             *   distance = (H << 3) + D + 1
             */
            distance = (usize::from(reader.read_u8()?) << 3) + ((usize::from(inst) >> 2) & 0x7) + 1;
            lblen = ((inst >> 5) as usize) + 1;
            n_state = (inst & 0x3) as usize;
        } else if (u32::from(inst) & M3_MARKER) != 0 {
            /* [M3]
             * 0 0 1 L L L L L  (32..63)
             *   Copy of small block within 16kB distance (preferably less than 34B)
             *   length = 2 + (L ?: 31 + (zero_bytes * 255) + non_zero_byte)
             * Always followed by exactly one LE16 :  D D D D D D D D : D D D D D D S S
             *   distance = D + 1
             *   state = S (copy S literals after this block)
             */
            lblen = ((inst & 0x1f) as usize).wrapping_add(2);
            if lblen == 2 {
                let offset = consume_zero_byte_length_stream(reader)?;
                lblen += (offset * 255 + 31 + u64::from(reader.read_u8()?)) as usize;
            }
            n_state = reader.read_u16::<LittleEndian>()? as usize;
            distance = (n_state >> 2) + 1;
            n_state &= 0x3;
        } else if u32::from(inst) & M4_MARKER != 0 {
            /* [M4]
             * 0 0 0 1 H L L L  (16..31)
             *   Copy of a block within 16..48kB distance (preferably less than 10B)
             *   length = 2 + (L ?: 7 + (zero_bytes * 255) + non_zero_byte)
             * Always followed by exactly one LE16 :  D D D D D D D D : D D D D D D S S
             *   distance = 16384 + (H << 14) + D
             *   state = S (copy S literals after this block)
             *   End of stream is reached if distance == 16384
             */
            lblen = ((inst & 0x7) as usize).wrapping_add(2); /* Stream finished */
            if lblen == 2 {
                let offset = consume_zero_byte_length_stream(reader)?;
                lblen += (offset * 255 + 7 + u64::from(reader.read_u8()?)) as usize;
            }
            n_state = reader.read_u16::<LittleEndian>()? as usize;

            distance = (usize::from(inst & 0x8) << 11) + (n_state >> 2);

            n_state &= 0x3;
            if distance == 0 {
                break;
            }
            distance += 16384;
        } else if state == 0 {
            /* [M1] Depends on the number of literals copied by the last instruction. */
            /* If last instruction did not copy any literal (state == 0), this
             * encoding will be a copy of 4 or more literal, and must be interpreted
             * like this :
             *
             *    0 0 0 0 L L L L  (0..15)  : copy long literal string
             *    length = 3 + (L ?: 15 + (zero_bytes * 255) + non_zero_byte)
             *    state = 4  (no extra literals are copied)
             */
            let mut len: usize = (inst + 3) as usize;
            if len == 3 {
                let offset = consume_zero_byte_length_stream(reader)?;
                len += (offset * 255 + 15 + u64::from(reader.read_u8()?)) as usize;
            }
            /* copy_literal_run */
            write_read_bytes(reader, &mut result, len, max_size)?;
            state = 4;
            continue;
        } else if state != 4 {
            /* If last instruction used to copy between 1 to 3 literals (encoded in
             * the instruction's opcode or distance), the instruction is a copy of a
             * 2-byte block from the dictionary within a 1kB distance. It is worth
             * noting that this instruction provides little savings since it uses 2
             * bytes to encode a copy of 2 other bytes but it encodes the number of
             * following literals for free. It must be interpreted like this :
             *
             *    0 0 0 0 D D S S  (0..15)  : copy 2 bytes from <= 1kB distance
             *    length = 2
             *    state = S (copy S literals after this block)
             *  Always followed by exactly one byte : H H H H H H H H
             *    distance = (H << 2) + D + 1
             */
            n_state = (u32::from(inst) & 0x3) as usize;

            distance = (usize::from(inst) >> 2) + (usize::from(reader.read_u8()?) << 2) + 1;
            lblen = 2;
        } else {
            /* If last instruction used to copy 4 or more literals (as detected by
             * state == 4), the instruction becomes a copy of a 3-byte block from the
             * dictionary from a 2..3kB distance, and must be interpreted like this :
             *
             *    0 0 0 0 D D S S  (0..15)  : copy 3 bytes from 2..3 kB distance
             *    length = 3
             *    state = S (copy S literals after this block)
             *  Always followed by exactly one byte : H H H H H H H H
             *    distance = (H << 2) + D + 2049
             */
            n_state = (inst & 0x3) as usize;
            distance = (usize::from(inst) >> 2) + (usize::from(reader.read_u8()?) << 2) + 2049;
            lblen = 3;
        }

        let lbcur = result
            .len()
            .checked_sub(distance)
            .ok_or(crate::Error::LookbehindOverrun)?;
        ensure_output_size(result.len(), lblen, max_size)?;
        for i in 0..lblen {
            let val = result[lbcur + i];
            result.write_u8(val)?;
        }

        state = n_state;

        /* Copy literal */

        write_read_bytes(reader, &mut result, n_state, max_size)?;
    }
    // *dst_size = outp.offset_from(dst) as usize;
    if lblen != 3 {
        /* Ensure terminating M4 was encountered */
        return Err(crate::Error::Unknown);
    }

    result.flush()?;

    Ok(result)
}

/// Decompresses a byte slice and returns the result as a new [`Vec<u8>`].
///
/// # Arguments
/// * `data` - Data to decompress
/// * `expected_size` - Expected size of output. This is used as an allocation hint.
///
/// # Errors
/// See [`decompress`] for details on possible errors.
///
/// # Example
/// ```rust
/// let data = include_bytes!("../test-data/compressed/fields.c.lzo");
///
/// let decompressed = lzokay_native::decompress_all(data, None);
/// ```
///
pub fn decompress_all(data: &[u8], expected_size: Option<usize>) -> Result<Vec<u8>, crate::Error> {
    let mut data_reader = std::io::Cursor::new(data);

    decompress(&mut data_reader, expected_size)
}

/// Decompresses a byte slice and returns the result as a new [`Vec<u8>`].
///
/// This function fails with [`Error::OutputOverrun`](crate::Error::OutputOverrun)
/// if the decompressed output would exceed `max_size`.
///
/// # Arguments
/// * `data` - Data to decompress
/// * `max_size` - Maximum allowed size of output
///
/// # Errors
/// See [`decompress_bounded`] for details on possible errors.
///
/// # Example
/// ```rust
/// let data = include_bytes!("../test-data/compressed/fields.c.lzo");
///
/// let decompressed = lzokay_native::decompress_all_bounded(data, 11_150);
/// ```
///
pub fn decompress_all_bounded(data: &[u8], max_size: usize) -> Result<Vec<u8>, crate::Error> {
    let mut data_reader = std::io::Cursor::new(data);

    decompress_bounded(&mut data_reader, max_size)
}
