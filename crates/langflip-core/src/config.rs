//! Application configuration (`langflip.toml`), lookup precedence, and strict
//! fail-fast validation.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hotkey::HotkeySpec;
use crate::key::{key_name_for, parse_key_name};
use crate::layout::{LayoutId, LayoutPair};

pub const DEFAULT_MAX_BUFFER_LEN: usize = 128;
pub const MIN_BUFFER_LEN: usize = 1;
pub const MAX_BUFFER_LEN: usize = 512;
pub const DEFAULT_REPLAY_DELAY_MS: u64 = 1;
pub const MAX_REPLAY_DELAY_MS: u64 = 10;

/// Validated runtime configuration for LangFlip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub layouts: LayoutPair,
    pub hotkey: HotkeySpec,
    pub max_buffer_len: usize,
    pub replay_delay_ms: u64,
    pub toggle_on_empty_buffer: bool,
    pub auto_start: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            layouts: LayoutPair::default(),
            hotkey: HotkeySpec::default(),
            max_buffer_len: DEFAULT_MAX_BUFFER_LEN,
            replay_delay_ms: DEFAULT_REPLAY_DELAY_MS,
            toggle_on_empty_buffer: true,
            auto_start: true,
        }
    }
}

/// Configuration loading / validation errors (fail-fast at startup).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    Io(String),
    Parse(String),
    InvalidField { field: &'static str, reason: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "failed to read config: {msg}"),
            Self::Parse(msg) => write!(f, "invalid TOML in config: {msg}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid config field `{field}`: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    layouts: RawLayouts,
    #[serde(default)]
    hotkey: RawHotkey,
    #[serde(default)]
    behavior: RawBehavior,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawLayouts {
    primary: Option<String>,
    secondary: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawHotkey {
    key: Option<String>,
    ctrl: Option<bool>,
    alt: Option<bool>,
    shift: Option<bool>,
    meta: Option<bool>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawBehavior {
    max_buffer_len: Option<usize>,
    replay_delay_ms: Option<u64>,
    toggle_on_empty_buffer: Option<bool>,
    auto_start: Option<bool>,
}

impl AppConfig {
    /// Parses and strictly validates a TOML string. Rejects unknown/invalid keys
    /// or out-of-range values rather than silently clamping.
    pub fn from_toml(src: &str) -> Result<Self, ConfigError> {
        let raw: RawConfig =
            toml::from_str(src).map_err(|e| ConfigError::Parse(e.to_string()))?;

        let defaults = Self::default();

        let primary = raw
            .layouts
            .primary
            .map(LayoutId::new)
            .unwrap_or(defaults.layouts.primary);
        let secondary = raw
            .layouts
            .secondary
            .map(LayoutId::new)
            .unwrap_or(defaults.layouts.secondary);
        if primary.as_str().is_empty() || secondary.as_str().is_empty() {
            return Err(ConfigError::InvalidField {
                field: "layouts",
                reason: "primary and secondary layouts must be non-empty".into(),
            });
        }
        if primary == secondary {
            return Err(ConfigError::InvalidField {
                field: "layouts",
                reason: "primary and secondary layouts must be distinct".into(),
            });
        }

        let key = match raw.hotkey.key {
            Some(ref name) => parse_key_name(name).ok_or_else(|| ConfigError::InvalidField {
                field: "hotkey.key",
                reason: format!("unknown physical key name `{name}`"),
            })?,
            None => defaults.hotkey.key,
        };
        let hotkey = HotkeySpec {
            key,
            ctrl: raw.hotkey.ctrl.unwrap_or(defaults.hotkey.ctrl),
            alt: raw.hotkey.alt.unwrap_or(defaults.hotkey.alt),
            shift: raw.hotkey.shift.unwrap_or(defaults.hotkey.shift),
            meta: raw.hotkey.meta.unwrap_or(defaults.hotkey.meta),
        };

        let max_buffer_len = raw
            .behavior
            .max_buffer_len
            .unwrap_or(defaults.max_buffer_len);
        if !(MIN_BUFFER_LEN..=MAX_BUFFER_LEN).contains(&max_buffer_len) {
            return Err(ConfigError::InvalidField {
                field: "behavior.max_buffer_len",
                reason: format!(
                    "must be between {MIN_BUFFER_LEN} and {MAX_BUFFER_LEN} (got {max_buffer_len})"
                ),
            });
        }

        let replay_delay_ms = raw
            .behavior
            .replay_delay_ms
            .unwrap_or(defaults.replay_delay_ms);
        if replay_delay_ms > MAX_REPLAY_DELAY_MS {
            return Err(ConfigError::InvalidField {
                field: "behavior.replay_delay_ms",
                reason: format!("must be between 0 and {MAX_REPLAY_DELAY_MS} (got {replay_delay_ms})"),
            });
        }

        let toggle_on_empty_buffer = raw
            .behavior
            .toggle_on_empty_buffer
            .unwrap_or(defaults.toggle_on_empty_buffer);
        let auto_start = raw.behavior.auto_start.unwrap_or(defaults.auto_start);

        Ok(Self {
            layouts: LayoutPair { primary, secondary },
            hotkey,
            max_buffer_len,
            replay_delay_ms,
            toggle_on_empty_buffer,
            auto_start,
        })
    }

    /// Discovers and loads `langflip.toml` in precedence order:
    /// 1. `<exe_dir>/langflip.toml` (portable override)
    /// 2. `<os_config_dir>/langflip/langflip.toml`
    /// 3. Built-in defaults if neither file exists.
    pub fn load_with_precedence(
        exe_dir: Option<&Path>,
        os_config_dir: Option<&Path>,
    ) -> Result<(Self, Option<PathBuf>), ConfigError> {
        let candidates = [
            exe_dir.map(|d| d.join("langflip.toml")),
            os_config_dir.map(|d| d.join("langflip").join("langflip.toml")),
        ];

        for path in candidates.into_iter().flatten() {
            if path.is_file() {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))?;
                let cfg = Self::from_toml(&content)?;
                return Ok((cfg, Some(path)));
            }
        }

        Ok((Self::default(), None))
    }

    /// Serializes this validated configuration back to TOML, preserving all fields.
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        let key_name = key_name_for(self.hotkey.key).ok_or_else(|| ConfigError::InvalidField {
            field: "hotkey.key",
            reason: "cannot serialize unknown physical key".into(),
        })?;
        let raw = RawConfig {
            layouts: RawLayouts {
                primary: Some(self.layouts.primary.as_str().to_string()),
                secondary: Some(self.layouts.secondary.as_str().to_string()),
            },
            hotkey: RawHotkey {
                key: Some(key_name.to_string()),
                ctrl: Some(self.hotkey.ctrl),
                alt: Some(self.hotkey.alt),
                shift: Some(self.hotkey.shift),
                meta: Some(self.hotkey.meta),
            },
            behavior: RawBehavior {
                max_buffer_len: Some(self.max_buffer_len),
                replay_delay_ms: Some(self.replay_delay_ms),
                toggle_on_empty_buffer: Some(self.toggle_on_empty_buffer),
                auto_start: Some(self.auto_start),
            },
        };
        toml::to_string_pretty(&raw).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Resolves where to write `langflip.toml`: updates `loaded_path` if present,
    /// otherwise `<os_config_dir>/langflip/langflip.toml`.
    pub fn resolve_save_path(
        loaded_path: Option<&Path>,
        os_config_dir: Option<&Path>,
    ) -> Result<PathBuf, ConfigError> {
        if let Some(existing) = loaded_path {
            return Ok(existing.to_path_buf());
        }
        let dir = os_config_dir.ok_or_else(|| {
            ConfigError::Io("cannot determine OS config directory to save langflip.toml".into())
        })?;
        Ok(dir.join("langflip").join("langflip.toml"))
    }

    /// Persists this configuration to `path`, creating parent directories as needed.
    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        if self.layouts.primary == self.layouts.secondary {
            return Err(ConfigError::InvalidField {
                field: "layouts",
                reason: "primary and secondary layouts must be distinct".into(),
            });
        }
        let serialized = self.to_toml()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::Io(format!("{}: {e}", parent.display())))?;
        }
        std::fs::write(path, serialized)
            .map_err(|e| ConfigError::Io(format!("{}: {e}", path.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{PhysicalKey, scan};
    use crate::layout::LayoutId;

    #[test]
    fn empty_toml_produces_defaults() {
        let cfg = AppConfig::from_toml("").expect("empty TOML should use defaults");
        assert_eq!(cfg, AppConfig::default());
        assert_eq!(cfg.layouts.primary, LayoutId::new("en"));
        assert_eq!(cfg.layouts.secondary, LayoutId::new("he"));
        assert_eq!(cfg.hotkey.key, scan::PAUSE);
        assert_eq!(cfg.max_buffer_len, 128);
        assert_eq!(cfg.replay_delay_ms, 1);
        assert!(cfg.toggle_on_empty_buffer);
        assert!(cfg.auto_start);
    }

    #[test]
    fn custom_toml_overrides_layouts_hotkey_and_behavior() {
        let raw = r#"
            [layouts]
            primary = "00004009"
            secondary = "0002040D"

            [hotkey]
            key = "L"
            ctrl = true
            shift = true

            [behavior]
            max_buffer_len = 64
            replay_delay_ms = 2
            toggle_on_empty_buffer = false
        "#;
        let cfg = AppConfig::from_toml(raw).expect("valid custom TOML");
        assert_eq!(cfg.layouts.primary, LayoutId::new("00004009"));
        assert_eq!(cfg.layouts.secondary, LayoutId::new("0002040D"));
        assert_eq!(cfg.hotkey.key, PhysicalKey::new(0x26, false));
        assert!(cfg.hotkey.ctrl && cfg.hotkey.shift && !cfg.hotkey.alt && !cfg.hotkey.meta);
        assert_eq!(cfg.max_buffer_len, 64);
        assert_eq!(cfg.replay_delay_ms, 2);
        assert!(!cfg.toggle_on_empty_buffer);
    }

    #[test]
    fn out_of_range_and_invalid_values_are_rejected() {
        let too_big = "[behavior]\nmax_buffer_len = 9999\n";
        assert!(matches!(
            AppConfig::from_toml(too_big),
            Err(ConfigError::InvalidField { field: "behavior.max_buffer_len", .. })
        ));

        let zero_buf = "[behavior]\nmax_buffer_len = 0\n";
        assert!(matches!(
            AppConfig::from_toml(zero_buf),
            Err(ConfigError::InvalidField { field: "behavior.max_buffer_len", .. })
        ));

        let high_delay = "[behavior]\nreplay_delay_ms = 25\n";
        assert!(matches!(
            AppConfig::from_toml(high_delay),
            Err(ConfigError::InvalidField { field: "behavior.replay_delay_ms", .. })
        ));

        let bad_key = "[hotkey]\nkey = \"NotARealKey\"\n";
        assert!(matches!(
            AppConfig::from_toml(bad_key),
            Err(ConfigError::InvalidField { field: "hotkey.key", .. })
        ));

        let same_layouts = "[layouts]\nprimary = \"en\"\nsecondary = \"EN\"\n";
        assert!(matches!(
            AppConfig::from_toml(same_layouts),
            Err(ConfigError::InvalidField { field: "layouts", .. })
        ));
    }

    #[test]
    fn example_toml_file_parses_cleanly() {
        let example = include_str!("../../../langflip.example.toml");
        let cfg = AppConfig::from_toml(example).expect("langflip.example.toml must parse cleanly");
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn load_with_precedence_prefers_exe_dir_over_os_config_dir() {
        let tmp = std::env::temp_dir().join(format!("langflip_cfg_test_{}", std::process::id()));
        let exe_dir = tmp.join("exe");
        let os_dir = tmp.join("os");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(os_dir.join("langflip")).unwrap();

        std::fs::write(
            os_dir.join("langflip").join("langflip.toml"),
            "[behavior]\nmax_buffer_len = 50\n",
        )
        .unwrap();

        // Only OS config present -> loads 50
        let (cfg1, path1) = AppConfig::load_with_precedence(Some(&exe_dir), Some(&os_dir)).unwrap();
        assert_eq!(cfg1.max_buffer_len, 50);
        assert_eq!(path1, Some(os_dir.join("langflip").join("langflip.toml")));

        // Add exe-dir config -> takes precedence (loads 80)
        std::fs::write(exe_dir.join("langflip.toml"), "[behavior]\nmax_buffer_len = 80\n").unwrap();
        let (cfg2, path2) = AppConfig::load_with_precedence(Some(&exe_dir), Some(&os_dir)).unwrap();
        assert_eq!(cfg2.max_buffer_len, 80);
        assert_eq!(path2, Some(exe_dir.join("langflip.toml")));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn to_toml_and_save_to_path_round_trip_preserving_all_fields() {
        let cfg = AppConfig {
            layouts: LayoutPair {
                primary: LayoutId::new("00004009"),
                secondary: LayoutId::new("0002040D"),
            },
            hotkey: HotkeySpec {
                key: PhysicalKey::new(0x43, false), // F9
                ctrl: true,
                alt: false,
                shift: true,
                meta: false,
            },
            max_buffer_len: 200,
            replay_delay_ms: 3,
            toggle_on_empty_buffer: false,
            auto_start: false,
        };

        let serialized = cfg.to_toml().expect("to_toml should succeed");
        let reparsed = AppConfig::from_toml(&serialized).expect("serialized TOML must parse");
        assert_eq!(reparsed, cfg);

        let tmp = std::env::temp_dir().join(format!("langflip_save_test_{}", std::process::id()));
        let os_dir = tmp.join("appdata");
        let target = AppConfig::resolve_save_path(None, Some(&os_dir)).unwrap();
        assert_eq!(target, os_dir.join("langflip").join("langflip.toml"));

        cfg.save_to_path(&target).unwrap();
        let (loaded, loaded_from) = AppConfig::load_with_precedence(None, Some(&os_dir)).unwrap();
        assert_eq!(loaded, cfg);
        assert_eq!(loaded_from, Some(target));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
