// SPDX-License-Identifier: GPL-3.0-only
//! The app's words: the Fluent catalogs in `i18n/`, one per language, English
//! the fallback. The app reads them from here, so each string is translated
//! once, and looks a message up by its key at run time with [`text`].

use std::collections::HashMap;
use std::sync::LazyLock;

use fluent::FluentValue;
use i18n_embed::fluent::{FluentLanguageLoader, fluent_language_loader};
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{DefaultLocalizer, LanguageLoader, Localizer};
use rust_embed::RustEmbed;

/// The catalogs, built into the library.
#[derive(RustEmbed)]
#[folder = "i18n/"]
pub struct Localizations;

/// The loader every app's lookups go through, set to the language
/// [`select`] picked.
pub static LANGUAGE_LOADER: LazyLock<FluentLanguageLoader> = LazyLock::new(|| {
    let loader: FluentLanguageLoader = fluent_language_loader!();
    loader
        .load_fallback_language(&Localizations)
        .expect("Error while loading fallback language");
    loader
});

/// Use the first of `requested` a catalog exists for, falling back to English.
pub fn select(requested: &[LanguageIdentifier]) {
    let localizer = DefaultLocalizer::new(&*LANGUAGE_LOADER, &Localizations);
    if let Err(why) = localizer.select(requested) {
        log::error!("error while loading fluent localizations: {why}");
    }
}

/// [`select`], from language tags as a platform gives them (`fr-CA`, `en`).
/// A tag that doesn't parse is skipped.
pub fn select_tags(requested: &[String]) {
    let languages: Vec<LanguageIdentifier> = requested
        .iter()
        .filter_map(|tag| tag.parse().ok())
        .collect();
    select(&languages);
}

/// The message `key` in the selected language, with `args` filled in.
///
/// An argument that reads as a whole number is passed as one, so a message
/// that picks its words by count ("1 note", "3 notes") picks the right ones.
/// Anything else stays text as given: a figure like "8.0" keeps its decimal,
/// which Fluent would drop from a number.
#[must_use]
pub fn text(key: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() {
        return LANGUAGE_LOADER.get(key);
    }
    let args: HashMap<&str, FluentValue<'_>> = args
        .iter()
        .map(|(name, value)| {
            let value = value
                .parse::<i64>()
                .map_or_else(|_| FluentValue::from(*value), FluentValue::from);
            (*name, value)
        })
        .collect();
    LANGUAGE_LOADER.get_args_concrete(key, args)
}

#[cfg(test)]
mod tests;
