//! Render the app icon's SVG into a macOS `.iconset` folder (PNGs at every
//! size `iconutil` expects). Used by `scripts/make-icon.sh`.
//!
//! Usage: cargo run --example render_icon -- <icon.svg> <out.iconset>
//!        cargo run --example render_icon -- --ios <icon.svg> <AppIcon.png>
//!
//! `--ios` renders one 1024 px PNG for the iPhone app instead: the tile
//! alone, square and edge to edge, since iOS rounds the corners itself.
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
    if let [flag, svg_path, out_png] = args.as_slice()
        && flag == "--ios"
    {
        let svg =
            std::fs::read_to_string(svg_path).with_context(|| format!("reading {svg_path}"))?;
        return render(&parse(&ios_tile(&svg)?)?, 1024, Path::new(out_png));
    }
    let [svg_path, out_dir] = args.as_slice() else {
        bail!("usage: render_icon [--ios] <icon.svg> <out.iconset | AppIcon.png>");
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

/// The SVG with its 824 px tile at (100, 100) filling the canvas, corners
/// square.
fn ios_tile(svg: &str) -> anyhow::Result<String> {
    const TILE: &str = r#"x="100" y="100" width="824" height="824" rx="185""#;
    const VIEW_BOX: &str = r#"viewBox="0 0 1024 1024""#;
    if !svg.contains(TILE) || !svg.contains(VIEW_BOX) {
        bail!("the icon's tile or viewBox has changed; update ios_tile");
    }
    Ok(svg
        .replace(TILE, r#"x="100" y="100" width="824" height="824" rx="0""#)
        .replace(VIEW_BOX, r#"viewBox="100 100 824 824""#))
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
