//! CPU decoding of DXT/BGRA/G8 mips to RGBA8, for the few cases that need pixels on the CPU
//! (merging a separate opacity texture into a diffuse texture's alpha).

use crate::TextureData;
use upk::texture::Format;

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

fn color_block(b: &[u8], out: &mut [[u8; 4]; 16], dxt1: bool) {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32| ((a as u32 * wa + b as u32 * wb) / (wa + wb)) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if !dxt1 || c0 > c1 {
        pal[2] = [mix(p0[0], p1[0], 2, 1), mix(p0[1], p1[1], 2, 1), mix(p0[2], p1[2], 2, 1), 255];
        pal[3] = [mix(p0[0], p1[0], 1, 2), mix(p0[1], p1[1], 1, 2), mix(p0[2], p1[2], 1, 2), 255];
    } else {
        pal[2] = [mix(p0[0], p1[0], 1, 1), mix(p0[1], p1[1], 1, 1), mix(p0[2], p1[2], 1, 1), 255];
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    for (i, px) in out.iter_mut().enumerate() {
        *px = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

fn alpha_block_dxt5(b: &[u8], out: &mut [[u8; 4]; 16]) {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = (((7 - i as u32) * a0 + i as u32 * a1) / 7) as u8;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = (((5 - i as u32) * a0 + i as u32 * a1) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let mut bits = 0u64;
    for (i, &v) in b[2..8].iter().enumerate() {
        bits |= (v as u64) << (8 * i);
    }
    for (i, px) in out.iter_mut().enumerate() {
        px[3] = pal[((bits >> (3 * i)) & 7) as usize];
    }
}

/// Decode mip 0 of `t` to tightly packed RGBA8.
pub fn decode_rgba(t: &TextureData) -> Vec<u8> {
    let (w, h) = (t.width as usize, t.height as usize);
    let src = &t.mips[0];
    let mut out = vec![0u8; w * h * 4];
    match t.format {
        Format::Dxt1 | Format::Dxt3 | Format::Dxt5 => {
            let bsize = if t.format == Format::Dxt1 { 8 } else { 16 };
            let bw = w.div_ceil(4);
            for (bi, b) in src.chunks_exact(bsize).enumerate() {
                let (bx, by) = (bi % bw, bi / bw);
                let mut px = [[0u8; 4]; 16];
                match t.format {
                    Format::Dxt1 => color_block(b, &mut px, true),
                    Format::Dxt3 => {
                        color_block(&b[8..], &mut px, false);
                        for i in 0..16 {
                            px[i][3] = ((b[i / 2] >> (4 * (i % 2))) & 15) * 17;
                        }
                    }
                    _ => {
                        color_block(&b[8..], &mut px, false);
                        alpha_block_dxt5(b, &mut px);
                    }
                }
                for (i, p) in px.iter().enumerate() {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    if x < w && y < h {
                        out[(y * w + x) * 4..][..4].copy_from_slice(p);
                    }
                }
            }
        }
        Format::Bgra8 => {
            for (o, s) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                o.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        Format::G8 => {
            for (o, &g) in out.chunks_exact_mut(4).zip(src.iter()) {
                o.copy_from_slice(&[g, g, g, 255]);
            }
        }
        Format::Other => {}
    }
    out
}

/// Diffuse RGB with alpha taken from one channel of `opacity` (resampled), plus a box-filtered
/// mip chain. Returns (width, height, mips).
pub fn merge_opacity(diffuse: &TextureData, opacity: &TextureData, channel: usize) -> (u32, u32, Vec<Vec<u8>>) {
    let (w, h) = (diffuse.width as usize, diffuse.height as usize);
    let mut rgba = decode_rgba(diffuse);
    let a = decode_rgba(opacity);
    let (ow, oh) = (opacity.width as usize, opacity.height as usize);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * ow / w, y * oh / h);
            rgba[(y * w + x) * 4 + 3] = a[(sy * ow + sx) * 4 + channel.min(3)];
        }
    }
    let mut mips = vec![rgba];
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let prev = mips.last().unwrap();
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        sum += prev[(sy * cw + sx) * 4 + c] as u32;
                    }
                    next[(y * nw + x) * 4 + c] = (sum / 4) as u8;
                }
            }
        }
        mips.push(next);
        cw = nw;
        ch = nh;
    }
    (w as u32, h as u32, mips)
}
