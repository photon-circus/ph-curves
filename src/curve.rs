//! Curve types and traits.
//!
//! A *curve* maps a normalized input position to an output value via a
//! pre-computed lookup table (LUT).  When the curve is *monotonic* an
//! inverse LUT is also available, enabling the tickless scheduler to work
//! backwards from a target output value to the corresponding input position.

use crate::math::UnitValue;

/// Curve evaluation trait.
///
/// `I` is the input (index / domain) type and `V` is the output (value / range)
/// type.  For the standard 8-bit case, both are `u8` and evaluation is a
/// single index into a `&'static [u8; 256]`.
///
/// # Example
///
/// ```ignore
/// use ph_curves::Curve;
///
/// let brightness: u8 = GAMMA_22.eval(input);
/// ```
pub trait Curve<I, V> {
    /// Evaluate the curve at the normalized position `u`.
    ///
    /// This is a single array lookup — O(1) with no arithmetic.
    fn eval(&self, u: I) -> V;
}

/// Trait for monotonic curves that can be inverted.
///
/// A curve is *monotonic* when its output never decreases (or never increases)
/// across its domain, guaranteeing a unique inverse mapping.  This is required
/// by the [`Tickless`](crate::Tickless) scheduler, which needs to map a target
/// output value back to an input position.
///
/// `I` is the input (index / domain) type and `V` is the output (value / range)
/// type.
pub trait MonotonicCurve<I, V>: Curve<I, V> {
    /// Look up the input position that maps to the output value `w`.
    ///
    /// This is a single array lookup into the inverse LUT — O(1).
    fn inv(&self, w: V) -> I;
}

/// A curve backed by an `N`-entry forward and optional `M`-entry inverse LUT.
///
/// `I` is the index type used to look up into the forward table, `V` is the
/// value type stored in each entry, and `N` / `M` are the forward / inverse
/// table sizes. Each table must contain one entry for every value in its
/// normalized domain: 256 entries for `u8` and 65,536 for `u16`.
///
/// # Panics
///
/// [`Curve::eval`] panics when `u.to_index() >= N`. If an inverse table is
/// present, [`MonotonicCurve::inv`] on the value returned by
/// [`CurveLut::monotonic`] panics when `w.to_index() >= M`.
#[derive(Copy, Clone, Debug)]
pub struct CurveLut<I: UnitValue, V: UnitValue, const N: usize, const M: usize = N> {
    /// Forward lookup table mapping `I` → `V`.
    pub(crate) fwd: &'static [V; N],
    /// Optional inverse lookup table mapping `V` → `I`.
    pub(crate) inv: Option<&'static [I; M]>,
}

/// A 256-entry curve lookup table (u8 domain and range).
pub type CurveLut256 = CurveLut<u8, u8, 256>;

/// A 65536-entry curve lookup table (u16 domain and range).
///
/// This alias is unavailable on 16-bit-pointer targets, where the array length
/// cannot be represented by `usize`.
#[cfg(not(target_pointer_width = "16"))]
pub type CurveLut65536 = CurveLut<u16, u16, 65536>;

impl<I: UnitValue, V: UnitValue, const N: usize, const M: usize> CurveLut<I, V, N, M> {
    /// Create a new curve from a forward LUT and an optional inverse LUT.
    ///
    /// Pass `Some(inv)` to enable [`CurveLut::monotonic`] conversion and
    /// inverse lookups.  Pass `None` if the curve is non-monotonic or the
    /// inverse table was not generated.
    ///
    /// The size invariant documented on [`CurveLut`] is a caller
    /// responsibility. This remains a `const fn` so generated and handwritten
    /// LUTs can be constructed in static storage; an undersized table is
    /// reported by the corresponding lookup panic.
    pub const fn new(fwd: &'static [V; N], inv: Option<&'static [I; M]>) -> Self {
        Self { fwd, inv }
    }

    #[inline(always)]
    /// Return the forward lookup table.
    pub const fn fwd_lut(&self) -> &'static [V; N] {
        self.fwd
    }

    #[inline(always)]
    /// Return the inverse lookup table if available.
    pub const fn inv_lut(&self) -> Option<&'static [I; M]> {
        self.inv
    }

    #[inline(always)]
    /// Convert to a [`MonotonicCurveLut`] if the inverse LUT is present.
    ///
    /// Returns `None` when the curve was constructed without an inverse table
    /// (i.e. `monotonic = false` in the TOML definition or `inv: None` in
    /// [`CurveLut::new`]).
    pub const fn monotonic(self) -> Option<MonotonicCurveLut<I, V, N, M>> {
        match self.inv {
            Some(inv) => Some(MonotonicCurveLut { fwd: self.fwd, inv }),
            None => None,
        }
    }
}

