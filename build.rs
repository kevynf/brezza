use std::path::Path;

const APP: &str = "assets/brezza-app.svg";
const BITMAPS: [(&str, &str, u32); 2] = [
    ("assets/brezza-mark-white.svg", "mark-white.rgba", 64),
    ("assets/brezza-mark-black.svg", "mark-black.rgba", 64),
];
const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

fn render(source: &str, size: u32) -> resvg::tiny_skia::Pixmap {
    println!("cargo:rerun-if-changed={source}");
    let svg = std::fs::read(source).expect("read icon");
    let tree =
        resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).expect("parse icon");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("pixmap");
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

fn main() {
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);

    for (source, name, size) in BITMAPS {
        let rgba: Vec<u8> = render(source, size)
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect();
        std::fs::write(out.join(name), rgba).expect("write bitmap");
    }

    let pngs: Vec<(u32, Vec<u8>)> = SIZES
        .iter()
        .map(|&size| (size, render(APP, size).encode_png().expect("encode icon")))
        .collect();
    let mut ico = vec![0, 0, 1, 0];
    ico.extend((pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, png) in &pngs {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend([side, side, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &pngs {
        ico.extend(png);
    }
    let ico_path = out.join("brezza.ico");
    std::fs::write(&ico_path, ico).expect("write app icon");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(&ico_path.to_string_lossy())
            .compile()
            .expect("embed app icon");
    }
}
