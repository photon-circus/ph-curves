//! Structured source citation, distinct from generation/consumer policy.
//!
//! Host-only: module-local std link (crate root stays `#![no_std]`).

extern crate std;

use std::prelude::v1::*;
use std::{format, vec};

use serde::Deserialize;

use super::super::rustdoc::rustdoc_debug;
use super::{BoundaryDef, ObservationGuardBehaviorDef, ObservationGuardDef};

/// Caller-declared source citation.
///
/// This identifies an external document or catalog entry. It is not the
/// generator's selected model representation (formula text, point count, NTC
/// parameters) and it is not consumer policy (fit budget, boundaries,
/// emission status, observation-guard classification).
///
/// URLs are stored as opaque strings and are never fetched.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenance {
    /// Source title or stable identifier. Required on a source-backed family.
    pub identity: String,
    /// Document revision or version, when known.
    #[serde(default)]
    pub revision: Option<String>,
    /// Section, table, page, row, or equation locator.
    #[serde(default)]
    pub locator: Option<String>,
    /// Optional URL. Stored, never fetched.
    #[serde(default)]
    pub url: Option<String>,
    /// Optional free-form note.
    #[serde(default)]
    pub note: Option<String>,
}

impl SourceProvenance {
    /// Citation whose only required field is `identity`.
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            revision: None,
            locator: None,
            url: None,
            note: None,
        }
    }

    /// Document revision or version.
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }

    /// Section, table, page, row, or equation locator.
    pub fn with_locator(mut self, locator: impl Into<String>) -> Self {
        self.locator = Some(locator.into());
        self
    }

    /// Opaque URL string. Never fetched.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Free-form note attached to the citation.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Merge an override.
    ///
    /// Set fields replace and explicitly cleared fields become absent. Unset
    /// fields inherit while identity is unchanged. Replacing identity starts a
    /// new citation, so every unspecified optional field is cleared instead of
    /// being silently attached to the new document.
    pub fn merge(&self, overlay: &SourceProvenanceOverride) -> Result<Self, String> {
        overlay.validate()?;
        let replaces_identity = overlay
            .identity
            .as_deref()
            .is_some_and(|identity| identity != self.identity);
        let merged = Self {
            identity: overlay
                .identity
                .clone()
                .unwrap_or_else(|| self.identity.clone()),
            revision: merge_optional_field(
                self.revision.as_ref(),
                overlay.revision.as_ref(),
                overlay.clears(SourceProvenanceField::Revision),
                replaces_identity,
            ),
            locator: merge_optional_field(
                self.locator.as_ref(),
                overlay.locator.as_ref(),
                overlay.clears(SourceProvenanceField::Locator),
                replaces_identity,
            ),
            url: merge_optional_field(
                self.url.as_ref(),
                overlay.url.as_ref(),
                overlay.clears(SourceProvenanceField::Url),
                replaces_identity,
            ),
            note: merge_optional_field(
                self.note.as_ref(),
                overlay.note.as_ref(),
                overlay.clears(SourceProvenanceField::Note),
                replaces_identity,
            ),
        };
        merged.validate()?;
        Ok(merged)
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        reject_blank("provenance.identity", &self.identity)?;
        reject_optional_blank("provenance.revision", self.revision.as_deref())?;
        reject_optional_blank("provenance.locator", self.locator.as_deref())?;
        reject_optional_blank("provenance.url", self.url.as_deref())?;
        reject_optional_blank("provenance.note", self.note.as_deref())?;
        Ok(())
    }

    /// Debug-formatted citation fields for generated rustdoc.
    pub(crate) fn rustdoc_clause(&self) -> String {
        let mut parts = vec![format!("identity {}", rustdoc_debug(&self.identity))];
        if let Some(revision) = &self.revision {
            parts.push(format!("revision {}", rustdoc_debug(revision)));
        }
        if let Some(locator) = &self.locator {
            parts.push(format!("locator {}", rustdoc_debug(locator)));
        }
        if let Some(url) = &self.url {
            parts.push(format!("url {}", rustdoc_debug(url)));
        }
        if let Some(note) = &self.note {
            parts.push(format!("note {}", rustdoc_debug(note)));
        }
        parts.join("; ")
    }
}

