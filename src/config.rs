use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Per-layer overrides read from a YAML config file.
#[derive(Deserialize)]
pub struct LayerFillConfig {
    /// Target metal density in percent.
    pub density: f64,
    /// Acceptable deviation from the target in percent.
    pub deviation: f64,
    /// Optional algorithm override (falls back to PDK default order).
    pub algorithm: Option<serde_yml::Value>,
}

/// Top-level fill configuration file (`--config-file`).
#[derive(Deserialize)]
pub struct FillConfig {
    #[serde(rename = "PDK")]
    pub pdk: Option<String>,
    pub layers: Option<HashMap<String, LayerFillConfig>>,
}

impl FillConfig {
    /// Parse a YAML config file from `path`.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yml::from_str(&content)?)
    }

    /// Return the names of all layers listed in the config, or an empty vec if
    /// no layer section is present.
    pub fn layer_names(&self) -> Vec<String> {
        self.layers
            .as_ref()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}
