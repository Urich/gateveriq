//! Procedural landscape puzzles. No reusable image/category lookup bank.
//! Image matching and blind guesses remain possible: this is an experimental risk signal.
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageEncoder, Rgba, RgbaImage};
use rand::{seq::IteratorRandom, Rng};
use serde_json::{json, Value};

mod landscape;

pub(crate) const WIDTH: u32 = 280;
pub(crate) const HEIGHT: u32 = 140;
pub(crate) const PIECE: u32 = 40;
pub(crate) const MAX_POSITION: u32 = WIDTH - PIECE;
pub(crate) const TOLERANCE: u32 = 3;
const MIN_CUTOUT_X: u32 = 56;
const MAX_CUTOUT_X: u32 = MAX_POSITION - 6;
const MIN_CUTOUT_Y: u32 = 16;
const MAX_CUTOUT_Y: u32 = HEIGHT - PIECE - 12;
const CUTOUT_X_SPACING: u32 = PIECE + 4;
const CUTOUT_Y_SPACING: u32 = 16;
// Tabs on the top, right, bottom and left edges respectively.
const PROFILES: [[bool; 4]; 6] = [
    [true, true, false, false],
    [false, true, true, false],
    [false, false, true, true],
    [true, false, false, true],
    [true, false, true, false],
    [false, true, false, true],
];

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
fn inside(x: f32, y: f32, profile: [bool; 4]) -> bool {
    let centers = [(20.0, 6.0), (34.0, 20.0), (20.0, 34.0), (6.0, 20.0)];
    for ((cx, cy), tab) in centers.into_iter().zip(profile) {
        if (x - cx).powi(2) + (y - cy).powi(2) <= 5.5f32.powi(2) {
            return tab;
        }
    }
    (6.0..34.0).contains(&x) && (6.0..34.0).contains(&y)
}

fn cutout_positions(rng: &mut impl Rng, answer: u32, piece_y: u32) -> [(u32, u32); 3] {
    let mut positions = [(answer, piece_y); 3];
    for i in 1..positions.len() {
        let previous = &positions[..i];
        let x = (MIN_CUTOUT_X..=MAX_CUTOUT_X)
            .filter(|x| {
                previous
                    .iter()
                    .all(|(px, _)| x.abs_diff(*px) >= CUTOUT_X_SPACING)
            })
            .choose(rng)
            .expect("the horizontal range fits three spaced cutouts");
        let y = (MIN_CUTOUT_Y..=MAX_CUTOUT_Y)
            .filter(|y| {
                previous
                    .iter()
                    .all(|(_, py)| y.abs_diff(*py) >= CUTOUT_Y_SPACING)
            })
            .choose(rng)
            .expect("the vertical range fits three spaced cutouts");
        positions[i] = (x, y);
    }
    positions
}

pub(crate) fn new_task(expires: i64, remaining: u32) -> (Value, u32) {
    let mut rng = rand::thread_rng();
    let mut background = landscape::generate(&mut rng);
    let profile = PROFILES[rng.gen_range(0..PROFILES.len())];
    let contains = |x, y| inside(x, y, profile);
    let answer = rng.gen_range(MIN_CUTOUT_X..=MAX_CUTOUT_X);
    let piece_y = rng.gen_range(MIN_CUTOUT_Y..=MAX_CUTOUT_Y);
    let positions = cutout_positions(&mut rng, answer, piece_y);
    let mut piece = RgbaImage::new(PIECE, PIECE);
    for y in 0..PIECE {
        for x in 0..PIECE {
            let coverage = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
                .iter()
                .filter(|(dx, dy)| contains(x as f32 + dx, y as f32 + dy))
                .count() as f32
                / 4.0;
            if coverage == 0.0 {
                continue;
            }
            let original = *background.get_pixel(answer + x, piece_y + y);
            let (xf, yf) = (x as f32, y as f32);
            let lower_edge = !contains(xf, yf + 1.0);
            let edge = lower_edge
                || !contains(xf - 1.0, yf)
                || !contains(xf + 1.0, yf)
                || !contains(xf, yf - 1.0);
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
            let tint = if lower_edge {
                [99, 117, 129]
            } else {
                [10, 22, 34]
            };
            for (cutout_x, cutout_y) in positions {
                let pixel = background.get_pixel_mut(cutout_x + x, cutout_y + y);
                let hole = mix([pixel[0], pixel[1], pixel[2]], tint, 0.68 * coverage);
                *pixel = Rgba([hole[0], hole[1], hole[2], 255]);
            }
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
    fn cutouts_are_in_bounds_at_distinct_heights_and_do_not_overlap() {
        use rand::{rngs::StdRng, SeedableRng};

        let mut rng = StdRng::seed_from_u64(0);
        for answer in MIN_CUTOUT_X..=MAX_CUTOUT_X {
            for piece_y in [
                MIN_CUTOUT_Y,
                (MIN_CUTOUT_Y + MAX_CUTOUT_Y) / 2,
                MAX_CUTOUT_Y,
            ] {
                let positions = cutout_positions(&mut rng, answer, piece_y);
                assert_eq!(positions[0], (answer, piece_y));
                for (i, (x, y)) in positions.into_iter().enumerate() {
                    assert!(x + PIECE <= WIDTH && y + PIECE <= HEIGHT);
                    for &(previous_x, previous_y) in &positions[..i] {
                        assert!(x.abs_diff(previous_x) >= PIECE);
                        assert!(y.abs_diff(previous_y) >= CUTOUT_Y_SPACING);
                    }
                }
            }
        }
    }

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