fn merge_optional_field(
    base: Option<&String>,
    replacement: Option<&String>,
    clear: bool,
    replaces_identity: bool,
) -> Option<String> {
    if clear {
        None
    } else if let Some(replacement) = replacement {
        Some(replacement.clone())
    } else if replaces_identity {
        None
    } else {
        base.cloned()
    }
}

/// Optional citation field that an override can explicitly clear.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvenanceField {
    /// Document revision or version.
    Revision,
    /// Section, table, page, row, or equation locator.
    Locator,
    /// Opaque source URL.
    Url,
    /// Free-form source note.
    Note,
}

impl SourceProvenanceField {
    fn name(self) -> &'static str {
        match self {
            Self::Revision => "revision",
            Self::Locator => "locator",
            Self::Url => "url",
            Self::Note => "note",
        }
    }
}

/// Partial citation that replaces declared fields and can clear optional ones.
///
/// Used on family members, gaps, and observation-guard tables. A standalone
/// `TransferDef` uses [`SourceProvenance`] directly. Unset optional fields
/// inherit unless `identity` changes, which starts a new citation and resets
/// them. Resolving without a parent still requires `identity`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenanceOverride {
    /// Replacement identity. Inherited when omitted.
    #[serde(default)]
    pub identity: Option<String>,
    /// Replacement revision. Inherited when omitted and identity is unchanged.
    #[serde(default)]
    pub revision: Option<String>,
    /// Replacement locator. Inherited when omitted and identity is unchanged.
    #[serde(default)]
    pub locator: Option<String>,
    /// Replacement URL. Inherited when omitted and identity is unchanged. Never fetched.
    #[serde(default)]
    pub url: Option<String>,
    /// Replacement note. Inherited when omitted and identity is unchanged.
    #[serde(default)]
    pub note: Option<String>,
    /// Optional inherited fields to remove.
    ///
    /// TOML example: `clear = ["revision", "url"]`.
    #[serde(default)]
    pub clear: Vec<SourceProvenanceField>,
}

impl SourceProvenanceOverride {
    /// Citation that sets only `identity`.
    ///
    /// When it replaces a different identity, unspecified optional fields are
    /// cleared. When it repeats the parent identity, they inherit.
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: Some(identity.into()),
            ..Self::default()
        }
    }

    /// Explicitly remove one inherited optional field.
    pub fn clearing(mut self, field: SourceProvenanceField) -> Self {
        if !self.clear.contains(&field) {
            self.clear.push(field);
        }
        self
    }

    /// Resolve against an optional parent citation.
    ///
    /// Missing `identity` after inheritance fails: a source-backed citation
    /// must name its document.
    pub fn resolve(&self, base: Option<&SourceProvenance>) -> Result<SourceProvenance, String> {
        match base {
            Some(base) => base.merge(self),
            None => {
                self.validate()?;
                if let Some(field) = self.clear.first() {
                    return Err(format!(
                        "provenance.clear cannot remove `{}` without inherited source provenance",
                        field.name()
                    ));
                }
                let provenance = SourceProvenance {
                    identity: self.identity.clone().unwrap_or_default(),
                    revision: self.revision.clone(),
                    locator: self.locator.clone(),
                    url: self.url.clone(),
                    note: self.note.clone(),
                };
                if provenance.identity.trim().is_empty() {
                    return Err("source-backed citation requires provenance.identity".into());
                }
                provenance.validate()?;
                Ok(provenance)
            }
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        reject_optional_blank("provenance.identity", self.identity.as_deref())?;
        reject_optional_blank("provenance.revision", self.revision.as_deref())?;
        reject_optional_blank("provenance.locator", self.locator.as_deref())?;
        reject_optional_blank("provenance.url", self.url.as_deref())?;
        reject_optional_blank("provenance.note", self.note.as_deref())?;
        for (index, field) in self.clear.iter().enumerate() {
            if self.clear[..index].contains(field) {
                return Err(format!(
                    "provenance.clear contains duplicate field `{}`",
                    field.name()
                ));
            }
            let is_set = match field {
                SourceProvenanceField::Revision => self.revision.is_some(),
                SourceProvenanceField::Locator => self.locator.is_some(),
                SourceProvenanceField::Url => self.url.is_some(),
                SourceProvenanceField::Note => self.note.is_some(),
            };
            if is_set {
                return Err(format!(
                    "provenance.{} cannot be both set and cleared",
                    field.name()
                ));
            }
        }
        Ok(())
    }

    fn clears(&self, field: SourceProvenanceField) -> bool {
        self.clear.contains(&field)
    }
}

