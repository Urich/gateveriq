//! Rebuild small runtime tiles after replacing the generated 2x2 illustration atlases.
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/visual");
    let output = root.join("tiles");
    std::fs::create_dir_all(&output)?;
    for category in ["cats", "dogs", "bicycles", "cars", "flowers", "trees"] {
        let source = image::open(root.join(format!("{category}.png")))?.to_rgb8();
        let (width, height) = source.dimensions();
        if width != height || width % 2 != 0 {
            return Err("atlas must be a square 2x2 grid".into());
        }
        for index in 0..4 {
            let tile = image::imageops::crop_imm(
                &source,
                (index % 2) * width / 2,
                (index / 2) * height / 2,
                width / 2,
                height / 2,
            )
            .to_image();
            let tile =
                image::imageops::resize(&tile, 160, 160, image::imageops::FilterType::Lanczos3);
            tile.save(output.join(format!("{category}-{index}.png")))?;
        }
    }
    println!("Prepared 24 illustration tiles in {}", output.display());
    Ok(())
}
