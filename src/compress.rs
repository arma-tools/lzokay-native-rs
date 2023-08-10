use std::{
    intrinsics::{copy_nonoverlapping, write_bytes},
    ptr::null,
};

use crate::{
    util::{
        M1_MARKER, M1_MAX_OFFSET, M2_MAX_LEN, M2_MAX_OFFSET, M2_MIN_LEN, M3_MARKER, M3_MAX_LEN,
        M3_MAX_OFFSET, M4_MARKER, M4_MAX_LEN,
    },
    Error,
};

#[must_use]
pub const fn compress_worst_size(uncompressed_size: usize) -> usize {
    uncompressed_size + uncompressed_size / 16 + 64 + 3
}

pub fn compress(data: &[u8]) -> Result<Vec<u8>, crate::Error> {
    compress_with_dict(data, &mut Dict::new())
}

pub fn compress_with_dict(data: &[u8], dict: &mut Dict) -> Result<Vec<u8>, crate::Error> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let worst = compress_worst_size(data.len());
    let mut dst = Vec::with_capacity(worst);
    unsafe {
        let src_buf = std::ptr::addr_of!(data[0]);
        let dst_buf = dst.as_mut_ptr();
        let mut size: usize = 0;
        let res = lzokay_compress_dict(src_buf, data.len(), dst_buf, worst, &mut size, dict);

        if let Err(err) = res {
            Err(err)
        } else {
            dst.set_len(size);
            Ok(dst)
        }
    }
}
#[derive(Debug, PartialEq, Eq, Clone)]
struct Match3 {
    pub head: Vec<u16>,
    pub chain_sz: Vec<u16>,
    pub chain: Vec<u16>,
    pub best_len: Vec<u16>,
}
/* chain-pos -> best-match-length */
/* Encoding of 2-byte data matches */
#[derive(Debug, PartialEq, Eq, Clone)]
struct Match2 {
    pub head: Vec<u16>,
}
/* 2-byte-data -> head-pos */
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Dict {
    match3: Match3,
    match2: Match2,
    buffer: Vec<u8>, //: vec![0u8; 53247],
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct State {
    pub src: *const u8,
    pub src_end: *const u8,
    pub inp: *const u8,
    pub wind_sz: u32,
    pub wind_b: u32,
    pub wind_e: u32,
    pub cycle1_countdown: u32,
    pub bufp: *const u8,
    pub buf_sz: u32,
}

unsafe fn std_mismatch(mut first1: *mut u8, last1: *mut u8, mut first2: *mut u8) -> *mut u8 {
    while first1 != last1 && u32::from(*first1) == u32::from(*first2) {
        first1 = first1.offset(1);
        first2 = first2.offset(1);
    }
    first1
}
/* Max M3 len + 1 */

impl State {
    const fn new() -> Self {
        Self {
            src: null(),
            src_end: null(),
            inp: null(),
            wind_sz: 0,
            wind_b: 0,
            wind_e: 0,
            cycle1_countdown: 0,
            bufp: null(),
            buf_sz: 0,
        }
    }

    /* Access next input byte and advance both ends of circular buffer */
    unsafe fn get_byte(&mut self, buf: *mut u8) {
        if self.inp >= self.src_end {
            if self.wind_sz > 0 {
                self.wind_sz = self.wind_sz.wrapping_sub(1);
            }
            *buf.offset(self.wind_e as isize) = 0;
            if self.wind_e < 0x800_u32 {
                *buf.offset((0xbfff_u32 + 0x800_u32).wrapping_add(self.wind_e) as isize) = 0;
            }
        } else {
            *buf.offset(self.wind_e as isize) = *self.inp;
            if self.wind_e < 0x800_u32 {
                *buf.offset((0xbfff_u32 + 0x800_u32).wrapping_add(self.wind_e) as isize) =
                    *self.inp;
            }
            self.inp = self.inp.offset(1);
        }
        self.wind_e = self.wind_e.wrapping_add(1);
        if self.wind_e == 0xbfff_u32 + 0x800_u32 {
            self.wind_e = 0;
        }
        self.wind_b = self.wind_b.wrapping_add(1);
        if self.wind_b == 0xbfff_u32 + 0x800_u32 {
            self.wind_b = 0;
        };
    }

    unsafe fn pos2off(&mut self, pos: u32) -> u32 {
        if self.wind_b > pos {
            self.wind_b.wrapping_sub(pos)
        } else {
            (0xbfff_u32 + 0x800_u32).wrapping_sub(pos.wrapping_sub(self.wind_b))
        }
    }
}

impl Match3 {
    const unsafe fn make_key(data: *const u8) -> u32 {
        let data_0 = *data.offset(0) as u32;
        let data_1 = *data.offset(1) as u32;
        let data_2 = *data.offset(2) as u32;

        ((0x9f5f_u32.wrapping_mul(((data_0 << 5 ^ data_1) << 5) ^ data_2)) >> 5) & 0x3fff_u32
    }
    unsafe fn get_head(&mut self, key: u32) -> u16 {
        if self.chain_sz[key as usize] == 0 {
            65535_u16
        } else {
            self.head[key as usize]
        }
    }
    unsafe fn init(&mut self) {
        self.chain_sz = vec![0; 16384];
    }
    unsafe fn remove(&mut self, pos: u32, b: *const u8) {
        self.chain_sz[Self::make_key(b.offset(pos as isize)) as usize] =
            self.chain_sz[Self::make_key(b.offset(pos as isize)) as usize].wrapping_sub(1);
    }
    unsafe fn advance(
        &mut self,
        s: &mut State,
        match_pos: *mut u32,
        match_count: *mut u32,
        b: *const u8,
    ) {
        let key: u32 = Self::make_key(b.offset(s.wind_b as isize));
        self.chain[s.wind_b as usize] = self.get_head(key);
        *match_pos = u32::from(self.chain[s.wind_b as usize]);
        let tmp = self.chain_sz[key as usize];
        self.chain_sz[key as usize] = self.chain_sz[key as usize].wrapping_add(1);
        *match_count = u32::from(tmp);
        if *match_count > 0x800_u32 {
            *match_count = 0x800_u32;
        }
        self.head[key as usize] = s.wind_b as u16;
    }
    unsafe fn skip_advance(&mut self, s: &mut State, b: *const u8) {
        let key: u32 = Self::make_key(b.offset(s.wind_b as isize));
        self.chain[s.wind_b as usize] = self.get_head(key);
        self.head[key as usize] = s.wind_b as u16;
        self.best_len[s.wind_b as usize] = (0x800_u32 + 1) as u16;
        self.chain_sz[key as usize] = self.chain_sz[key as usize].wrapping_add(1);
    }
}

impl Match2 {
    const unsafe fn make_key(data: *const u8) -> u32 {
        *data.offset(0) as u32 ^ ((*data.offset(1) as u32) << 8)
    }
    unsafe fn init(&mut self) {
        self.head = vec![65535_u16; 65536];
    }
    unsafe fn add(&mut self, pos: u16, b: *const u8) {
        self.head[Self::make_key(b.offset(pos as isize)) as usize] = pos;
    }
    unsafe fn remove(&mut self, pos: u32, b: *const u8) {
        let p: *mut u16 = std::ptr::addr_of_mut!(*self.head.as_mut_ptr().offset((Self::make_key
            as unsafe fn(_: *const u8) -> u32)(
            b.offset(pos as isize)
        )
            as isize,));
        if u32::from(*p) == pos {
            *p = 65535_u16;
        };
    }
    unsafe fn search(
        &mut self,
        s: &mut State,
        lb_pos: *mut u32,
        lb_len: *mut u32,
        best_pos: *mut u32,
        b: *const u8,
    ) -> bool {
        let pos: u16 = self.head[Self::make_key(b.offset(s.wind_b as isize)) as usize];
        if pos == 65535 {
            return false;
        }
        if *best_pos.offset(2) == 0 {
            *best_pos.offset(2) = u32::from(pos) + 1;
        }
        if *lb_len < 2 {
            *lb_len = 2;
            *lb_pos = u32::from(pos);
        }
        true
    }
}

impl Dict {
    #[must_use]
    pub fn new() -> Self {
        Self {
            match3: Match3 {
                head: vec![0; 16384],
                chain_sz: vec![0; 16384],
                chain: vec![0; 51199],
                best_len: vec![0; 51199],
            },
            match2: Match2 {
                head: vec![0; 65536],
            },
            buffer: vec![0; 53247],
        }
    }

    unsafe fn init(&mut self, s: &mut State, src: *const u8, src_size: usize) {
        s.cycle1_countdown = 0xbfff_u32;
        self.match3.init();
        self.match2.init();

        s.src = src;
        s.src_end = src.add(src_size);
        s.inp = src;
        s.wind_sz = if src_size as u32 > 0x800_u32 {
            0x800_u32
        } else {
            src_size as u32
        };
        s.wind_b = 0;
        s.wind_e = s.wind_sz;
        copy_nonoverlapping(s.inp, self.buffer.as_mut_ptr(), s.wind_sz as usize);

        s.inp = s.inp.offset(s.wind_sz as isize);
        if s.wind_e == (0xbfff_u32 + 0x800_u32) {
            s.wind_e = 0;
        }
        if s.wind_sz < 3 {
            write_bytes(
                self.buffer
                    .as_mut_ptr()
                    .offset(s.wind_b.wrapping_add(s.wind_sz) as isize)
                    .cast::<u8>(),
                0,
                3,
            );
        };
    }
    unsafe fn reset_next_input_entry(&mut self, s: &mut State) {
        /* Remove match from about-to-be-clobbered buffer entry */
        if s.cycle1_countdown == 0 {
            self.match3.remove(s.wind_e, self.buffer.as_mut_ptr());
            self.match2.remove(s.wind_e, self.buffer.as_mut_ptr());
        } else {
            s.cycle1_countdown = s.cycle1_countdown.wrapping_sub(1);
        };
    }
    unsafe fn advance(
        &mut self,
        s: &mut State,
        lb_off: *mut u32,
        lb_len: *mut u32,
        best_off: *mut u32,
        skip: bool,
    ) {
        if skip {
            let mut i: u32 = 0;
            while i < (*lb_len).wrapping_sub(1) {
                self.reset_next_input_entry(s);
                self.match3.skip_advance(s, self.buffer.as_mut_ptr());
                self.match2.add(s.wind_b as u16, self.buffer.as_mut_ptr());
                s.get_byte(self.buffer.as_mut_ptr());
                i = i.wrapping_add(1);
            }
        }
        *lb_len = 1;
        *lb_off = 0;
        let mut lb_pos: u32 = 0;
        let mut best_pos = [0u32; 34];
        let mut match_pos: u32 = 0;
        let mut match_count: u32 = 0;
        self.match3.advance(
            s,
            &mut match_pos,
            &mut match_count,
            self.buffer.as_mut_ptr(),
        );
        let mut best_char: i32 = i32::from(self.buffer[s.wind_b as usize]);
        let best_len: u32 = *lb_len;
        if *lb_len >= s.wind_sz {
            if s.wind_sz == 0 {
                best_char = -1;
            }
            *lb_off = 0;
            self.match3.best_len[s.wind_b as usize] = (0x800_u32 + 1) as u16;
        } else {
            if u32::from(self.match2.search(
                s,
                &mut lb_pos,
                lb_len,
                best_pos.as_mut_ptr(),
                self.buffer.as_mut_ptr(),
            )) != 0
                && s.wind_sz >= 3
            {
                let mut i_0: u32 = 0;
                while i_0 < match_count {
                    let ref_ptr: *mut u8 = self.buffer.as_mut_ptr().offset(s.wind_b as isize);
                    let match_ptr: *mut u8 = self.buffer.as_mut_ptr().offset(match_pos as isize);
                    let mismatch: *mut u8 =
                        std_mismatch(ref_ptr, ref_ptr.offset(s.wind_sz as isize), match_ptr);
                    let match_len: u64 = mismatch.offset_from(ref_ptr) as usize as u64;
                    if match_len >= 2 {
                        if match_len < 34 && best_pos[match_len as usize] == 0 {
                            best_pos[match_len as usize] = match_pos.wrapping_add(1);
                        }
                        if match_len > u64::from(*lb_len) {
                            *lb_len = match_len as u32;
                            lb_pos = match_pos;
                            if match_len == u64::from(s.wind_sz)
                                || match_len > u64::from(self.match3.best_len[match_pos as usize])
                            {
                                break;
                            }
                        }
                    }
                    i_0 = i_0.wrapping_add(1);
                    match_pos = u32::from(self.match3.chain[match_pos as usize]);
                }
            }
            if *lb_len > best_len {
                *lb_off = s.pos2off(lb_pos);
            }
            self.match3.best_len[s.wind_b as usize] = *lb_len as u16;
            let end_best_pos: *const u32 = std::ptr::addr_of_mut!(*best_pos.as_mut_ptr().add(
                (::std::mem::size_of::<[u32; 34]>()).wrapping_div(::std::mem::size_of::<u32>()),
            ));

            let mut offit: *mut u32 = best_off.offset(2);
            let mut posit: *const u32 = best_pos.as_mut_ptr().offset(2);
            while posit < end_best_pos {
                *offit = if *posit > 0 {
                    s.pos2off((*posit).wrapping_sub(1))
                } else {
                    0
                };
                posit = posit.offset(1);
                offit = offit.offset(1);
            }
        }
        self.reset_next_input_entry(s);
        self.match2.add(s.wind_b as u16, self.buffer.as_mut_ptr());
        s.get_byte(self.buffer.as_mut_ptr());
        if best_char < 0 {
            s.buf_sz = 0;
            *lb_len = 0;
            /* Signal exit */
        } else {
            s.buf_sz = s.wind_sz.wrapping_add(1);
        }
        s.bufp = s.inp.offset(-(s.buf_sz as isize));
    }
}

impl Default for Dict {
    fn default() -> Self {
        Self::new()
    }
}

unsafe fn find_better_match(best_off: *const u32, p_lb_len: *mut u32, p_lb_off: *mut u32) {
    if *p_lb_len <= M2_MIN_LEN || *p_lb_off <= M2_MAX_OFFSET {
        return;
    }
    if *p_lb_off > M2_MAX_OFFSET
        && *p_lb_len >= M2_MIN_LEN.wrapping_add(1)
        && *p_lb_len <= M2_MAX_LEN.wrapping_add(1)
        && *best_off.offset((*p_lb_len).wrapping_sub(1) as isize) != 0
        && *best_off.offset((*p_lb_len).wrapping_sub(1) as isize) <= M2_MAX_OFFSET
    {
        *p_lb_len = (*p_lb_len).wrapping_sub(1);
        *p_lb_off = *best_off.offset(*p_lb_len as isize);
    } else if *p_lb_off > M3_MAX_OFFSET
        && *p_lb_len >= M4_MAX_LEN.wrapping_add(1)
        && *p_lb_len <= M2_MAX_LEN.wrapping_add(2)
        && *best_off.offset((*p_lb_len).wrapping_sub(2) as isize) != 0
        && *best_off.offset(*p_lb_len as isize) <= M2_MAX_OFFSET
    {
        *p_lb_len = (*p_lb_len).wrapping_sub(2);
        *p_lb_off = *best_off.offset(*p_lb_len as isize);
    } else if *p_lb_off > M3_MAX_OFFSET
        && *p_lb_len >= M4_MAX_LEN.wrapping_add(1)
        && *p_lb_len <= M3_MAX_LEN.wrapping_add(1)
        && *best_off.offset((*p_lb_len).wrapping_sub(1) as isize) != 0
        && *best_off.offset((*p_lb_len).wrapping_sub(2) as isize) <= M3_MAX_OFFSET
    {
        *p_lb_len = (*p_lb_len).wrapping_sub(1);
        *p_lb_off = *best_off.offset(*p_lb_len as isize);
    };
}
unsafe fn encode_literal_run(
    outpp: *mut *mut u8,
    outp_end: *const u8,
    dst: *const u8,
    dst_size: *mut usize,
    lit_ptr: *const u8,
    lit_len: u32,
) -> Result<(), Error> {
    let mut outp: *mut u8 = *outpp;
    if outp == dst as *mut u8 && lit_len <= 238 {
        if outp.offset(1) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = 17u32.wrapping_add(lit_len) as u8;
        outp = outp.offset(1);
    } else if lit_len <= 3 {
        *outp.offset(-2) = (u32::from(*outp.offset(-2)) | lit_len) as u8;
    } else if lit_len <= 18 {
        if outp.offset(1) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = lit_len.wrapping_sub(3) as u8;
        outp = outp.offset(1);
    } else {
        if outp.offset(lit_len.wrapping_sub(18).wrapping_div(255).wrapping_add(2) as isize)
            > outp_end as *mut u8
        {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = 0;
        outp = outp.offset(1);
        let mut l = lit_len.wrapping_sub(18);
        while l > 255 {
            *outp = 0;
            outp = outp.offset(1);
            l = l.wrapping_sub(255);
        }
        *outp = l as u8;
        outp = outp.offset(1);
    }
    if outp.offset(lit_len as isize) > outp_end as *mut u8 {
        *dst_size = outp.offset_from(dst) as usize;
        return Err(Error::OutputOverrun);
    }
    copy_nonoverlapping(lit_ptr, outp, lit_len as usize);

    outp = outp.offset(lit_len as isize);
    *outpp = outp;
    Ok(())
}
#[allow(clippy::too_many_lines)]
unsafe fn encode_lookback_match(
    outpp: *mut *mut u8,
    outp_end: *const u8,
    dst: *const u8,
    dst_size: *mut usize,
    mut lb_len: u32,
    mut lb_off: u32,
    last_lit_len: u32,
) -> Result<(), Error> {
    let mut outp: *mut u8 = *outpp;
    if lb_len == 2 {
        lb_off = lb_off.wrapping_sub(1);
        if outp.offset(2) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = (M1_MARKER | ((lb_off & 0x3) << 2)) as u8;
        outp = outp.offset(1);
        *outp = (lb_off >> 2) as u8;
    } else if lb_len <= M2_MAX_LEN && lb_off <= M2_MAX_OFFSET {
        lb_off = lb_off.wrapping_sub(1);
        if outp.offset(2) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = (lb_len.wrapping_sub(1) << 5 | ((lb_off & 0x7) << 2)) as u8;
        outp = outp.offset(1);
        *outp = (lb_off >> 3) as u8;
    } else if lb_len == M2_MIN_LEN
        && lb_off <= M1_MAX_OFFSET.wrapping_add(M2_MAX_OFFSET)
        && last_lit_len >= 4
    {
        lb_off = lb_off.wrapping_sub(1_u32.wrapping_add(M2_MAX_OFFSET));
        if outp.offset(2) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = (M1_MARKER | ((lb_off & 0x3) << 2)) as u8;
        outp = outp.offset(1);
        *outp = (lb_off >> 2) as u8;
    } else if lb_off <= M3_MAX_OFFSET {
        lb_off = lb_off.wrapping_sub(1);
        if lb_len <= M3_MAX_LEN {
            if outp.offset(1) > outp_end as *mut u8 {
                *dst_size = outp.offset_from(dst) as usize;
                return Err(Error::OutputOverrun);
            }
            *outp = (M3_MARKER | lb_len.wrapping_sub(2)) as u8;
        } else {
            lb_len = lb_len.wrapping_sub(M3_MAX_LEN);
            if outp.offset(lb_len.wrapping_div(255).wrapping_add(2) as isize) > outp_end as *mut u8
            {
                *dst_size = outp.offset_from(dst) as usize;
                return Err(Error::OutputOverrun);
            }
            *outp = M3_MARKER as u8;
            outp = outp.offset(1);
            let mut l = lb_len;
            while l > 255 {
                *outp = 0;
                outp = outp.offset(1);
                l = l.wrapping_sub(255);
            }
            *outp = l as u8;
        }
        outp = outp.offset(1);
        if outp.offset(2) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = (lb_off << 2) as u8;
        outp = outp.offset(1);
        *outp = (lb_off >> 6) as u8;
    } else {
        lb_off = lb_off.wrapping_sub(0x4000);
        if lb_len <= M4_MAX_LEN {
            if outp.offset(1) > outp_end as *mut u8 {
                *dst_size = outp.offset_from(dst) as usize;
                return Err(Error::OutputOverrun);
            }
            *outp = (M4_MARKER | ((lb_off & 0x4000) >> 11) | lb_len.wrapping_sub(2)) as u8;
        } else {
            lb_len = lb_len.wrapping_sub(M4_MAX_LEN);
            if outp.offset(lb_len.wrapping_div(255).wrapping_add(2) as isize) > outp_end as *mut u8
            {
                *dst_size = outp.offset_from(dst) as usize;
                return Err(Error::OutputOverrun);
            }
            *outp = (M4_MARKER | ((lb_off & 0x4000) >> 11)) as u8;
            outp = outp.offset(1);
            let mut l_0 = lb_len;
            while l_0 > 255 {
                *outp = 0;
                outp = outp.offset(1);
                l_0 = l_0.wrapping_sub(255);
            }
            *outp = l_0 as u8;
        }
        outp = outp.offset(1);
        if outp.offset(2) > outp_end as *mut u8 {
            *dst_size = outp.offset_from(dst) as usize;
            return Err(Error::OutputOverrun);
        }
        *outp = (lb_off << 2) as u8;
        outp = outp.offset(1);
        *outp = (lb_off >> 6) as u8;
    }
    outp = outp.offset(1);
    *outpp = outp;
    Ok(())
}

unsafe fn lzokay_compress_dict(
    src: *const u8,
    src_size: usize,
    dst: *mut u8,
    init_dst_size: usize,
    dst_size: *mut usize,
    dict_storage: &mut Dict,
) -> Result<(), Error> {
    //let mut err: Result<(), Error> = Ok(());
    let mut s: State = State::new();
    *dst_size = init_dst_size;
    let mut outp: *mut u8 = dst;
    let outp_end: *mut u8 = dst.add(init_dst_size);
    let mut lit_len: u32 = 0;
    let mut lb_off: u32 = 0;
    let mut lb_len: u32 = 0;
    let mut best_off: [u32; 34] = [0; 34];
    dict_storage.init(&mut s, src, src_size);
    let mut lit_ptr: *const u8 = s.inp;
    dict_storage.advance(
        &mut s,
        &mut lb_off,
        &mut lb_len,
        best_off.as_mut_ptr(),
        false,
    );
    while s.buf_sz > 0 {
        if lit_len == 0 {
            lit_ptr = s.bufp;
        }
        // if lb_len < 2
        //     || lb_len == 2 && (lb_off > M1_MAX_OFFSET || lit_len == 0 || lit_len >= 4)
        //     || lb_len == 2 && outp == dst
        //     || outp == dst && lit_len == 0
        // {
        //     lb_len = 0
        // } else if lb_len == M2_MIN_LEN
        //     && lb_off > M1_MAX_OFFSET.wrapping_add(M2_MAX_OFFSET)
        //     && lit_len >= 4
        // {
        if (lb_len < 2
            || lb_len == 2 && (lb_off > M1_MAX_OFFSET || lit_len == 0 || lit_len >= 4)
            || lb_len == 2 && outp == dst
            || outp == dst && lit_len == 0)
            || (lb_len == M2_MIN_LEN
                && lb_off > M1_MAX_OFFSET.wrapping_add(M2_MAX_OFFSET)
                && lit_len >= 4)
        {
            lb_len = 0;
        }
        if lb_len == 0 {
            lit_len = lit_len.wrapping_add(1);
            dict_storage.advance(
                &mut s,
                &mut lb_off,
                &mut lb_len,
                best_off.as_mut_ptr(),
                false,
            );
        } else {
            find_better_match(
                best_off.as_mut_ptr() as *const u32,
                &mut lb_len,
                &mut lb_off,
            );
            encode_literal_run(&mut outp, outp_end, dst, dst_size, lit_ptr, lit_len)?;

            encode_lookback_match(&mut outp, outp_end, dst, dst_size, lb_len, lb_off, lit_len)?;

            lit_len = 0;
            dict_storage.advance(
                &mut s,
                &mut lb_off,
                &mut lb_len,
                best_off.as_mut_ptr(),
                true,
            );
        }
    }
    encode_literal_run(&mut outp, outp_end, dst, dst_size, lit_ptr, lit_len)?;
    /* Terminating M4 */
    if outp.offset(3) > outp_end {
        *dst_size = outp.offset_from(dst) as usize;
        return Err(Error::OutputOverrun);
    }
    *outp = (M4_MARKER | 1) as u8;
    outp = outp.offset(1);
    *outp = 0;
    outp = outp.offset(1);
    *outp = 0;
    outp = outp.offset(1);
    *dst_size = outp.offset_from(dst) as usize;
    Ok(())
}