/// Observation-guard behavior projected without any source citation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationGuardPolicy {
    /// Observation code classified by this policy.
    pub code: u16,
    /// Behavior applied when the code is observed.
    pub behavior: ObservationGuardBehaviorDef,
}

/// Consumer/generator policy, distinct from [`SourceProvenance`].
///
/// Fitting budget, knot cap, boundary behavior, and observation-guard
/// classification live here. Applicability windows stay on the member as
/// source facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationPolicy {
    /// Requested interpolation error bound in output quanta.
    pub max_interpolation_error: u32,
    /// Knot budget for the greedy fitter.
    pub max_knots: usize,
    /// Observation-domain below policy.
    pub below: BoundaryDef,
    /// Observation-domain above policy.
    pub above: BoundaryDef,
    /// Explicit observation-code guard (TOML `saturation`), when declared.
    pub observation_guard: Option<ObservationGuardPolicy>,
}

impl GenerationPolicy {
    pub(crate) fn new(
        max_interpolation_error: u32,
        max_knots: usize,
        below: BoundaryDef,
        above: BoundaryDef,
        observation_guard: Option<&ObservationGuardDef>,
    ) -> Self {
        Self {
            max_interpolation_error,
            max_knots,
            below,
            above,
            observation_guard: observation_guard.map(|guard| ObservationGuardPolicy {
                code: guard.code,
                behavior: guard.behavior,
            }),
        }
    }

    pub(crate) fn rustdoc_clause(&self) -> String {
        format!(
            "requested interpolation error <= {}; max_knots = {}; below = {}; above = {}",
            self.max_interpolation_error,
            self.max_knots,
            self.below.toml_name(),
            self.above.toml_name(),
        )
    }
}

fn reject_blank(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field} must not be blank"))
    } else {
        Ok(())
    }
}

