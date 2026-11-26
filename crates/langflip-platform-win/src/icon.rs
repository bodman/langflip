//! Procedural crisp 32x32 RGBA tray icon generator for LangFlip (`T008`).
//!
//! Draws a rounded indigo/blue badge with bright white opposing swap arrows
//! that remains legible on both Windows 11 dark and light taskbars.

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// Builds a 32x32 32-bpp BGRA `HICON` with alpha transparency.
/// Caller owns the returned `HICON` and should destroy it with `DestroyIcon`.
pub fn create_tray_icon() -> Result<HICON, String> {
    const SIZE: usize = 32;
    let mut pixels = [0u32; SIZE * SIZE];

    // Draw rounded square background (#2563EB -> #4F46E5 subtle vertical gradient).
    for y in 0..SIZE {
        for x in 0..SIZE {
            if in_rounded_rect(x as f32 + 0.5, y as f32 + 0.5, 1.0, 1.0, 30.0, 30.0, 7.0) {
                let t = y as f32 / (SIZE - 1) as f32;
                let r = lerp(37.0, 79.0, t) as u8;
                let g = lerp(99.0, 70.0, t) as u8;
                let b = lerp(235.0, 229.0, t) as u8;
                pixels[y * SIZE + x] = bgra(b, g, r, 255);
            }
        }
    }

    let white = bgra(255, 255, 255, 255);
    let accent = bgra(255, 224, 147, 255); // soft cyan-white for bottom arrow

    // Top arrow pointing right (-->) around y = 10..12
    fill_rect(&mut pixels, SIZE, 7, 10, 22, 13, white);
    // Right arrowhead
    for step in 0..5 {
        let x = 20 + step;
        let top = 7 + step;
        let bot = 16 - step;
        if top <= bot {
            fill_rect(&mut pixels, SIZE, x, top, x + 1, bot, white);
        }
    }

    // Bottom arrow pointing left (<--) around y = 20..22
    fill_rect(&mut pixels, SIZE, 10, 20, 25, 23, accent);
    // Left arrowhead
    for step in 0..5 {
        let x = 11 - step;
        let top = 17 + step;
        let bot = 26 - step;
        if top <= bot {
            fill_rect(&mut pixels, SIZE, x, top, x + 1, bot, accent);
        }
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE as i32,
            biHeight: -(SIZE as i32), // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let color_bmp = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(|e| format!("CreateDIBSection failed: {e}"))?;

    if bits.is_null() {
        let _ = unsafe { DeleteObject(color_bmp.into()) };
        return Err("CreateDIBSection returned null pixel buffer".into());
    }

    unsafe {
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
    }

    let mask_bmp = unsafe { CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, None) };
    if mask_bmp.is_invalid() {
        let _ = unsafe { DeleteObject(color_bmp.into()) };
        return Err("CreateBitmap mask failed".into());
    }

    let icon_info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bmp,
        hbmColor: color_bmp,
    };

    let icon_res = unsafe { CreateIconIndirect(&icon_info) };
    let _ = unsafe { DeleteObject(color_bmp.into()) };
    let _ = unsafe { DeleteObject(mask_bmp.into()) };

    icon_res.map_err(|e| format!("CreateIconIndirect failed: {e}"))
}

const fn bgra(b: u8, g: u8, r: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn fill_rect(pixels: &mut [u32], stride: usize, x0: usize, y0: usize, x1: usize, y1: usize, color: u32) {
    for y in y0..y1.min(stride) {
        for x in x0..x1.min(stride) {
            pixels[y * stride + x] = color;
        }
    }
}

fn in_rounded_rect(x: f32, y: f32, rx: f32, ry: f32, rw: f32, rh: f32, radius: f32) -> bool {
    if x < rx || x > rx + rw || y < ry || y > ry + rh {
        return false;
    }
    let cx = if x < rx + radius {
        rx + radius
    } else if x > rx + rw - radius {
        rx + rw - radius
    } else {
        return true;
    };
    let cy = if y < ry + radius {
        ry + radius
    } else if y > ry + rh - radius {
        ry + rh - radius
    } else {
        return true;
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= radius * radius
}