impl<I: UnitValue, V: UnitValue, const N: usize, const M: usize> Curve<I, V>
    for CurveLut<I, V, N, M>
{
    #[inline(always)]
    fn eval(&self, u: I) -> V {
        let index = u.to_index();
        assert!(index < N, "CurveLut forward LUT does not cover input index");
        self.fwd[index]
    }
}

/// A monotonic curve backed by an `N`-entry forward and `M`-entry inverse LUT.
///
/// Unlike [`CurveLut`], this type *guarantees* that an inverse table is
/// available, so it implements [`MonotonicCurve`] and can be used with the
/// [`Tickless`](crate::Tickless) scheduler.
///
/// `I` is the index type, `V` is the value type, and `N` / `M` are the forward
/// / inverse table sizes. Each table must contain one entry for every value in
/// its normalized domain: 256 entries for `u8` and 65,536 for `u16`.
///
/// # Panics
///
/// [`Curve::eval`] panics when `u.to_index() >= N` and
/// [`MonotonicCurve::inv`] panics when `w.to_index() >= M`.
#[derive(Copy, Clone, Debug)]
pub struct MonotonicCurveLut<I: UnitValue, V: UnitValue, const N: usize, const M: usize = N> {
    /// Forward lookup table mapping `I` → `V`.
    fwd: &'static [V; N],
    /// Inverse lookup table mapping `V` → `I`.
    inv: &'static [I; M],
}

/// A 256-entry monotonic curve lookup table (u8 domain and range).
pub type MonotonicCurveLut256 = MonotonicCurveLut<u8, u8, 256>;

/// A 65536-entry monotonic curve lookup table (u16 domain and range).
///
/// This alias is unavailable on 16-bit-pointer targets, where the array length
/// cannot be represented by `usize`.
#[cfg(not(target_pointer_width = "16"))]
pub type MonotonicCurveLut65536 = MonotonicCurveLut<u16, u16, 65536>;

impl<I: UnitValue, V: UnitValue, const N: usize, const M: usize> MonotonicCurveLut<I, V, N, M> {
    /// Create a new monotonic curve from forward and inverse LUTs.
    ///
    /// The size invariant documented on [`MonotonicCurveLut`] is a caller
    /// responsibility. This remains a `const fn` so generated and handwritten
    /// LUTs can be constructed in static storage; an undersized table is
    /// reported by the corresponding lookup panic.
    pub const fn new(fwd: &'static [V; N], inv: &'static [I; M]) -> Self {
        Self { fwd, inv }
    }

    #[inline(always)]
    /// Return the forward lookup table.
    pub const fn fwd_lut(&self) -> &'static [V; N] {
        self.fwd
    }

    #[inline(always)]
    /// Return the inverse lookup table.
    pub const fn inv_lut(&self) -> &'static [I; M] {
        self.inv
    }
}

impl<I: UnitValue, V: UnitValue, const N: usize, const M: usize> Curve<I, V>
    for MonotonicCurveLut<I, V, N, M>
{
    #[inline(always)]
    fn eval(&self, u: I) -> V {
        let index = u.to_index();
        assert!(
            index < N,
            "MonotonicCurveLut forward LUT does not cover input index"
        );
        self.fwd[index]
    }
}

impl<I: UnitValue, V: UnitValue, const N: usize, const M: usize> MonotonicCurve<I, V>
    for MonotonicCurveLut<I, V, N, M>
{
    #[inline(always)]
    fn inv(&self, w: V) -> I {
        let index = w.to_index();
        assert!(
            index < M,
            "MonotonicCurveLut inverse LUT does not cover value index"
        );
        self.inv[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SHORT: [u8; 255] = [0; 255];
    static FULL: [u8; 256] = [0; 256];

    const SHORT_CURVE: CurveLut<u8, u8, 255> = CurveLut::new(&SHORT, None);
    const SHORT_MONOTONIC_FORWARD: MonotonicCurveLut<u8, u8, 255, 256> =
        MonotonicCurveLut::new(&SHORT, &FULL);
    const SHORT_MONOTONIC_INVERSE: MonotonicCurveLut<u8, u8, 256, 255> =
        MonotonicCurveLut::new(&FULL, &SHORT);

    #[test]
    #[should_panic(expected = "CurveLut forward LUT does not cover input index")]
    fn undersized_curve_lut_has_an_explicit_eval_panic() {
        let _ = SHORT_CURVE.eval(u8::MAX);
    }

    #[test]
    #[should_panic(expected = "MonotonicCurveLut forward LUT does not cover input index")]
    fn undersized_monotonic_forward_lut_has_an_explicit_eval_panic() {
        let _ = SHORT_MONOTONIC_FORWARD.eval(u8::MAX);
    }

    #[test]
    #[should_panic(expected = "MonotonicCurveLut inverse LUT does not cover value index")]
    fn undersized_monotonic_inverse_lut_has_an_explicit_inv_panic() {
        let _ = SHORT_MONOTONIC_INVERSE.inv(u8::MAX);
    }
}