fn reject_optional_blank(field: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(value) => reject_blank(field, value),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_replaces_only_declared_fields() {
        let base = SourceProvenance::new("datasheet")
            .with_revision("1.0")
            .with_locator("Table 1");
        let overlay = SourceProvenanceOverride {
            locator: Some("§4.2".into()),
            ..SourceProvenanceOverride::default()
        };
        let resolved = base.merge(&overlay).unwrap();
        assert_eq!(resolved.identity, "datasheet");
        assert_eq!(resolved.revision.as_deref(), Some("1.0"));
        assert_eq!(resolved.locator.as_deref(), Some("§4.2"));
    }

    #[test]
    fn override_without_parent_requires_identity() {
        let overlay = SourceProvenanceOverride {
            locator: Some("Table 1".into()),
            ..SourceProvenanceOverride::default()
        };
        let error = overlay.resolve(None).unwrap_err();
        assert!(
            error.contains("source-backed citation requires provenance.identity"),
            "{error}"
        );
    }

    #[test]
    fn replacing_identity_does_not_inherit_old_document_fields() {
        let base = SourceProvenance::new("old document")
            .with_revision("old revision")
            .with_locator("old table")
            .with_url("https://old.invalid")
            .with_note("old note");
        let resolved = base
            .merge(&SourceProvenanceOverride::new("new document"))
            .unwrap();

        assert_eq!(resolved.identity, "new document");
        assert_eq!(resolved.revision, None);
        assert_eq!(resolved.locator, None);
        assert_eq!(resolved.url, None);
        assert_eq!(resolved.note, None);
    }

    #[test]
    fn override_can_clear_one_inherited_optional_field() {
        let base = SourceProvenance::new("datasheet")
            .with_revision("1.0")
            .with_locator("Table 1");
        let overlay = SourceProvenanceOverride::default().clearing(SourceProvenanceField::Locator);
        let resolved = base.merge(&overlay).unwrap();

        assert_eq!(resolved.identity, "datasheet");
        assert_eq!(resolved.revision.as_deref(), Some("1.0"));
        assert_eq!(resolved.locator, None);
    }

    #[test]
    fn override_rejects_setting_and_clearing_the_same_field() {
        let overlay = SourceProvenanceOverride {
            locator: Some("Table 2".into()),
            clear: vec![SourceProvenanceField::Locator],
            ..SourceProvenanceOverride::default()
        };
        let error = SourceProvenance::new("datasheet")
            .merge(&overlay)
            .unwrap_err();
        assert!(
            error.contains("provenance.locator cannot be both set and cleared"),
            "{error}"
        );
    }

    #[test]
    fn override_rejects_duplicate_clear_fields() {
        let overlay = SourceProvenanceOverride {
            clear: vec![
                SourceProvenanceField::Locator,
                SourceProvenanceField::Locator,
            ],
            ..SourceProvenanceOverride::default()
        };
        let error = SourceProvenance::new("datasheet")
            .merge(&overlay)
            .unwrap_err();
        assert!(
            error.contains("provenance.clear contains duplicate field `locator`"),
            "{error}"
        );
    }

    #[test]
    fn override_rejects_unknown_clear_field() {
        let error = toml::from_str::<SourceProvenanceOverride>(
            "identity = \"datasheet\"\nclear = [\"uri\"]\n",
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("unknown variant `uri`") && error.contains("revision"),
            "{error}"
        );
    }

    #[test]
    fn override_rejects_clear_without_parent_provenance() {
        let overlay =
            SourceProvenanceOverride::new("standalone").clearing(SourceProvenanceField::Locator);
        let error = overlay.resolve(None).unwrap_err();
        assert!(
            error.contains(
                "provenance.clear cannot remove `locator` without inherited source provenance"
            ),
            "{error}"
        );
    }

    #[test]
    fn rustdoc_clause_escapes_markdown_html_urls_and_backtick_runs() {
        let clause = SourceProvenance::new("[datasheet]")
            .with_url("https://example.com/datasheet.pdf")
            .with_note("`code` ````` <tag> & http://example.org/note")
            .rustdoc_clause();
        assert!(clause.contains(r#"identity "\[datasheet\]""#), "{clause}");
        assert!(
            clause.contains(r#"url `"https://example.com/datasheet.pdf"`"#),
            "{clause}"
        );
        assert!(clause.contains("note ``````\""), "{clause}");
        assert!(clause.ends_with("\"``````"), "{clause}");
    }

    #[test]
    fn blank_identity_is_rejected() {
        let error = SourceProvenance::new("   ").validate().unwrap_err();
        assert!(
            error.contains("provenance.identity must not be blank"),
            "{error}"
        );
    }

    #[test]
    fn blank_optional_field_is_rejected() {
        let error = SourceProvenance::new("datasheet")
            .with_url("  ")
            .validate()
            .unwrap_err();
        assert!(
            error.contains("provenance.url must not be blank"),
            "{error}"
        );
    }
}
