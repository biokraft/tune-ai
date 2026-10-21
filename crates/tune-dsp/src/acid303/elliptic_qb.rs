//! Port of `rosic_EllipticQuarterBandFilter`: a 12th-order elliptic subband
//! filter in direct form II.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use super::math::TINY;

/// `rosic::EllipticQuarterBandFilter`.
#[derive(Clone, Debug)]
pub struct EllipticQuarterBandFilter {
    w: [f64; 12],
}

impl Default for EllipticQuarterBandFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl EllipticQuarterBandFilter {
    pub fn new() -> Self {
        Self { w: [0.0; 12] }
    }

    pub fn reset(&mut self) {
        self.w = [0.0; 12];
    }

    #[inline]
    pub fn get_sample(&mut self, input: f64) -> f64 {
        const A01: f64 = -9.1891604652189471;
        const A02: f64 = 40.177553696870497;
        const A03: f64 = -110.11636661771178;
        const A04: f64 = 210.18506612078195;
        const A05: f64 = -293.84744771903240;
        const A06: f64 = 308.16345558359234;
        const A07: f64 = -244.06786780384243;
        const A08: f64 = 144.81877911392738;
        const A09: f64 = -62.770692151724198;
        const A10: f64 = 18.867762095902137;
        const A11: f64 = -3.5327094230551848;
        const A12: f64 = 0.31183189275203149;
        const B00: f64 = 0.00013671732099945628;
        const B01: f64 = -0.00055538501265606384;
        const B02: f64 = 0.0013681887636296387;
        const B03: f64 = -0.0022158566490711852;
        const B04: f64 = 0.0028320091007278322;
        const B05: f64 = -0.0029776933151090413;
        const B06: f64 = 0.0030283628243514991;
        const B07: f64 = -0.0029776933151090413;
        const B08: f64 = 0.0028320091007278331;
        const B09: f64 = -0.0022158566490711861;
        const B10: f64 = 0.0013681887636296393;
        const B11: f64 = -0.00055538501265606384;
        const B12: f64 = 0.00013671732099945636;

        let w = &self.w;
        let tmp = (input + TINY)
            - ((A01 * w[0] + A02 * w[1]) + (A03 * w[2] + A04 * w[3]))
            - ((A05 * w[4] + A06 * w[5]) + (A07 * w[6] + A08 * w[7]))
            - ((A09 * w[8] + A10 * w[9]) + (A11 * w[10] + A12 * w[11]));
        let y = B00 * tmp
            + ((B01 * w[0] + B02 * w[1]) + (B03 * w[2] + B04 * w[3]))
            + ((B05 * w[4] + B06 * w[5]) + (B07 * w[6] + B08 * w[7]))
            + ((B09 * w[8] + B10 * w[9]) + (B11 * w[10] + B12 * w[11]));

        // update state variables (memmove(&w[1], &w[0], 11 * sizeof(double))):
        self.w.copy_within(0..11, 1);
        self.w[0] = tmp;
        y
    }
}
