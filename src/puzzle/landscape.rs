use super::{mix, HEIGHT, WIDTH};
use image::{Rgba, RgbaImage};
use rand::Rng;

type Color = [u8; 3];

#[derive(Clone, Copy)]
enum Theme {
    AlpineLake,
    Desert,
    Coast,
    NorthernLights,
}

impl Theme {
    const ALL: [Self; 4] = [
        Self::AlpineLake,
        Self::Desert,
        Self::Coast,
        Self::NorthernLights,
    ];

    fn colors(self) -> [Color; 6] {
        // Zenith, horizon, distant ridge, near ridge, foreground, sunlight.
        match self {
            Self::AlpineLake => [
                [52, 107, 150],
                [239, 209, 170],
                [105, 128, 153],
                [45, 83, 102],
                [18, 80, 95],
                [255, 231, 161],
            ],
            Self::Desert => [
                [115, 68, 116],
                [250, 166, 120],
                [179, 80, 98],
                [141, 60, 77],
                [224, 119, 71],
                [255, 226, 156],
            ],
            Self::Coast => [
                [34, 125, 162],
                [166, 223, 207],
                [73, 135, 143],
                [29, 79, 79],
                [21, 131, 141],
                [255, 241, 186],
            ],
            Self::NorthernLights => [
                [23, 26, 62],
                [67, 95, 119],
                [69, 83, 123],
                [29, 52, 79],
                [22, 45, 77],
                [197, 234, 223],
            ],
        }
    }

    fn ridges(self, x: f32, phase: f32) -> (f32, f32) {
        let a = x * 0.023 + phase;
        let b = x * 0.041 - phase;
        let peaks = (a.sin().abs() * 0.70
            + (b * 1.8).sin().abs() * 0.22
            + (x * 0.29 + phase).sin().abs() * 0.08)
            .powf(1.5);
        match self {
            Self::AlpineLake => (86.0 - 52.0 * peaks, 96.0 + 12.0 * (a - 1.1).sin()),
            Self::Desert => (
                76.0 + 15.0 * a.sin() + 5.0 * b.sin(),
                95.0 + 18.0 * (a - 1.4).sin() + 5.0 * (b + 1.3).sin(),
            ),
            Self::Coast => (
                86.0 + 8.0 * a.sin() + 4.0 * b.sin(),
                78.0 + 29.0 * (a - 1.8).sin() + 6.0 * (b * 1.8).sin(),
            ),
            Self::NorthernLights => (96.0 - 35.0 * peaks, 103.0 + 7.0 * (a - 1.0).sin()),
        }
    }
}

fn opacity(distance: f32) -> f32 {
    (distance + 0.5).clamp(0.0, 1.0)
}

