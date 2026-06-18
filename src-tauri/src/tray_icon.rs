#![cfg_attr(not(feature = "desktop"), allow(dead_code))]

use crate::models::QuotaSnapshot;

pub fn render_quota_icon_rgba(
    snapshot: Option<&QuotaSnapshot>,
    size: u32,
) -> Result<Vec<u8>, String> {
    if size < 32 {
        return Err("tray icon size must be at least 32".to_string());
    }

    let mut canvas = vec![0_u8; (size * size * 4) as usize];
    fill(&mut canvas, size, 5, 7, 10, 255);
    draw_border(&mut canvas, size);

    let top = snapshot.and_then(|snapshot| snapshot.five_hour.remaining_percent);
    let bottom = snapshot.and_then(|snapshot| snapshot.weekly.remaining_percent);
    let top_line = format!("5H{}", compact_percent(top));
    let bottom_line = format!("1W{}", compact_percent(bottom));

    draw_text_centered(&mut canvas, size, &top_line, 9, 2, Color(34, 230, 209, 255));
    draw_text_centered(
        &mut canvas,
        size,
        &bottom_line,
        38,
        2,
        Color(155, 124, 255, 255),
    );
    draw_line(&mut canvas, size, 8, 32, 56, Color(29, 42, 45, 255));

    Ok(canvas)
}

fn compact_percent(value: Option<u8>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "--".to_string())
}

#[derive(Debug, Clone, Copy)]
struct Color(u8, u8, u8, u8);

fn fill(canvas: &mut [u8], size: u32, r: u8, g: u8, b: u8, a: u8) {
    for y in 0..size {
        for x in 0..size {
            set_pixel(canvas, size, x, y, Color(r, g, b, a));
        }
    }
}

fn draw_border(canvas: &mut [u8], size: u32) {
    let border = Color(29, 42, 45, 255);
    for x in 0..size {
        set_pixel(canvas, size, x, 0, border);
        set_pixel(canvas, size, x, size - 1, border);
    }
    for y in 0..size {
        set_pixel(canvas, size, 0, y, border);
        set_pixel(canvas, size, size - 1, y, border);
    }
}

fn draw_line(canvas: &mut [u8], size: u32, x1: u32, y: u32, x2: u32, color: Color) {
    for x in x1..=x2.min(size - 1) {
        set_pixel(canvas, size, x, y, color);
    }
}

fn draw_text_centered(canvas: &mut [u8], size: u32, text: &str, y: u32, scale: u32, color: Color) {
    let width = text_width(text, scale);
    let x = size.saturating_sub(width) / 2;
    draw_text(canvas, size, text, x, y, scale, color);
}

fn text_width(text: &str, scale: u32) -> u32 {
    let glyph_count = text.chars().count() as u32;
    if glyph_count == 0 {
        return 0;
    }
    glyph_count * 3 * scale + glyph_count.saturating_sub(1) * scale
}

fn draw_text(canvas: &mut [u8], size: u32, text: &str, x: u32, y: u32, scale: u32, color: Color) {
    let mut cursor = x;
    for character in text.chars() {
        draw_glyph(canvas, size, character, cursor, y, scale, color);
        cursor += 4 * scale;
    }
}

fn draw_glyph(
    canvas: &mut [u8],
    size: u32,
    character: char,
    x: u32,
    y: u32,
    scale: u32,
    color: Color,
) {
    let Some(pattern) = glyph(character) else {
        return;
    };
    for (row, bits) in pattern.iter().enumerate() {
        for column in 0..3 {
            if bits & (1 << (2 - column)) == 0 {
                continue;
            }
            for sy in 0..scale {
                for sx in 0..scale {
                    set_pixel(
                        canvas,
                        size,
                        x + column * scale + sx,
                        y + row as u32 * scale + sy,
                        color,
                    );
                }
            }
        }
    }
}

fn glyph(character: char) -> Option<[u8; 5]> {
    match character {
        '0' => Some([0b111, 0b101, 0b101, 0b101, 0b111]),
        '1' => Some([0b010, 0b110, 0b010, 0b010, 0b111]),
        '2' => Some([0b111, 0b001, 0b111, 0b100, 0b111]),
        '3' => Some([0b111, 0b001, 0b111, 0b001, 0b111]),
        '4' => Some([0b101, 0b101, 0b111, 0b001, 0b001]),
        '5' => Some([0b111, 0b100, 0b111, 0b001, 0b111]),
        '6' => Some([0b111, 0b100, 0b111, 0b101, 0b111]),
        '7' => Some([0b111, 0b001, 0b010, 0b010, 0b010]),
        '8' => Some([0b111, 0b101, 0b111, 0b101, 0b111]),
        '9' => Some([0b111, 0b101, 0b111, 0b001, 0b111]),
        'H' | 'h' => Some([0b101, 0b101, 0b111, 0b101, 0b101]),
        'W' | 'w' => Some([0b101, 0b101, 0b101, 0b111, 0b101]),
        '-' => Some([0b000, 0b000, 0b111, 0b000, 0b000]),
        _ => None,
    }
}

fn set_pixel(canvas: &mut [u8], size: u32, x: u32, y: u32, color: Color) {
    if x >= size || y >= size {
        return;
    }
    let index = ((y * size + x) * 4) as usize;
    canvas[index] = color.0;
    canvas[index + 1] = color.1;
    canvas[index + 2] = color.2;
    canvas[index + 3] = color.3;
}

#[cfg(test)]
mod tests {
    use crate::models::{QuotaReading, QuotaSnapshot, SnapshotSource};
    use crate::tray_icon::render_quota_icon_rgba;

    fn snapshot(five: Option<u8>, week: Option<u8>) -> QuotaSnapshot {
        QuotaSnapshot {
            id: "snap".to_string(),
            source: SnapshotSource::PastedStatus,
            captured_at: "unix:1".to_string(),
            five_hour: QuotaReading {
                remaining_percent: five,
                reset_at: None,
                reset_countdown_seconds: None,
            },
            weekly: QuotaReading {
                remaining_percent: week,
                reset_at: None,
                reset_countdown_seconds: None,
            },
            raw_text: String::new(),
            status_message: String::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn renders_known_quota_icon() {
        let rgba = render_quota_icon_rgba(Some(&snapshot(Some(72), Some(46))), 64).unwrap();

        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert!(rgba.iter().any(|value| *value != 0));
    }

    #[test]
    fn renders_full_and_low_quota_icon() {
        let rgba = render_quota_icon_rgba(Some(&snapshot(Some(100), Some(8))), 64).unwrap();

        assert_eq!(rgba.len(), 64 * 64 * 4);
    }

    #[test]
    fn renders_unknown_quota_icon() {
        let rgba = render_quota_icon_rgba(None, 64).unwrap();

        assert_eq!(rgba.len(), 64 * 64 * 4);
    }
}
