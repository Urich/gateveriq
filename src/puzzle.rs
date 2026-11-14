//! Procedural landscape puzzles. No reusable image/category lookup bank.
//! Image matching and blind guesses remain possible: this is an experimental risk signal.
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageEncoder, Rgba, RgbaImage};
use rand::Rng;
use serde_json::{json, Value};

pub(crate) const WIDTH: u32 = 280;
pub(crate) const HEIGHT: u32 = 140;
pub(crate) const PIECE: u32 = 40;
pub(crate) const MAX_POSITION: u32 = WIDTH - PIECE;
pub(crate) const TOLERANCE: u32 = 3;

fn mix(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|i| (a[i] as f32 * (1.0 - amount) + b[i] as f32 * amount) as u8)
}
fn png(image: &RgbaImage) -> String {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("fixed-size RGBA image is valid PNG input");
    format!("data:image/png;base64,{}", STANDARD.encode(bytes))
}
fn inside(x: f32, y: f32) -> bool {
    let circle = |cx: f32, cy: f32| (x - cx).powi(2) + (y - cy).powi(2) <= 5.5f32.powi(2);
    (((6.0..34.0).contains(&x) && (6.0..34.0).contains(&y))
        || circle(20.0, 6.0)
        || circle(34.0, 20.0))
        && !circle(6.0, 20.0)
        && !circle(20.0, 34.0)
}

pub(crate) fn new_task(expires: i64, remaining: u32) -> (Value, u32) {
    let mut rng = rand::thread_rng();
    // Sky top, horizon, distant mountains, near mountains, water.
    let palettes = [
        [
            [62, 121, 169],
            [218, 231, 218],
            [113, 157, 161],
            [45, 99, 112],
            [78, 148, 158],
        ],
        [
            [67, 74, 129],
            [243, 185, 152],
            [152, 126, 157],
            [73, 85, 124],
            [110, 135, 162],
        ],
        [
            [26, 48, 83],
            [121, 160, 172],
            [72, 109, 135],
            [32, 74, 93],
            [54, 112, 136],
        ],
        [
            [112, 159, 173],
            [249, 225, 172],
            [138, 156, 135],
            [67, 114, 103],
            [111, 168, 158],
        ],
        [
            [119, 112, 170],
            [240, 201, 205],
            [173, 154, 184],
            [96, 115, 153],
            [137, 162, 188],
        ],
    ];
    let colors = palettes[rng.gen_range(0..palettes.len())];
    let phase = rng.gen_range(0.0..std::f32::consts::TAU);
    let frequency = rng.gen_range(0.015..0.035f32);
    let sun_x = rng.gen_range(25.0..255.0f32);
    let sun_y = rng.gen_range(23.0..48.0f32);
    let sun_radius = rng.gen_range(10.0..18.0f32);
    let shore = rng.gen_range(95.0..115.0f32);
    let mut background = RgbaImage::new(WIDTH, HEIGHT);
    for (x, y, pixel) in background.enumerate_pixels_mut() {
        let (xf, yf) = (x as f32, y as f32);
        let mut color = mix(colors[0], colors[1], (yf / shore).min(1.0));
        if (xf - sun_x).powi(2) + (yf - sun_y).powi(2) < sun_radius.powi(2) {
            color = mix([255, 242, 208], color, 0.1);
        }
        let far = 77.0 + (xf * frequency + phase).sin() * 17.0 + (xf * 0.053 + phase).cos() * 8.0;
        let near = 102.0 + (xf * frequency * 1.8 + phase + 2.0).sin() * 15.0;
        if yf > far {
            color = colors[2];
        }
        if yf > near {
            color = colors[3];
        }
        if yf > shore {
            let ripple =
                ((xf * 0.11 + yf * 0.8 + phase).sin() * 0.04 + 0.08) * (1.0 - (yf - shore) / 60.0);
            color = mix(colors[4], colors[1], ripple.max(0.0));
        }
        // Subtle per-pixel grain keeps each rendering unique, including flat regions.
        let grain = rng.gen_range(-3i16..=3);
        *pixel = Rgba([
            color[0].saturating_add_signed(grain as i8),
            color[1].saturating_add_signed(grain as i8),
            color[2].saturating_add_signed(grain as i8),
            255,
        ]);
    }
    // A new foreground on every scene; silhouettes give the puzzle texture.
    for _ in 0..rng.gen_range(5..12) {
        let tx = rng.gen_range(0..WIDTH) as i32;
        let base = rng.gen_range(105..HEIGHT) as i32;
        let height = rng.gen_range(14..36);
        for dy in 0..height {
            let half = dy / 3 + 1;
            for dx in -half..=half {
                let (x, y) = (tx + dx, base - height + dy);
                if x >= 0 && x < WIDTH as i32 && y >= 0 && y < HEIGHT as i32 {
                    background.put_pixel(x as u32, y as u32, Rgba([24, 55, 66, 255]));
                }
            }
        }
    }
    let answer = rng.gen_range(56..=MAX_POSITION - 6);
    let piece_y = rng.gen_range(16..=HEIGHT - PIECE - 12);
    let mut piece = RgbaImage::new(PIECE, PIECE);
    for y in 0..PIECE {
        for x in 0..PIECE {
            let coverage = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                .iter()
                .filter(|(dx, dy)| inside(x as f32 + dx, y as f32 + dy))
                .count() as f32
                / 4.0;
            if coverage == 0.0 {
                continue;
            }
            let original = *background.get_pixel(answer + x, piece_y + y);
            let edge = !inside(x as f32 - 1.0, y as f32)
                || !inside(x as f32 + 1.0, y as f32)
                || !inside(x as f32, y as f32 - 1.0)
                || !inside(x as f32, y as f32 + 1.0);
            let mut fragment = original;
            if edge {
                let bright = mix(
                    [original[0], original[1], original[2]],
                    [255, 255, 255],
                    0.7,
                );
                fragment = Rgba([bright[0], bright[1], bright[2], 255]);
            }
            fragment[3] = (coverage * 255.0) as u8;
            piece.put_pixel(x, y, fragment);
            let hole = mix(
                [original[0], original[1], original[2]],
                [10, 22, 34],
                0.68 * coverage,
            );
            background.put_pixel(
                answer + x,
                piece_y + y,
                Rgba([hole[0], hole[1], hole[2], 255]),
            );
        }
    }
    (
        json!({"kind":"slider_puzzle", "task_id":crate::core::random(), "expires_at":expires,
        "instruction_key":"slide_piece", "background":png(&background), "piece":png(&piece),
        "width":WIDTH, "height":HEIGHT, "piece_size":PIECE, "piece_y":piece_y,
        "max_position":MAX_POSITION, "max_attempts":remaining}),
        answer,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scenes_are_unique_and_do_not_publish_coordinates() {
        let mut images = std::collections::HashSet::new();
        for _ in 0..8 {
            let (task, answer) = new_task(1234, 3);
            assert!((56..=MAX_POSITION - 6).contains(&answer));
            assert!(task.get("answer").is_none());
            assert!(task.get("position").is_none());
            assert!(images.insert(task["background"].as_str().unwrap().to_owned()));
            for (field, w, h) in [("background", WIDTH, HEIGHT), ("piece", PIECE, PIECE)] {
                let data = STANDARD
                    .decode(task[field].as_str().unwrap().split(',').nth(1).unwrap())
                    .unwrap();
                let image = image::load_from_memory(&data).unwrap();
                assert_eq!((image.width(), image.height()), (w, h));
            }
            assert_eq!(task["max_attempts"], 3);
        }
    }
}
