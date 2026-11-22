use egui::epaint::text::{FontInsert, FontPriority, FontTweak, InsertFontFamily, VariationCoords};
use egui::{FontData, FontFamily};
use fontique::{Collection, CollectionOptions, FontStyle, FontWeight, FontWidth, SourceCache};

use super::theme;

pub fn install(ctx: &egui::Context) {
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: true,
    });
    let mut cache = SourceCache::default();
    for (family, stack) in [
        (FontFamily::Proportional, theme::font::SANS),
        (FontFamily::Monospace, theme::font::MONO),
    ] {
        for name in stack.iter().rev() {
            if let Some(data) = load(&mut collection, &mut cache, name) {
                let id = format!("{name} ({family})");
                add(ctx, &id, data, family.clone(), FontPriority::Highest);
            }
        }
    }
    let lucide =
        FontData::from_static(include_bytes!("../../assets/lucide.ttf")).tweak(FontTweak {
            scale: 1.0,
            y_offset_factor: 0.08,
            ..Default::default()
        });
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let id = format!("lucide ({family})");
        add(ctx, &id, lucide.clone(), family, FontPriority::Lowest);
    }
}

fn load(collection: &mut Collection, cache: &mut SourceCache, name: &str) -> Option<FontData> {
    let family = collection.family_by_name(name)?;
    let font = family.match_font(
        FontWidth::NORMAL,
        FontStyle::Normal,
        FontWeight::NORMAL,
        false,
    )?;
    let (blob, _) = font.load(Some(cache))?.into_raw_parts();
    Some(FontData::from_blob(blob, font.index()).tweak(FontTweak {
        coords: VariationCoords::new([(b"wght", theme::font::WEIGHT)]),
        ..Default::default()
    }))
}

fn add(ctx: &egui::Context, id: &str, data: FontData, family: FontFamily, priority: FontPriority) {
    let families = vec![InsertFontFamily { family, priority }];
    ctx.add_font(FontInsert::new(id, data, families));
}
