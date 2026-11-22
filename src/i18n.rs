use std::sync::LazyLock;

use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{DesktopLanguageRequester, LanguageLoader};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "i18n"]
struct Localizations;

pub const LANGUAGES: [(&str, &str); 2] = [("en", "English"), ("zh-CN", "简体中文")];

pub static LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("embedded fallback language");
    loader.set_use_isolating(false);
    loader
});

#[macro_export]
macro_rules! fl {
    ($($args:tt)*) => {
        i18n_embed_fl::fl!($crate::i18n::LOADER, $($args)*)
    };
}

pub fn init(preferred: Option<&str>) {
    let requested = preferred
        .and_then(|l| l.parse::<LanguageIdentifier>().ok())
        .map(|l| vec![l])
        .unwrap_or_else(DesktopLanguageRequester::requested_languages);
    let _ = i18n_embed::select(&*LOADER, &Localizations, &requested);
}

pub fn set(language: &str) {
    init(Some(language));
}

pub fn current() -> String {
    LOADER.current_language().to_string()
}
