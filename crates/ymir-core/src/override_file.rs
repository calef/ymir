//! Override file parsing and validation for injecting real observational data
//! into any pipeline stage.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Per-stage override data stored as raw JSON values.
///
/// Each field corresponds to a pipeline stage. When present, the JSON value
/// replaces the entire computed output for that stage. The concrete
/// deserialization into stage-specific types happens downstream in ymir-stages,
/// not here in ymir-core.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StageOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orbital_body: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub climate: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub biome: Option<serde_json::Value>,
}

/// A parsed override file that specifies replacement values for pipeline stages.
///
/// Override files target a specific star (and optionally a planet within that
/// star's system). The `overrides` field contains raw JSON for each stage that
/// should be overridden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverrideFile {
    pub version: String,
    pub target_star: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_planet: Option<String>,
    #[serde(default)]
    pub overrides: StageOverrides,
}

impl OverrideFile {
    /// Load and parse an override file from disk.
    pub fn load(path: &Path) -> Result<Self, OverrideError> {
        let contents = std::fs::read_to_string(path)?;
        Self::from_json(&contents)
    }

    /// Parse an override file from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, OverrideError> {
        // First do a raw parse to check for required fields before serde
        // tries to fill in defaults.
        let raw: serde_json::Value = serde_json::from_str(json)?;
        let obj = raw.as_object().ok_or_else(|| {
            OverrideError::Parse(serde_json::from_str::<OverrideFile>("[]").unwrap_err())
        })?;

        if !obj.contains_key("version") {
            return Err(OverrideError::MissingField("version".to_string()));
        }
        if !obj.contains_key("target_star") {
            return Err(OverrideError::MissingField("target_star".to_string()));
        }

        let file: OverrideFile = serde_json::from_value(raw)?;
        file.validate()?;
        Ok(file)
    }

    /// Validate the parsed override file contents.
    fn validate(&self) -> Result<(), OverrideError> {
        if self.version != "1.0" {
            return Err(OverrideError::InvalidVersion(self.version.clone()));
        }
        if self.target_star.is_empty() {
            return Err(OverrideError::MissingField("target_star".to_string()));
        }
        Ok(())
    }

    /// Returns the names of stages that have overrides present.
    pub fn overridden_stages(&self) -> Vec<&str> {
        let mut stages = Vec::new();
        if self.overrides.orbital_body.is_some() {
            stages.push("orbital_body");
        }
        if self.overrides.atmosphere.is_some() {
            stages.push("atmosphere");
        }
        if self.overrides.skeleton.is_some() {
            stages.push("skeleton");
        }
        if self.overrides.climate.is_some() {
            stages.push("climate");
        }
        if self.overrides.biome.is_some() {
            stages.push("biome");
        }
        stages
    }
}

/// Errors that can occur when loading or validating an override file.
#[derive(Debug)]
pub enum OverrideError {
    /// I/O error reading the file from disk.
    Io(std::io::Error),
    /// JSON parse error.
    Parse(serde_json::Error),
    /// The version field contains an unsupported value.
    InvalidVersion(String),
    /// A required field is missing or empty.
    MissingField(String),
}

impl fmt::Display for OverrideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OverrideError::Io(err) => write!(f, "I/O error: {err}"),
            OverrideError::Parse(err) => write!(f, "JSON parse error: {err}"),
            OverrideError::InvalidVersion(v) => {
                write!(f, "unsupported override file version: {v}")
            }
            OverrideError::MissingField(field) => {
                write!(f, "missing required field: {field}")
            }
        }
    }
}

