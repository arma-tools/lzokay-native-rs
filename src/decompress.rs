// use anyhow::anyhow;
// use anyhow::Result;
use std::io::{BufRead, Seek, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use crate::util::{
    consume_zero_byte_length_stream, peek_u8, read_bytes, LzokayError, M3_MARKER, M4_MARKER,
};

pub fn decompress_stream<I>(
    reader: &mut I,
    expected_size: Option<usize>,
) -> Result<Vec<u8>, LzokayError>
where
    I: BufRead + Seek,
{
    let mut result = Vec::<u8>::with_capacity(expected_size.unwrap_or_default());

    let mut lbcur: u64;
    let mut lblen: usize;
    let mut state: usize = 0;
    let mut nstate: usize;

    /* First byte encoding */
    if peek_u8(reader)? >= 22 {
        /* 22..255 : copy literal string
         *           length = (byte - 17) = 4..238
         *           state = 4 [ don't copy extra literals ]
         *           skip byte
         */
        let len: usize = (reader.read_u8()? - 17) as usize;
        let written = result.write(&read_bytes(reader, len)?)?;
        assert!(written == len);
        state = 4
    } else if peek_u8(reader)? >= 18 {
        /* 18..21 : copy 0..3 literals
         *          state = (byte - 17) = 0..3  [ copy <state> literals ]
         *          skip byte
         */
        nstate = (reader.read_u8()? - 17) as usize;
        state = nstate;
        let written = result.write(&read_bytes(reader, nstate)?)?;
        assert!(written == nstate);
    }
    loop
    /* 0..17 : follow regular instruction encoding, see below. It is worth
     *         noting that codes 16 and 17 will represent a block copy from
     *         the dictionary which is empty, and that they will always be
     *         invalid at this place.
     */
    {
        let inst = reader.read_u8()?;
        if (inst as u32 & 0xc0) != 0 {
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
                - ((((reader.read_u8()? as u32) << 3) + ((inst as u32 >> 2) & 0x7) + 1) as u64);
            lblen = ((inst >> 5) as usize) + 1;
            nstate = (inst & 0x3) as usize;
        } else if (inst as u32 & M3_MARKER) != 0 {
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
                lblen += (offset * 255 + 31 + reader.read_u8()? as u64) as usize;
            }
            nstate = reader.read_u16::<LittleEndian>()? as usize;
            lbcur = result.len() as u64 - ((nstate >> 2).wrapping_add(1) as u64);
            nstate &= 0x3
        } else if inst as u32 & M4_MARKER != 0 {
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
                lblen += (offset * 255 + 7 + reader.read_u8()? as u64) as usize;
            }
            nstate = reader.read_u16::<LittleEndian>()? as usize;

            lbcur = (result.len() as u64).wrapping_sub(
                ((((inst & 0x8) as i32) << 11) as u64).wrapping_add((nstate >> 2_usize) as u64)
                    as u64,
            );

            nstate &= 0x3;
            if lbcur as u64 == result.len() as u64 {
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
                len += (offset * 255 + 15 + reader.read_u8()? as u64) as usize;
            }
            /* copy_literal_run */
            let written = result.write(&read_bytes(reader, len)?)?;
            assert!(written == len);
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
            nstate = (inst as u32 & 0x3) as usize;

            lbcur = (result.len() as u64).wrapping_sub(
                ((inst as u32) >> 2).wrapping_add(((reader.read_u8()? as u32) << 2).wrapping_add(1))
                    as u64,
            );
            lblen = 2
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
            nstate = (inst & 0x3) as usize;
            lbcur = (result.len() as u64)
                - (((inst as u32 >> 2) + ((reader.read_u8()? as u32) << 2) + 2049) as isize) as u64;
            lblen = 3
        }

        for i in 0..lblen {
            let val = result[lbcur as usize + i];
            result.write_u8(val).unwrap();
        }

        state = nstate;

        /* Copy literal */

        let written = result.write(&read_bytes(reader, nstate)?)?;
        assert!(written == nstate);
    }
    // *dst_size = outp.offset_from(dst) as usize;
    if lblen != 3 {
        /* Ensure terminating M4 was encountered */
        return Err(LzokayError::Error);
    }

    Ok(result)
}

