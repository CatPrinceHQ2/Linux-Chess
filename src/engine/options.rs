//! Engine option model. Nothing here knows about any particular engine: options are whatever
//! the engine declared with `option name ... type ...` lines.
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OptionKind {
    Check { default: bool },
    Spin { default: i64, min: i64, max: i64 },
    Combo { default: String, choices: Vec<String> },
    #[serde(rename = "string")]
    Str { default: String },
    Button,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct EngineOption {
    pub name: String,
    #[serde(flatten)]
    pub kind: OptionKind,
}

impl EngineOption {
    pub fn is_button(&self) -> bool {
        matches!(self.kind, OptionKind::Button)
    }

    /// The default value rendered the way it is sent in `setoption ... value <x>`.
    pub fn default_value(&self) -> Option<String> {
        match &self.kind {
            OptionKind::Check { default } => Some(default.to_string()),
            OptionKind::Spin { default, .. } => Some(default.to_string()),
            OptionKind::Combo { default, .. } => Some(default.clone()),
            OptionKind::Str { default } => Some(default.clone()),
            OptionKind::Button => None,
        }
    }

    /// Validate and normalise a user-supplied value for this option.
    pub fn normalize_value(&self, raw: &str) -> Result<String, String> {
        match &self.kind {
            OptionKind::Check { .. } => match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "on" | "yes" => Ok("true".into()),
                "false" | "0" | "off" | "no" => Ok("false".into()),
                _ => Err(format!("'{}' expects true or false", self.name)),
            },
            OptionKind::Spin { min, max, .. } => {
                let v: i64 = raw
                    .trim()
                    .parse()
                    .map_err(|_| format!("'{}' expects a whole number", self.name))?;
                if v < *min || v > *max {
                    return Err(format!("'{}' must be between {min} and {max}", self.name));
                }
                Ok(v.to_string())
            }
            OptionKind::Combo { choices, .. } => choices
                .iter()
                .find(|c| c.eq_ignore_ascii_case(raw.trim()))
                .cloned()
                .ok_or_else(|| format!("'{}' must be one of: {}", self.name, choices.join(", "))),
            OptionKind::Str { .. } => {
                if raw.contains('\n') || raw.contains('\r') {
                    Err(format!("'{}' must not contain line breaks", self.name))
                } else {
                    Ok(raw.to_string())
                }
            }
            OptionKind::Button => Ok(String::new()),
        }
    }
}

/// Look up an option by name, ignoring case (engines disagree about `Hash` vs `hash`, and so on).
pub fn find_option<'a>(options: &'a [EngineOption], name: &str) -> Option<&'a EngineOption> {
    options.iter().find(|o| o.name.eq_ignore_ascii_case(name))
}

/// Which convenient global controls a given engine can honour. Every field is derived from the
/// engine's own declarations; a control is only offered when the option exists with a numeric range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub threads: Option<(String, i64, i64)>,
    pub hash_mb: Option<(String, i64, i64)>,
    pub multipv: Option<(String, i64, i64)>,
}

impl Capabilities {
    pub fn from_options(options: &[EngineOption]) -> Capabilities {
        let spin = |name: &str| {
            find_option(options, name).and_then(|o| match &o.kind {
                OptionKind::Spin { min, max, .. } => Some((o.name.clone(), *min, *max)),
                _ => None,
            })
        };
        Capabilities { threads: spin("Threads"), hash_mb: spin("Hash"), multipv: spin("MultiPV") }
    }
}
