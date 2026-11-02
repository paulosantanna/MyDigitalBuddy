// SPDX-License-Identifier: GPL-3.0-or-later
//! Draws Sophia and, on Windows, embeds that picture as the executable icon.

use image::ImageEncoder;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FRAME: [u8; 4] = [0x7c, 0x3a, 0xed, 255];
const SHELL: [u8; 4] = [0x16, 0x10, 0x22, 255];
const SCREEN: [u8; 4] = [0x0c, 0x08, 0x16, 255];
const EYE: [u8; 4] = [0xc0, 0x84, 0xfc, 255];
const SHINE: [u8; 4] = [0xf3, 0xe8, 0xff, 255];

fn main() {
    if let Err(error) = paint_and_embed() {
        panic!("Sophia icon: {error}");
    }
}

fn paint_and_embed() -> Result<(), String> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let rgba = paint(256);
    fs::write(out_dir.join("sophia_256.rgba"), &rgba).map_err(|e| e.to_string())?;
    fs::write(out_dir.join("sophia.png"), png(256)).map_err(|e| e.to_string())?;
    let ico_path = out_dir.join("sophia.ico");
    fs::write(&ico_path, ico(&[16, 32, 48, 256])).map_err(|e| e.to_string())?;
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        embed(&out_dir, &ico_path, target.contains("windows-gnu"))?;
    }
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}

fn embed(out_dir: &Path, ico: &Path, gnu: bool) -> Result<(), String> {
    let rc_path = out_dir.join("sophia.rc");
    let icon = ico
        .to_str()
        .ok_or("icon path is not valid Unicode")?
        .replace('\\', "/");
    fs::write(&rc_path, format!("1 ICON \"{icon}\"\n")).map_err(|e| e.to_string())?;
    let compiled = if gnu {
        let object = out_dir.join("sophia.o");
        let windres = find_tool(&["windres", "x86_64-w64-mingw32-windres"])
            .ok_or("windres was not found; install MinGW-w64 binutils")?;
        run(
            &windres,
            &[
                "--input",
                rc_path.to_str().ok_or("rc path")?,
                "--output",
                object.to_str().ok_or("object path")?,
                "--output-format",
                "coff",
            ],
        )?;
        object
    } else {
        let res = out_dir.join("sophia.res");
        let rc = find_tool(&["rc", "llvm-rc"]).ok_or("rc.exe was not found")?;
        run(
            &rc,
            &[
                "/nologo",
                "/fo",
                res.to_str().ok_or("res path")?,
                rc_path.to_str().ok_or("rc path")?,
            ],
        )?;
        res
    };
    let link = compiled
        .to_str()
        .ok_or("compiled resource path is not valid Unicode")?;
    println!("cargo:rustc-link-arg-bins={link}");
    Ok(())
}

fn find_tool(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        for name in names {
            let exe = dir.join(name);
            if exe.is_file() {
                return Some(exe);
            }
            let with_exe = dir.join(format!("{name}.exe"));
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

fn run(program: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} failed: {}",
            program.display(),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn ico(sizes: &[u32]) -> Vec<u8> {
    let images: Vec<Vec<u8>> = sizes.iter().copied().map(png).collect();
    let count = u16::try_from(images.len()).unwrap_or(0);
    let mut file = Vec::new();
    file.extend_from_slice(&0_u16.to_le_bytes());
    file.extend_from_slice(&1_u16.to_le_bytes());
    file.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, image) in sizes.iter().zip(&images) {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        file.push(dim);
        file.push(dim);
        file.push(0);
        file.push(0);
        file.extend_from_slice(&1_u16.to_le_bytes());
        file.extend_from_slice(&32_u16.to_le_bytes());
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        file.extend_from_slice(&image);
    }
    file
}

fn png(size: u32) -> Vec<u8> {
    let rgba = paint(size);
    let mut encoded = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut encoded);
    encoder
        .write_image(&rgba, size, size, image::ExtendedColorType::Rgba8)
        .expect("png encoder accepts an in-memory buffer");
    encoded
}

fn paint(size: u32) -> Vec<u8> {
    let mut pixels = vec![0_u8; size as usize * size as usize * 4];
    let pixel = 160.0 / size as f32;
    for y in 0..size {
        for x in 0..size {
            let lx = (x as f32 + 0.5 - size as f32 * 0.5) * pixel;
            let ly = (y as f32 + 0.5 - size as f32 * 0.5) * pixel;
            let color = sophia(lx, ly, pixel);
            let i = (y as usize * size as usize + x as usize) * 4;
            pixels[i..i + 4].copy_from_slice(&color);
        }
    }
    pixels
}

fn sophia(x: f32, y: f32, pixel: f32) -> [u8; 4] {
    let mut out = [0_u8; 4];
    layer(
        &mut out,
        FRAME,
        cover(sdf(x, y, 0.0, 0.0, 62.0, 47.0, 16.0), pixel),
    );
    layer(
        &mut out,
        SHELL,
        cover(sdf(x, y, 0.0, 0.0, 58.0, 43.0, 13.0), pixel),
    );
    layer(
        &mut out,
        SCREEN,
        cover(sdf(x, y, 0.0, -1.5, 51.0, 34.5, 11.0), pixel),
    );
    for side in [-1.0_f32, 1.0] {
        let cx = side * 24.0;
        let cy = 1.0;
        let eye = cover(sdf(x, y, cx, cy, 16.0, 17.0, 16.0), pixel).clamp(0.0, 1.0);
        layer(&mut out, EYE, eye);
        let t = ((x - (cx - 16.0)) / 32.0).clamp(0.0, 1.0);
        let curve = cy + 17.0 - 0.36 * 34.0 * (std::f32::consts::PI * t).sin();
        if y > curve {
            layer(&mut out, SCREEN, eye);
        }
        let shine = cover(circle(x, y, cx - 5.0, cy - 6.0, 3.2), pixel).clamp(0.0, 1.0);
        layer(&mut out, SHINE, shine * eye);
    }
    layer(&mut out, EYE, cover(circle(x, y, 0.0, 39.0, 2.2), pixel));
    out
}

fn sdf(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, radius: f32) -> f32 {
    let qx = (x - cx).abs() - hw + radius;
    let qy = (y - cy).abs() - hh + radius;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, radius: f32) -> f32 {
    (x - cx).hypot(y - cy) - radius
}

fn cover(distance: f32, pixel: f32) -> f32 {
    (pixel * 0.5 - distance) / pixel
}

fn layer(dst: &mut [u8; 4], src: [u8; 4], coverage: f32) {
    let a = (src[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let under = dst[channel] as f32;
        dst[channel] = (src[channel] as f32 * a + under * (1.0 - a)).round() as u8;
    }
    let under = dst[3] as f32 / 255.0;
    dst[3] = ((a + under * (1.0 - a)) * 255.0).round() as u8;
}