// pub unsafe fn decompress_unsafe(
//     src: *const u8,
//     src_size: usize,
//     dst: *mut u8,
//     init_dst_size: usize,
//     mut dst_size: *mut usize,
// ) -> Result<usize, LzokayError> {
//     *dst_size = init_dst_size;
//     if src_size < 3 {
//         *dst_size = 0;
//         return Err(LzokayError::InputOverrun);
//     }
//     let mut inp: *const u8 = src;
//     let inp_end: *const u8 = src.add(src_size);
//     let mut outp: *mut u8 = dst;
//     let outp_end: *mut u8 = dst.add(*dst_size);
//     let mut lbcur: *mut u8;
//     let mut lblen: usize;
//     let mut state: usize = 0;
//     let mut nstate: usize;
//     /* First byte encoding */
//     if *inp as u32 >= 22_u32 {
//         /* 22..255 : copy literal string
//          *           length = (byte - 17) = 4..238
//          *           state = 4 [ don't copy extra literals ]
//          *           skip byte
//          */
//         let len: usize = (*inp as u32 - 17) as usize;
//         inp = inp.offset(1);
//         needs_in(inp, len, inp_end, &mut dst_size, outp, dst)?;
//         needs_out(outp, len, outp_end, &mut dst_size, dst)?;
//         let mut i: usize = 0;
//         while i < len {
//             *outp = *inp;
//             inp = inp.offset(1);
//             outp = outp.offset(1);
//             i = i.wrapping_add(1)
//         }
//         state = 4
//     } else if *inp as u32 >= 18 {
//         /* 18..21 : copy 0..3 literals
//          *          state = (byte - 17) = 0..3  [ copy <state> literals ]
//          *          skip byte
//          */
//         nstate = (*inp as u32 - 17) as usize;
//         inp = inp.offset(1);
//         state = nstate;
//         needs_in(inp, nstate, inp_end, &mut dst_size, outp, dst)?;
//         needs_out(outp, nstate, outp_end, &mut dst_size, dst)?;
//         let mut i: usize = 0;
//         while i < nstate {
//             *outp = *inp;
//             inp = inp.offset(1);
//             outp = outp.offset(1);
//             i += 1
//         }
//     }
//     loop
//     /* 0..17 : follow regular instruction encoding, see below. It is worth
//      *         noting that codes 16 and 17 will represent a block copy from
//      *         the dictionary which is empty, and that they will always be
//      *         invalid at this place.
//      */
//     {
//         needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;
//         let inst: u8 = *inp;
//         inp = inp.offset(1);
//         if (inst as u32 & 0xc0) != 0 {
//             /* [M2]
//              * 1 L L D D D S S  (128..255)
//              *   Copy 5-8 bytes from block within 2kB distance
//              *   state = S (copy S literals after this block)
//              *   length = 5 + L
//              * Always followed by exactly one byte : H H H H H H H H
//              *   distance = (H << 3) + D + 1
//              *
//              * 0 1 L D D D S S  (64..127)
//              *   Copy 3-4 bytes from block within 2kB distance
//              *   state = S (copy S literals after this block)
//              *   length = 3 + L
//              * Always followed by exactly one byte : H H H H H H H H
//              *   distance = (H << 3) + D + 1
//              */
//             needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;
//             lbcur = outp.offset(-((((*inp as u32) << 3) + (inst as u32 >> 2 & 0x7) + 1) as isize));
//             inp = inp.offset(1);
//             lblen = ((inst as u32 >> 5) as usize).wrapping_add(1);
//             nstate = (inst as u32 & 0x3) as usize
//         } else if inst as u32 & M3_MARKER != 0 {
//             /* [M3]
//              * 0 0 1 L L L L L  (32..63)
//              *   Copy of small block within 16kB distance (preferably less than 34B)
//              *   length = 2 + (L ?: 31 + (zero_bytes * 255) + non_zero_byte)
//              * Always followed by exactly one LE16 :  D D D D D D D D : D D D D D D S S
//              *   distance = D + 1
//              *   state = S (copy S literals after this block)
//              */
//             lblen = ((inst as u32 & 0x1f) as usize).wrapping_add(2);
//             if lblen == 2 {
//                 let mut offset: usize = 0;
//                 consume_zero_byte_length(&mut inp, &mut offset, &mut dst_size, outp, dst)?;
//                 needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;

