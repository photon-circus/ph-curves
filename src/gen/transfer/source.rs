//! Programmatic transfer sources for host tools that own evaluation.

// Host-only: module-local std link (crate root stays `#![no_std]`).
extern crate std;

use std::prelude::v1::*;

use super::{BoundaryDef, PhysicalPoint, TransferDef, default_boundary};

fn default_max_knots() -> usize {
    256
}

/// Dense unscaled physical samples over a contiguous `u16` observation domain.
///
/// `physical[i]` is the value at `domain_min + i`. Host tools evaluate their
/// own model into this series; ph-curves scales, fits, and emits the table.
#[derive(Clone, Debug)]
pub struct EvaluatedTruth {
    domain_min: u16,
    physical: Vec<f64>,
}

impl EvaluatedTruth {
    /// Samples covering `domain_min` through `domain_min + physical.len() - 1`.
    pub fn new(domain_min: u16, physical: Vec<f64>) -> Self {
        Self {
            domain_min,
            physical,
        }
    }

    /// First observation code in the series.
    pub fn domain_min(&self) -> u16 {
        self.domain_min
    }

    /// Unscaled physical values, one per consecutive input code.
    pub fn physical(&self) -> &[f64] {
        &self.physical
    }
}

/// Generation input that replaces a parsed formula, points table, or model.
///
/// This is not a plugin ABI: the caller evaluates device-owned truth (or
/// chooses knots) and hands the result to the existing fitter and codegen.
#[derive(Clone, Debug)]
pub enum TransferSource {
    /// Dense physical samples; the greedy fitter selects knots.
    EvaluatedTruth(EvaluatedTruth),
    /// Caller-chosen knots. The greedy fitter is skipped.
    ///
    /// When `truth` is `Some`, interpolation error is measured against that
    /// dense series. When `None`, metadata records that the bound was not
    /// verified against a source oracle.
    PrefittedKnots {
        /// Strictly increasing observation-domain knot codes.
        inputs: Vec<u16>,
        /// Scaled integer knot outputs (`physical * output_scale`, rounded).
        outputs: Vec<i32>,
        /// Optional dense unscaled truth covering the knot domain.
        truth: Option<EvaluatedTruth>,
    },
    /// Sparse physical control points, matching TOML `points`.
    Points(Vec<PhysicalPoint>),
}

impl TransferSource {
    /// Dense physical samples starting at `domain_min`.
    pub fn evaluated_truth(domain_min: u16, physical: Vec<f64>) -> Self {
        Self::EvaluatedTruth(EvaluatedTruth::new(domain_min, physical))
    }

    /// Prefitted knots without a dense oracle.
    pub fn prefitted_knots(inputs: Vec<u16>, outputs: Vec<i32>) -> Self {
        Self::PrefittedKnots {
            inputs,
            outputs,
            truth: None,
        }
    }

    /// Prefitted knots verified against dense unscaled truth.
    pub fn prefitted_knots_verified(
        inputs: Vec<u16>,
        outputs: Vec<i32>,
        truth: EvaluatedTruth,
    ) -> Self {
        Self::PrefittedKnots {
            inputs,
            outputs,
            truth: Some(truth),
        }
    }

    /// Sparse physical control points.
    pub fn points(points: Vec<PhysicalPoint>) -> Self {
        Self::Points(points)
    }
}

/// A standalone transfer constructed without TOML.
///
/// Fit policy (`max_interpolation_error`, `max_knots`, boundaries) lives here,
/// independent of dense curve LUT options.
#[derive(Clone, Debug)]
pub struct TransferSpec {
    name: String,
    input_unit: String,
    output_unit: String,
    output_scale: u32,
    max_interpolation_error: u32,
    max_knots: usize,
    below: BoundaryDef,
    above: BoundaryDef,
    source: TransferSource,
}

impl TransferSpec {
    /// Policy defaults: `max_knots = 256`, `below`/`above` = error.
    pub fn new(
        name: impl Into<String>,
        input_unit: impl Into<String>,
        output_unit: impl Into<String>,
        output_scale: u32,
        max_interpolation_error: u32,
        source: TransferSource,
    ) -> Self {
        Self {
            name: name.into(),
            input_unit: input_unit.into(),
            output_unit: output_unit.into(),
            output_scale,
            max_interpolation_error,
            max_knots: default_max_knots(),
            below: default_boundary(),
            above: default_boundary(),
            source,
        }
    }

    /// Knot budget for the greedy fitter (ignored for prefitted knots except
    /// as an upper bound on the supplied table).
    pub fn with_max_knots(mut self, max_knots: usize) -> Self {
        self.max_knots = max_knots;
        self
    }

    /// Observation-domain below/above policies.
    pub fn with_boundaries(mut self, below: BoundaryDef, above: BoundaryDef) -> Self {
        self.below = below;
        self.above = above;
        self
    }

    /// TOML-style table name / generated symbol stem.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Observation-domain unit label.
    pub fn input_unit(&self) -> &str {
        &self.input_unit
    }

    /// Physical-domain unit label.
    pub fn output_unit(&self) -> &str {
        &self.output_unit
    }

    /// Integer quanta per physical unit.
    pub fn output_scale(&self) -> u32 {
        self.output_scale
    }

    /// Requested interpolation error bound in output quanta.
    pub fn max_interpolation_error(&self) -> u32 {
        self.max_interpolation_error
    }

    /// Knot budget.
    pub fn max_knots(&self) -> usize {
        self.max_knots
    }

    /// Below-domain policy.
    pub fn below(&self) -> BoundaryDef {
        self.below
    }

    /// Above-domain policy.
    pub fn above(&self) -> BoundaryDef {
        self.above
    }

    /// Generation source.
    pub fn source(&self) -> &TransferSource {
        &self.source
    }

    pub(crate) fn into_parts(self) -> (String, TransferDef, TransferSource) {
        let def = TransferDef {
            input_unit: self.input_unit,
            output_unit: self.output_unit,
            output_scale: self.output_scale,
            max_interpolation_error: self.max_interpolation_error,
            max_knots: self.max_knots,
            below: self.below,
            above: self.above,
            points: None,
            formula: None,
            model: None,
            domain: None,
            output_range: None,
        };
        (self.name, def, self.source)
    }
}
