use super::*;
use std::collections::{HashMap, HashSet};

fn loader_for(language: &str) -> FluentLanguageLoader {
    let loader = fluent_language_loader!();
    i18n_embed::select(&loader, &Localizations, &[language.parse().unwrap()]).unwrap();
    loader
}

#[test]
fn french_regional_locale_and_english_fallback() {
    let french = loader_for("fr-FR");
    assert_eq!(french.get("models"), "Modèles");
    assert_eq!(french.get("recording"), "Enregistrement");
    assert_eq!(french.get("stop-manual"), "Arrêt manuel uniquement");
    assert_eq!(loader_for("en").get("models"), "Models");
    assert_eq!(loader_for("zz").get("models"), "Models");
}

#[test]
fn french_dynamic_message_preserves_model_name() {
    let loader = loader_for("fr");
    let args = HashMap::from([("target_model", "whisper-small")]);
    assert_eq!(
        loader.get_args("loading-model", args),
        "Chargement de \u{2068}whisper-small\u{2069}"
    );
}

#[test]
fn french_catalog_covers_english_messages_and_variables() {
    fn messages(path: &str) -> HashMap<String, HashSet<String>> {
        let resource = Localizations::get(path).unwrap();
        let text = std::str::from_utf8(&resource.data).unwrap();
        let mut messages = HashMap::new();
        let mut current = None;
        for line in text.lines() {
            if !line.starts_with(char::is_whitespace)
                && let Some((id, _)) = line.split_once(" = ")
            {
                assert!(
                    messages.insert(id.to_string(), HashSet::new()).is_none(),
                    "duplicate {id}"
                );
                current = Some(id.to_string());
            }
            if let Some(id) = &current {
                for part in line.split('$').skip(1) {
                    let name: String = part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                        .collect();
                    messages.get_mut(id).unwrap().insert(name);
                }
            }
        }
        messages
    }
    assert_eq!(
        messages("en/super_voice_app.ftl"),
        messages("fr/super_voice_app.ftl")
    );
}

/// The loader finds its fallback language under the crate's own name. A
/// rename that leaves the file under the old one panics the app at startup,
/// and nothing else starts the app in a test.
#[test]
fn the_fallback_language_loads() {
    let _ = &*LANGUAGE_LOADER;
}

/// A count passed as text picks its plural form, as an app in another language
/// passes it.
#[test]
fn a_count_picks_its_plural_form() {
    let one = text("notes-count", &[("count", "1")]);
    let three = text("notes-count", &[("count", "3")]);
    assert!(one.ends_with(" note"), "{one}");
    assert!(three.ends_with(" notes"), "{three}");
}

#[test]
fn a_key_with_no_arguments_reads_as_is() {
    assert_eq!(text("all-notes", &[]), LANGUAGE_LOADER.get("all-notes"));
}

/// A figure with a decimal keeps it, as the app shows it.
#[test]
fn a_decimal_figure_keeps_its_decimal() {
    let pair = text("memory-pair", &[("value0", "4.9"), ("value1", "8.0")]);
    assert!(pair.contains("8.0"), "{pair}");
}
