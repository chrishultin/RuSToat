//! Render the app icon's SVG into a macOS `.iconset` folder (PNGs at every
//! size `iconutil` expects). Used by `scripts/make-icon.sh`.
//!
//! Usage: cargo run --example render_icon -- <icon.svg> <out.iconset>
//!
//! Groups with `class="detail"` (circuit traces, rivets) are hidden in the
//! 16 and 32 px images, where they'd only blur the shape.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use resvg::{tiny_skia, usvg};

/// `iconutil`'s file names: (logical size, scale).
const SIZES: [(u32, u32); 10] = [
    (16, 1),
    (16, 2),
    (32, 1),
    (32, 2),
    (128, 1),
    (128, 2),
    (256, 1),
    (256, 2),
    (512, 1),
    (512, 2),
];

/// Images at or below this many pixels use the simplified drawing.
const SMALL_PX: u32 = 32;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [svg_path, out_dir] = args.as_slice() else {
        bail!("usage: render_icon <icon.svg> <out.iconset>");
    };
    let svg = std::fs::read_to_string(svg_path).with_context(|| format!("reading {svg_path}"))?;
    let full = parse(&svg)?;
    let small = parse(&svg.replace(r#"class="detail""#, r#"display="none""#))?;

    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    for (size, scale) in SIZES {
        let px = size * scale;
        let tree = if px <= SMALL_PX { &small } else { &full };
        let name = if scale == 1 {
            format!("icon_{size}x{size}.png")
        } else {
            format!("icon_{size}x{size}@{scale}x.png")
        };
        render(tree, px, &out_dir.join(name))?;
    }
    Ok(())
}

fn parse(svg: &str) -> anyhow::Result<usvg::Tree> {
    usvg::Tree::from_str(svg, &usvg::Options::default()).context("parsing the icon SVG")
}

fn render(tree: &usvg::Tree, px: u32, path: &Path) -> anyhow::Result<()> {
    let mut pixmap = tiny_skia::Pixmap::new(px, px).context("allocating the image")?;
    let scale = px as f32 / tree.size().width();
    resvg::render(
        tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .save_png(path)
        .with_context(|| format!("writing {}", path.display()))
}