fn render(rng: &mut impl Rng, theme: Theme) -> RgbaImage {
    let [zenith, horizon, distant, near, foreground, light] = theme.colors();
    let phase = rng.gen_range(0.0..std::f32::consts::TAU);
    let sun_x = rng.gen_range(42.0..238.0);
    let sun_y = rng.gen_range(22.0..44.0);
    let sun_radius = rng.gen_range(8.0..17.0);
    let baseline = rng.gen_range(105.0..119.0);
    let mut image = RgbaImage::new(WIDTH, HEIGHT);

    for x in 0..WIDTH {
        let xf = x as f32;
        let (far_y, near_y) = theme.ridges(xf, phase);
        let (next_far, next_near) = theme.ridges(xf + 1.0, phase);
        let far_light = ((next_far - far_y) * 0.36 + 0.5).clamp(0.0, 1.0);
        let near_light = ((next_near - near_y) * 0.32 + 0.5).clamp(0.0, 1.0);
        let front_y = match theme {
            Theme::Coast => 101.0 + 15.0 * (xf * 0.022 + phase - 1.9).sin(),
            Theme::Desert => 111.0 + 17.0 * (xf * 0.017 + phase + 1.4).sin(),
            _ => baseline,
        };
        let cloud = (xf * 0.025 + phase).sin() * 3.5 + (xf * 0.064 - phase).sin() * 1.4;
        let aurora_y = 46.0 + (xf * 0.023 + phase).sin() * 14.0 + cloud;

        for y in 0..HEIGHT {
            let yf = y as f32;
            let distance = (xf - sun_x).hypot(yf - sun_y);
            let mut color = mix(zenith, horizon, (yf / 96.0).min(1.0).powf(0.85));
            color = mix(
                color,
                light,
                (1.0 - distance / 58.0).max(0.0).powi(2) * 0.55,
            );
            color = mix(color, light, opacity(sun_radius - distance) * 0.94);

            if matches!(theme, Theme::NorthernLights) {
                let ribbon = (xf * 0.096 - phase).sin() * 0.12 + 0.40;
                let green = (1.0 - (yf - aurora_y).abs() / 15.0).max(0.0).powi(2);
                let violet = (1.0 - (yf - aurora_y + 8.0).abs() / 18.0).max(0.0).powi(2);
                color = mix(color, [164, 96, 188], violet * ribbon);
                color = mix(color, [101, 233, 172], green * ribbon);
                if yf < 65.0 && rng.gen_ratio(1, 215) {
                    color = mix(color, light, rng.gen_range(0.25..0.80));
                }
            } else {
                let cloud_y = 33.0 + cloud;
                let streak = (1.0 - (yf - cloud_y).abs() / 3.0).max(0.0)
                    + (1.0 - (yf - cloud_y - 16.0).abs() / 2.0).max(0.0) * 0.38;
                let gaps = ((xf * 0.031 + phase).cos() * 0.5 + 0.5).powi(2);
                color = mix(color, horizon, streak * gaps * 0.42);
            }

            if yf + 0.5 > far_y {
                let depth = (yf - far_y).max(0.0);
                let shade = (1.0 - depth / 38.0).max(0.0) * (far_light - 0.5);
                let mut rock = mix(distant, horizon, 0.22 + shade * 0.38);
                if matches!(theme, Theme::AlpineLake) {
                    let snowline = 64.0 + 4.0 * (xf * 0.21 + phase).sin();
                    let snow = opacity((snowline - yf) * 0.5) * (0.48 + far_light * 0.42);
                    rock = mix(rock, [230, 233, 229], snow);
                }
                rock = mix(rock, horizon, (depth / 80.0).min(0.38));
                color = mix(color, rock, opacity(yf - far_y));
            }

            if yf + 0.5 > near_y {
                let bands = ((yf - near_y) * 0.35 + cloud * 0.4).sin() * 0.045;
                let ridge = mix(
                    near,
                    distant,
                    (0.10 + near_light * 0.36 + bands).clamp(0.0, 1.0),
                );
                color = mix(color, ridge, opacity(yf - near_y));
            }

            if yf + 0.5 > front_y {
                let depth = (yf - front_y).max(0.0);
                let bands = (depth * 0.89 + cloud * 0.3).sin() * 0.5 + 0.5;
                let glow = (1.0 - (xf - sun_x).abs() / (25.0 + depth * 0.9)).max(0.0);
                let surface = match theme {
                    Theme::Desert => {
                        let wind = (depth * 0.48 + xf * 0.064).sin() * 0.5 + 0.5;
                        mix(
                            foreground,
                            light,
                            (0.36 - depth * 0.006 + wind * 0.10).max(0.0),
                        )
                    }
                    Theme::Coast => {
                        let foam = (1.0 - (depth - 3.0).abs() / 2.5).max(0.0);
                        mix(foreground, [202, 236, 216], foam * 0.66 + bands * 0.13)
                    }
                    _ => mix(foreground, horizon, 0.10 + bands * (0.14 + glow * 0.32)),
                };
                color = mix(color, surface, opacity(yf - front_y));
            }

            // Fine grain gives the illustration a printed texture.
            let grain = rng.gen_range(-3..=3);
            let [r, g, b] = color.map(|channel| channel.saturating_add_signed(grain));
            image.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    if matches!(theme, Theme::AlpineLake | Theme::NorthernLights) {
        let forest = mix(near, [11, 27, 38], 0.63);
        let island_x = rng.gen_range(18..=WIDTH as i32 - 18);
        for _ in 0..22 {
            let x = island_x + rng.gen_range(-43..=43);
            let height = rng.gen_range(6..19);
            let base = baseline as i32 + rng.gen_range(-2..=1);
            for dy in 0..height {
                // Narrow gaps between boughs break up the silhouettes.
                let width = (dy / 3 - i32::from(dy % 4 == 0)).max(0);
                for dx in -width..=width {
                    if (0..WIDTH as i32).contains(&(x + dx)) {
                        let [r, g, b] = forest;
                        image.put_pixel(
                            (x + dx) as u32,
                            (base - height + dy) as u32,
                            Rgba([r, g, b, 255]),
                        );
                    }
                }
            }
        }
    }
    image
}

pub(super) fn generate(rng: &mut impl Rng) -> RgbaImage {
    let theme = Theme::ALL[rng.gen_range(0..Theme::ALL.len())];
    render(rng, theme)
}
