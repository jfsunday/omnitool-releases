// Java's core/Lang.java: the English UI text is the key, lang_de.rs maps it
// to German, a missing entry stays English. Which language applies (the
// `language` setting, AUTO = the game's language code) is decided here; the
// setting itself lives in core.rs. Added to the game as `game::omnitool_lang`;
// also a module of this crate, checked against tests/fixtures/core-translation.json.

use super::omnitool_lang_de::DE;

/// The `language` setting's values (Java's `Lang.Language`).
pub const LANGUAGES: &[&str] = &["AUTO", "ENGLISH", "DEUTSCH"];

/// Java's `german()`: DEUTSCH yes, ENGLISH no, AUTO when the game's language
/// code starts with `de` (no code: English).
pub fn german(language: &str, game_code: Option<&str>) -> bool {
    match language {
        "DEUTSCH" => true,
        "ENGLISH" => false,
        _ => game_code.is_some_and(|c| c.starts_with("de")),
    }
}

/// The German text for `english`, if there is one.
pub fn german_text(english: &str) -> Option<&'static str> {
    DE.binary_search_by(|(k, _)| k.as_bytes().cmp(english.as_bytes()))
        .ok()
        .map(|i| DE[i].1)
}

/// Java's `tr`: German when `german` and translated, else unchanged.
pub fn tr(german: bool, english: &str) -> &str {
    if !german {
        return english;
    }
    german_text(english).unwrap_or(english)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn read(path: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn table_matches_de_json() {
        let de = read(concat!(env!("CARGO_MANIFEST_DIR"), "/lang/de.json"));
        let de = de.as_object().unwrap();
        assert_eq!(DE.len(), de.len(), "run tools/gen_lang_de.py");
        for (k, v) in de {
            assert_eq!(german_text(k), v.as_str(), "{k}: run tools/gen_lang_de.py");
        }
    }

    #[test]
    fn translation_fixture() {
        let fx = read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/core-translation.json"
        ));
        for c in fx["tr"].as_array().unwrap() {
            let g = german(
                c["language"].as_str().unwrap(),
                c.get("game_language_code").and_then(Value::as_str),
            );
            let got = c["input"].as_str().map(|s| tr(g, s));
            assert_eq!(got, c["expected"].as_str(), "{c}");
        }
        for (en, de) in fx["sample_pairs_de"].as_object().unwrap() {
            assert_eq!(tr(true, en), de.as_str().unwrap());
        }
        let rules = &fx["catalog_rules"];
        assert!(DE.len() >= rules["entry_count_at_spec_time"].as_u64().unwrap() as usize);
        for (en, de) in DE {
            assert!(!de.trim().is_empty(), "{en}");
            for token in rules["placeholder_tokens_kept"].as_array().unwrap() {
                let t = token.as_str().unwrap();
                assert!(!en.contains(t) || de.contains(t), "{en}: {t}");
            }
        }
        for k in fx["placeholder_keys"].as_array().unwrap() {
            assert!(german_text(k.as_str().unwrap()).is_some(), "{k}");
        }
        assert_eq!(
            LANGUAGES,
            fx["languages"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap())
                .collect::<Vec<_>>()
        );
    }
}
