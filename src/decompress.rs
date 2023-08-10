use std::io::{Read, Seek, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::util::{consume_zero_byte_length_stream, peek_u8, read_bytes, M3_MARKER, M4_MARKER};

pub fn decompress<I>(reader: &mut I, expected_size: Option<usize>) -> Result<Vec<u8>, crate::Error>
where
    I: Read + Seek,
{
    let mut result = Vec::<u8>::with_capacity(expected_size.unwrap_or_default());

    let mut lbcur: u64;
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
        result.write_all(&read_bytes(reader, len)?)?;
        state = 4;
    } else if peek_u8(reader)? >= 18 {
        /* 18..21 : copy 0..3 literals
         *          state = (byte - 17) = 0..3  [ copy <state> literals ]
         *          skip byte
         */
        n_state = (reader.read_u8()? - 17) as usize;
        state = n_state;
        result.write_all(&read_bytes(reader, n_state)?)?;
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
            lbcur = result.len() as u64
                - u64::from(
                    (u32::from(reader.read_u8()?) << 3) + ((u32::from(inst) >> 2) & 0x7) + 1,
                );
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
            lbcur = result.len() as u64 - ((n_state >> 2).wrapping_add(1) as u64);
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

            lbcur = (result.len() as u64).wrapping_sub(
                ((i32::from(inst & 0x8) << 11) as u64).wrapping_add((n_state >> 2_usize) as u64),
            );

            n_state &= 0x3;
            if lbcur == result.len() as u64 {
                break;
            }
            lbcur -= 16384;
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
            result.write_all(&read_bytes(reader, len)?)?;
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

            lbcur = (result.len() as u64).wrapping_sub(u64::from(
                (u32::from(inst) >> 2)
                    .wrapping_add((u32::from(reader.read_u8()?) << 2).wrapping_add(1)),
            ));
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
            lbcur = (result.len() as u64)
                - (((u32::from(inst) >> 2) + (u32::from(reader.read_u8()?) << 2) + 2049) as isize)
                    as u64;
            lblen = 3;
        }

        for i in 0..lblen {
            let val = result[lbcur as usize + i];
            result.write_u8(val)?;
        }

        state = n_state;

        /* Copy literal */

        result.write_all(&read_bytes(reader, n_state)?)?;
    }
    // *dst_size = outp.offset_from(dst) as usize;
    if lblen != 3 {
        /* Ensure terminating M4 was encountered */
        return Err(crate::Error::Unknown);
    }

    result.flush()?;

    Ok(result)
}

pub fn decompress_all(data: &[u8], expected_size: Option<usize>) -> Result<Vec<u8>, crate::Error> {
    let mut data_reader = std::io::Cursor::new(data);

    decompress(&mut data_reader, expected_size)
}