//                 lblen = lblen.wrapping_add(
//                     offset
//                         .wrapping_mul(255)
//                         .wrapping_add(31)
//                         .wrapping_add(*inp as usize),
//                 );
//                 inp = inp.offset(1);
//             }
//             needs_in(inp, 2, inp_end, &mut dst_size, outp, dst)?;
//             nstate = get_le16(inp) as usize;
//             inp = inp.offset(2);
//             lbcur = outp.offset(-((nstate >> 2).wrapping_add(1) as isize));
//             nstate &= 0x3
//         } else if inst as u32 & M4_MARKER != 0 {
//             /* [M4]
//              * 0 0 0 1 H L L L  (16..31)
//              *   Copy of a block within 16..48kB distance (preferably less than 10B)
//              *   length = 2 + (L ?: 7 + (zero_bytes * 255) + non_zero_byte)
//              * Always followed by exactly one LE16 :  D D D D D D D D : D D D D D D S S
//              *   distance = 16384 + (H << 14) + D
//              *   state = S (copy S literals after this block)
//              *   End of stream is reached if distance == 16384
//              */
//             lblen = ((inst as u32 & 0x7) as usize).wrapping_add(2); /* Stream finished */
//             if lblen == 2 {
//                 let mut offset: usize = 0;
//                 consume_zero_byte_length(&mut inp, &mut offset, &mut dst_size, outp, dst)?;
//                 needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;

//                 lblen = lblen.wrapping_add(
//                     offset
//                         .wrapping_mul(255)
//                         .wrapping_add(7)
//                         .wrapping_add(*inp as usize),
//                 );
//                 inp = inp.offset(1);
//             }
//             needs_in(inp, 2, inp_end, &mut dst_size, outp, dst)?;
//             nstate = get_le16(inp) as usize;
//             inp = inp.offset(2);
//             lbcur = outp.offset(
//                 -((((inst as u32 & 0x8) << 11) as usize).wrapping_add(nstate >> 2) as isize),
//             );
//             nstate &= 0x3;
//             if lbcur == outp {
//                 break;
//             }
//             lbcur = lbcur.offset(-16384)
//         } else if state == 0 {
//             /* [M1] Depends on the number of literals copied by the last instruction. */
//             /* If last instruction did not copy any literal (state == 0), this
//              * encoding will be a copy of 4 or more literal, and must be interpreted
//              * like this :
//              *
//              *    0 0 0 0 L L L L  (0..15)  : copy long literal string
//              *    length = 3 + (L ?: 15 + (zero_bytes * 255) + non_zero_byte)
//              *    state = 4  (no extra literals are copied)
//              */
//             let mut len: usize = (inst + 3) as usize;
//             if len == 3 {
//                 let mut offset: usize = 0;
//                 consume_zero_byte_length(&mut inp, &mut offset, &mut dst_size, outp, dst)?;
//                 needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;

