# Trial illustration bank

[English](README.md) | [Русский](README.ru.md)

Generated offline with the built-in image_gen tool. No external generation service
is used at runtime. Original prompts are preserved in [prompts.json](prompts.json).

Each source is a 2x2 PNG atlas with four variants:

| Source | Category |
|---|---|
| cats.png | Cats |
| dogs.png | Dogs |
| bicycles.png | Bicycles |
| cars.png | Cars |
| flowers.png | Potted flowering plants |
| trees.png | Trees |

The prepared `tiles/` directory contains 24 PNGs of 160x160 pixels. After changing
atlas sources, rebuild tiles:

```sh
cargo run --locked --example prepare_visual
```

Rebuild/restart the server after changing tiles: it embeds them in the binary and
caches their data URLs. No image resizing occurs on HTTP requests. Public tasks
contain image data, not category labels or source filenames. Each 16-image board
has distinct tiles; users select object types matching the reference.

Wrong answers change the object and board without resetting the deadline or
budget. Existing stored tasks retain their pictures until replacement or expiry.
See [website integration](../../docs/website-integration.md).

This is a style trial, not a production CAPTCHA corpus. Bots can recognize or
memorize 24 static pictures. Category membership is public in this repository.
More pictures improve variety, not a guarantee of human presence.
