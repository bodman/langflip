//! Opaque, platform-neutral keyboard layout identifiers, pairs, and pure
//! layout-resolution logic (exact ID -> exact BCP-47 -> primary language fallback).

use std::fmt;

/// Opaque, normalized keyboard layout identifier (e.g., `"en"`, `"en-US"`, `"00004009"`, `"0002040D"`).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct LayoutId(String);

impl LayoutId {
    pub fn new(raw: impl AsRef<str>) -> Self {
        Self(raw.as_ref().trim().to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for LayoutId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LayoutId({:?})", self.0)
    }
}

impl fmt::Display for LayoutId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An OS-installed keyboard layout reported by `LayoutController::list_installed_layouts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledLayout {
    pub id: LayoutId,
    pub language_tag: Option<String>,
    pub display_name: String,
}

/// Configured pair of layouts to toggle between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutPair {
    pub primary: LayoutId,
    pub secondary: LayoutId,
}

impl Default for LayoutPair {
    fn default() -> Self {
        Self {
            primary: LayoutId::new("en"),
            secondary: LayoutId::new("he"),
        }
    }
}

impl LayoutPair {
    /// Returns the opposite layout ONLY if `current` is a member of this pair.
    /// Returns `None` for a 3rd unrelated layout so non-empty bursts are never destructively erased.
    pub fn opposite_if_member(&self, current: &LayoutId) -> Option<&LayoutId> {
        if current == &self.primary {
            Some(&self.secondary)
        } else if current == &self.secondary {
            Some(&self.primary)
        } else {
            None
        }
    }

    /// Returns the target layout for an empty-buffer toggle (falls back to `primary` on a 3rd layout).
    pub fn target_for_empty_toggle(&self, current: &LayoutId) -> &LayoutId {
        self.opposite_if_member(current).unwrap_or(&self.primary)
    }
}

/// Non-fatal startup diagnostic produced by `resolve_layouts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutResolutionWarning {
    pub configured: LayoutId,
    pub message: String,
}

/// Pure layout resolution mapping configured `LayoutPair` against OS-installed layouts.
///
/// Resolution order per slot:
/// 1. Exact case-insensitive `InstalledLayout.id` match (e.g., `"0002040D"`).
/// 2. Exact case-insensitive `language_tag` match (e.g., `"en-IN"` or `"he-IL"`).
/// 3. Primary language subtag match (e.g., `"en"` matches `"en-IN"`, `"en-US"`; `"he"` matches `"he-IL"`).
///
/// Emits a warning if multiple installed layouts match (picking the first deterministically
/// and advising exact ID pinning) or if no installed layout matches.
pub fn resolve_layouts(
    installed: &[InstalledLayout],
    configured: &LayoutPair,
) -> (LayoutPair, Vec<LayoutResolutionWarning>) {
    let mut warnings = Vec::new();
    let primary = resolve_one(installed, &configured.primary, &mut warnings);
    let secondary = resolve_one(installed, &configured.secondary, &mut warnings);
    (LayoutPair { primary, secondary }, warnings)
}

fn resolve_one(
    installed: &[InstalledLayout],
    configured: &LayoutId,
    warnings: &mut Vec<LayoutResolutionWarning>,
) -> LayoutId {
    // 1. Exact ID match (e.g., exact Windows KLID "0002040d")
    if let Some(found) = installed.iter().find(|l| &l.id == configured) {
        return found.id.clone();
    }

    let wanted = configured.as_str();

    // 2. Exact BCP-47 language tag match (e.g., "he-il", "en-in")
    let exact_tag_matches: Vec<&InstalledLayout> = installed
        .iter()
        .filter(|l| {
            l.language_tag
                .as_deref()
                .is_some_and(|tag| tag.trim().eq_ignore_ascii_case(wanted))
        })
        .collect();
    if let Some(first) = exact_tag_matches.first() {
        if exact_tag_matches.len() > 1 {
            let ids: Vec<&str> = exact_tag_matches.iter().map(|l| l.id.as_str()).collect();
            warnings.push(LayoutResolutionWarning {
                configured: configured.clone(),
                message: format!(
                    "layout `{configured}` matched multiple installed layouts ({}); selected `{}` — pin an exact layout ID in langflip.toml to silence this warning",
                    ids.join(", "),
                    first.id.as_str().to_ascii_uppercase(),
                ),
            });
        }
        return first.id.clone();
    }

    // 3. Primary language subtag match (e.g., "en" matches "en-IN", "he" matches "he-IL")
    let wanted_primary = primary_subtag(wanted);
    let primary_matches: Vec<&InstalledLayout> = installed
        .iter()
        .filter(|l| {
            l.language_tag
                .as_deref()
                .is_some_and(|tag| primary_subtag(tag).eq_ignore_ascii_case(wanted_primary))
        })
        .collect();
    if let Some(first) = primary_matches.first() {
        if primary_matches.len() > 1 {
            let ids: Vec<&str> = primary_matches.iter().map(|l| l.id.as_str()).collect();
            warnings.push(LayoutResolutionWarning {
                configured: configured.clone(),
                message: format!(
                    "language `{configured}` matched multiple installed layouts ({}); selected `{}` — pin an exact layout ID in langflip.toml to silence this warning",
                    ids.join(", "),
                    first.id.as_str().to_ascii_uppercase(),
                ),
            });
        }
        return first.id.clone();
    }

    warnings.push(LayoutResolutionWarning {
        configured: configured.clone(),
        message: format!("configured layout `{configured}` is not installed on this system"),
    });
    configured.clone()
}

fn primary_subtag(tag: &str) -> &str {
    let trimmed = tag.trim();
    trimmed
        .split(['-', '_'])
        .next()
        .unwrap_or(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_installed() -> Vec<InstalledLayout> {
        vec![
            InstalledLayout {
                id: LayoutId::new("00004009"),
                language_tag: Some("en-IN".into()),
                display_name: "India".into(),
            },
            InstalledLayout {
                id: LayoutId::new("0002040D"),
                language_tag: Some("he-IL".into()),
                display_name: "Hebrew (Standard)".into(),
            },
        ]
    }

    #[test]
    fn layout_id_normalizes_whitespace_and_case() {
        assert_eq!(LayoutId::new("  0002040d "), LayoutId::new("0002040D"));
        assert_eq!(LayoutId::new("en-US"), LayoutId::new("EN-us"));
    }

    #[test]
    fn layout_pair_opposite_and_third_layout_safety() {
        let pair = LayoutPair {
            primary: LayoutId::new("00004009"),
            secondary: LayoutId::new("0002040D"),
        };
        let third = LayoutId::new("00000419"); // Russian

        assert_eq!(pair.opposite_if_member(&pair.primary), Some(&pair.secondary));
        assert_eq!(pair.opposite_if_member(&pair.secondary), Some(&pair.primary));
        assert_eq!(pair.opposite_if_member(&third), None);
        assert_eq!(pair.target_for_empty_toggle(&third), &pair.primary);
    }

    #[test]
    fn resolve_layouts_matches_default_en_he_to_installed_regional_variants() {
        let installed = sample_installed();
        let configured = LayoutPair::default(); // en <-> he

        let (resolved, warnings) = resolve_layouts(&installed, &configured);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(resolved.primary, LayoutId::new("00004009"));
        assert_eq!(resolved.secondary, LayoutId::new("0002040D"));
    }

    #[test]
    fn resolve_layouts_prefers_exact_klid_and_warns_on_ambiguity() {
        let mut installed = sample_installed();
        installed.push(InstalledLayout {
            id: LayoutId::new("0003040D"),
            language_tag: Some("he-IL".into()),
            display_name: "Hebrew (Standard, 2018)".into(),
        });

        // Exact KLID resolves cleanly with zero warnings
        let exact = LayoutPair {
            primary: LayoutId::new("00004009"),
            secondary: LayoutId::new("0003040D"),
        };
        let (resolved, warnings) = resolve_layouts(&installed, &exact);
        assert!(warnings.is_empty());
        assert_eq!(resolved.secondary, LayoutId::new("0003040D"));

        // Ambiguous tag picks deterministically and emits a warning
        let ambiguous = LayoutPair {
            primary: LayoutId::new("en"),
            secondary: LayoutId::new("he-IL"),
        };
        let (resolved2, warnings2) = resolve_layouts(&installed, &ambiguous);
        assert_eq!(resolved2.secondary, LayoutId::new("0002040D"));
        assert_eq!(warnings2.len(), 1);
        assert!(warnings2[0].message.contains("0002040D"));
    }

    #[test]
    fn resolve_layouts_warns_when_configured_layout_is_missing() {
        let installed = sample_installed();
        let missing = LayoutPair {
            primary: LayoutId::new("en"),
            secondary: LayoutId::new("ru-RU"),
        };
        let (resolved, warnings) = resolve_layouts(&installed, &missing);
        assert_eq!(resolved.secondary, LayoutId::new("ru-RU"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.to_lowercase().contains("not installed"));
    }
}