//                 len = len.wrapping_add(
//                     offset
//                         .wrapping_mul(255)
//                         .wrapping_add(15)
//                         .wrapping_add(*inp as usize),
//                 );
//                 inp = inp.offset(1);
//             }
//             /* copy_literal_run */
//             needs_in(inp, len, inp_end, &mut dst_size, outp, dst)?;
//             needs_out(outp, len, outp_end, &mut dst_size, dst)?;
//             let mut i: usize = 0;
//             while i < len {
//                 *outp = *inp;
//                 inp = inp.offset(1);
//                 outp = outp.offset(1);
//                 i += 1;
//             }
//             state = 4;
//             continue;
//         } else if state != 4 {
//             /* If last instruction used to copy between 1 to 3 literals (encoded in
//              * the instruction's opcode or distance), the instruction is a copy of a
//              * 2-byte block from the dictionary within a 1kB distance. It is worth
//              * noting that this instruction provides little savings since it uses 2
//              * bytes to encode a copy of 2 other bytes but it encodes the number of
//              * following literals for free. It must be interpreted like this :
//              *
//              *    0 0 0 0 D D S S  (0..15)  : copy 2 bytes from <= 1kB distance
//              *    length = 2
//              *    state = S (copy S literals after this block)
//              *  Always followed by exactly one byte : H H H H H H H H
//              *    distance = (H << 2) + D + 1
//              */
//             needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;
//             nstate = (inst as u32 & 0x3) as usize;
//             lbcur = outp.offset(-(((inst as u32 >> 2) + ((*inp as u32) << 2) + 1) as isize));
//             inp = inp.offset(1);
//             lblen = 2
//         } else {
//             /* If last instruction used to copy 4 or more literals (as detected by
//              * state == 4), the instruction becomes a copy of a 3-byte block from the
//              * dictionary from a 2..3kB distance, and must be interpreted like this :
//              *
//              *    0 0 0 0 D D S S  (0..15)  : copy 3 bytes from 2..3 kB distance
//              *    length = 3
//              *    state = S (copy S literals after this block)
//              *  Always followed by exactly one byte : H H H H H H H H
//              *    distance = (H << 2) + D + 2049
//              */
//             needs_in(inp, 1, inp_end, &mut dst_size, outp, dst)?;
//             nstate = (inst as u32 & 0x3) as usize;
//             lbcur = outp.offset(-(((inst as u32 >> 2) + ((*inp as u32) << 2) + 2049) as isize));
//             inp = inp.offset(1);
//             lblen = 3
//         }
//         if lbcur < dst {
//             *dst_size = outp.offset_from(dst) as usize as usize;
//             return Err(LzokayError::LookbehindOverrun);
//         }
//         needs_in(inp, nstate, inp_end, &mut dst_size, outp, dst)?;
//         needs_out(
//             outp,
//             lblen.wrapping_add(nstate),
//             outp_end,
//             &mut dst_size,
//             dst,
//         )?;

//         /* Copy lookbehind */
//         let mut i: usize = 0;
//         while i < lblen {
//             *outp = *lbcur;
//             lbcur = lbcur.offset(1);
//             outp = outp.offset(1);
//             i += 1
//         }
//         state = nstate;

//         /* Copy literal */
//         i = 0;
//         while i < nstate {
//             *outp = *inp;
//             inp = inp.offset(1);
//             outp = outp.offset(1);
//             i += 1
//         }
//     }
//     *dst_size = outp.offset_from(dst) as usize;
//     if lblen != 3 {
//         /* Ensure terminating M4 was encountered */
//         return Err(LzokayError::Error);
//     }

//     match inp.cmp(&inp_end) {
//         Ordering::Equal => Ok(*dst_size),
//         Ordering::Less => Err(LzokayError::InputNotConsumed),
//         _ => Err(LzokayError::InputOverrun),
//     }

//     // if inp == inp_end {
//     //     EResult_Success
//     // } else if inp < inp_end {
//     //     EResult_InputNotConsumed
//     // } else {
//     //     LzokayResult::InputOverrun
//     // }
// }

// unsafe fn consume_zero_byte_length(
//     inp: &mut *const u8,
//     offset: &mut usize,
//     dst_size: &mut *mut usize,
//     outp: *mut u8,
//     dst: *mut u8,
// ) -> Result<(), LzokayError> {
//     let old_inp: *const u8 = *inp;
//     while **inp as u32 == 0 {
//         *inp = inp.offset(1)
//     }
//     *offset = inp.offset_from(old_inp) as usize;
//     if *offset > MAX_255_COUNT {
//         **dst_size = outp.offset_from(dst) as usize;
//         return Err(LzokayError::Error);
//     }
//     Ok(())
// }

// unsafe fn needs_out(
//     outp: *mut u8,
//     len: usize,
//     outp_end: *mut u8,
//     dst_size: &mut *mut usize,
//     dst: *mut u8,
// ) -> Result<(), LzokayError> {
//     if outp.add(len) > outp_end {
//         **dst_size = outp.offset_from(dst) as usize as usize;
//         return Err(LzokayError::OutputOverrun);
//     }
//     Ok(())
// }

// unsafe fn needs_in(
//     inp: *const u8,
//     len: usize,
//     inp_end: *const u8,
//     dst_size: &mut *mut usize,
//     outp: *mut u8,
//     dst: *mut u8,
// ) -> Result<(), LzokayError> {
//     if inp.add(len) > inp_end {
//         **dst_size = outp.offset_from(dst) as usize as usize;
//         return Err(LzokayError::InputOverrun);
//     }
//     Ok(())
// }
