//! Structured source citation, distinct from generation/consumer policy.
//!
//! Host-only: module-local std link (crate root stays `#![no_std]`).

extern crate std;

use std::prelude::v1::*;
use std::{format, vec};

use serde::Deserialize;

use super::{BoundaryDef, ObservationGuardDef};

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

    /// Merge an override: set fields replace, unset fields inherit.
    pub fn merge(&self, overlay: &SourceProvenanceOverride) -> Result<Self, String> {
        overlay.validate()?;
        let merged = Self {
            identity: overlay
                .identity
                .clone()
                .unwrap_or_else(|| self.identity.clone()),
            revision: overlay.revision.clone().or_else(|| self.revision.clone()),
            locator: overlay.locator.clone().or_else(|| self.locator.clone()),
            url: overlay.url.clone().or_else(|| self.url.clone()),
            note: overlay.note.clone().or_else(|| self.note.clone()),
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
        let mut parts = vec![format!("identity {:?}", self.identity)];
        if let Some(revision) = &self.revision {
            parts.push(format!("revision {revision:?}"));
        }
        if let Some(locator) = &self.locator {
            parts.push(format!("locator {locator:?}"));
        }
        if let Some(url) = &self.url {
            parts.push(format!("url {url:?}"));
        }
        if let Some(note) = &self.note {
            parts.push(format!("note {note:?}"));
        }
        parts.join("; ")
    }
}

/// Partial citation that replaces declared fields and inherits the rest.
///
/// Used on family members, gaps, standalone transfers, and observation-guard
/// tables. Resolving without a parent still requires `identity`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceProvenanceOverride {
    /// Replacement identity. Inherited when omitted.
    #[serde(default)]
    pub identity: Option<String>,
    /// Replacement revision. Inherited when omitted.
    #[serde(default)]
    pub revision: Option<String>,
    /// Replacement locator. Inherited when omitted.
    #[serde(default)]
    pub locator: Option<String>,
    /// Replacement URL. Inherited when omitted. Never fetched.
    #[serde(default)]
    pub url: Option<String>,
    /// Replacement note. Inherited when omitted.
    #[serde(default)]
    pub note: Option<String>,
}

impl SourceProvenanceOverride {
    /// Citation that sets only `identity`. Other fields inherit or stay unset.
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: Some(identity.into()),
            ..Self::default()
        }
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
        Ok(())
    }
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
    pub observation_guard: Option<ObservationGuardDef>,
}

impl GenerationPolicy {
    pub(crate) fn new(
        max_interpolation_error: u32,
        max_knots: usize,
        below: BoundaryDef,
        above: BoundaryDef,
        observation_guard: Option<ObservationGuardDef>,
    ) -> Self {
        Self {
            max_interpolation_error,
            max_knots,
            below,
            above,
            observation_guard,
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