impl std::error::Error for OverrideError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OverrideError::Io(err) => Some(err),
            OverrideError::Parse(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for OverrideError {
    fn from(err: std::io::Error) -> Self {
        OverrideError::Io(err)
    }
}

impl From<serde_json::Error> for OverrideError {
    fn from(err: serde_json::Error) -> Self {
        OverrideError::Parse(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_json() -> String {
        json!({
            "version": "1.0",
            "target_star": "Proxima Centauri",
            "target_planet": "b",
            "overrides": {
                "orbital_body": {
                    "mass_earth": 1.07,
                    "semi_major_axis_au": 0.0485
                },
                "atmosphere": {
                    "surface_pressure_atm": 1.2,
                    "composition": {"N2": 0.78, "O2": 0.21, "CO2": 0.01}
                }
            }
        })
        .to_string()
    }

    #[test]
    fn parse_valid_override_file() {
        let file = OverrideFile::from_json(&valid_json()).expect("should parse");
        assert_eq!(file.version, "1.0");
        assert_eq!(file.target_star, "Proxima Centauri");
        assert_eq!(file.target_planet.as_deref(), Some("b"));
        assert!(file.overrides.orbital_body.is_some());
        assert!(file.overrides.atmosphere.is_some());
        assert!(file.overrides.skeleton.is_none());
        assert!(file.overrides.climate.is_none());
        assert!(file.overrides.biome.is_none());
    }

    #[test]
    fn overridden_stages_returns_correct_names() {
        let file = OverrideFile::from_json(&valid_json()).expect("should parse");
        let stages = file.overridden_stages();
        assert_eq!(stages, vec!["orbital_body", "atmosphere"]);
    }

    #[test]
    fn missing_version_field() {
        let json = json!({
            "target_star": "Proxima Centauri"
        })
        .to_string();
        let err = OverrideFile::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OverrideError::MissingField(ref f) if f == "version"),
            "expected MissingField(\"version\"), got: {err:?}"
        );
    }

    #[test]
    fn invalid_version() {
        let json = json!({
            "version": "2.0",
            "target_star": "Proxima Centauri"
        })
        .to_string();
        let err = OverrideFile::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OverrideError::InvalidVersion(ref v) if v == "2.0"),
            "expected InvalidVersion(\"2.0\"), got: {err:?}"
        );
    }

    #[test]
    fn empty_json_object() {
        let json = "{}";
        let err = OverrideFile::from_json(json).unwrap_err();
        assert!(
            matches!(err, OverrideError::MissingField(_)),
            "expected MissingField, got: {err:?}"
        );
    }

    #[test]
    fn round_trip_serialize_deserialize() {
        let original = OverrideFile::from_json(&valid_json()).expect("should parse");
        let serialized = serde_json::to_string(&original).expect("should serialize");
        let restored = OverrideFile::from_json(&serialized).expect("should deserialize");
        assert_eq!(original, restored);
    }

    #[test]
    fn empty_target_star_rejected() {
        let json = json!({
            "version": "1.0",
            "target_star": ""
        })
        .to_string();
        let err = OverrideFile::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OverrideError::MissingField(ref f) if f == "target_star"),
            "expected MissingField(\"target_star\"), got: {err:?}"
        );
    }

    #[test]
    fn missing_target_star_field() {
        let json = json!({
            "version": "1.0"
        })
        .to_string();
        let err = OverrideFile::from_json(&json).unwrap_err();
        assert!(
            matches!(err, OverrideError::MissingField(ref f) if f == "target_star"),
            "expected MissingField(\"target_star\"), got: {err:?}"
        );
    }

    #[test]
    fn no_overrides_section_uses_defaults() {
        let json = json!({
            "version": "1.0",
            "target_star": "TRAPPIST-1"
        })
        .to_string();
        let file = OverrideFile::from_json(&json).expect("should parse");
        assert!(file.overridden_stages().is_empty());
    }

    #[test]
    fn load_nonexistent_file() {
        let err = OverrideFile::load(Path::new("/nonexistent/path.json")).unwrap_err();
        assert!(
            matches!(err, OverrideError::Io(_)),
            "expected Io error, got: {err:?}"
        );
    }

    #[test]
    fn error_display_messages() {
        let err = OverrideError::InvalidVersion("3.0".to_string());
        assert_eq!(err.to_string(), "unsupported override file version: 3.0");

        let err = OverrideError::MissingField("version".to_string());
        assert_eq!(err.to_string(), "missing required field: version");
    }
}
