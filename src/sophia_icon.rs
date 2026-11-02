// SPDX-License-Identifier: GPL-3.0-or-later
//! Sophia's face, painted by build.rs and shared by the window and the tray.

pub const SIDE: usize = 256;

pub fn rgba() -> &'static [u8] {
    include_bytes!(concat!(env!("OUT_DIR"), "/sophia_256.rgba"))
}

pub fn bgra(side: usize) -> Vec<u8> {
    let src = rgba();
    let scale = SIDE / side.max(1);
    let mut out = vec![0_u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let mut sum = [0_u32; 4];
            let mut count = 0_u32;
            for dy in 0..scale {
                for dx in 0..scale {
                    let sx = x * scale + dx;
                    let sy = y * scale + dy;
                    let i = (sy * SIDE + sx) * 4;
                    for channel in 0..4 {
                        sum[channel] += u32::from(src[i + channel]);
                    }
                    count += 1;
                }
            }
            let o = (y * side + x) * 4;
            let n = count.max(1);
            out[o] = (sum[2] / n) as u8;
            out[o + 1] = (sum[1] / n) as u8;
            out[o + 2] = (sum[0] / n) as u8;
            out[o + 3] = (sum[3] / n) as u8;
        }
    }
    out
}
