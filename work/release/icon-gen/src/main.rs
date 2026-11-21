//! Renders the Math++ icon SVG to a multi-size .ico (16 to 256 px).
use std::{fs::File, path::Path};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../crates/mpp-ide/assets");
    let svg = std::fs::read(root.join("mathpp.svg")).expect("read svg");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).expect("parse svg");
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16u32, 24, 32, 48, 64, 128, 256] {
        let mut pix = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
        let s = size as f32 / tree.size().width();
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pix.as_mut());
        // tiny-skia stores premultiplied RGBA; ICO wants straight alpha.
        let mut rgba = Vec::with_capacity(pix.data().len());
        for p in pix.pixels() {
            let c = p.demultiply();
            rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        let img = ico::IconImage::from_rgba_data(size, size, rgba);
        dir.add_entry(ico::IconDirEntry::encode(&img).expect("encode"));
        if size == 256 {
            pix.save_png(root.join("mathpp-256.png")).expect("png");
        }
    }
    dir.write(File::create(root.join("mathpp.ico")).unwrap()).unwrap();
}
