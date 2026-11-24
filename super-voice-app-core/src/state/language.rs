// SPDX-License-Identifier: GPL-3.0-only
//! How a model's language resolves.

/// The per-model language resolution block returned by
/// `GET /stt/pipeline/{stage}/model/{model}/language`, deserialized once at the
/// client boundary instead of being poked field-by-field as a
/// `serde_json::Value` in the views. Unknown/absent fields default so a partial
/// or null block yields an empty, harmless resolution.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct LanguageResolution {
    /// The effective BCP-47 tag in use, if any (`None` when unresolved).
    #[serde(default)]
    pub effective: Option<String>,
    /// How `effective` was resolved: `"override"`, `"global"`, or `"default"`.
    #[serde(default)]
    pub source: String,
    /// The global Primary Language tag used as the default fallback.
    #[serde(default)]
    pub primary: String,
}
