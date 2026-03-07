use serde::de::{self, Deserialize, Deserializer};

pub(crate) fn normalize_enum_input(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .collect()
}

pub(crate) fn deserialize_string_enum<'de, D, T, F>(
    deserializer: D,
    parse_normalized: F,
    expected: &'static [&'static str],
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    F: Fn(&str) -> Option<T>,
{
    let raw = String::deserialize(deserializer)?;
    let normalized = normalize_enum_input(&raw);
    parse_normalized(&normalized).ok_or_else(|| de::Error::unknown_variant(raw.as_str(), expected))
}
