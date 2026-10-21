//! Port of `rosic_MipMappedWaveTable`: a single-cycle waveform plus a
//! one-table-per-octave band-limited mip-map built by FFT.
//!
//! Not ported because nothing in the oscillator path reaches them:
//! `setWaveform(double*, int)`, `fillWithPeak`, `fillWithMoogSaw`,
//! `removeDC`, `normalize` and `reverseTime`.
//!
//! The C++ `generateMipMap` keeps its spectrum scratch buffer in a
//! function-level `static`, shared by every instance. Here each table owns
//! its scratch buffer, so instances never interfere; the values are the same
//! because the buffer is fully overwritten before every read.
#![allow(clippy::excessive_precision, clippy::approx_constant)]

use super::fft::FourierTransformerRadix2;
use super::math::{PI, amp2db, clip, db2amp, round_to_int};

/// `MipMappedWaveTable::waveforms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Waveform {
    Silence = 0,
    Sine = 1,
    Triangle = 2,
    Square = 3,
    Saw = 4,
    Square303 = 5,
    Saw303 = 6,
}

/// `MipMappedWaveTable::tableLength`.
pub const TABLE_LENGTH: usize = 2048;
/// `MipMappedWaveTable::numTables`.
pub const NUM_TABLES: usize = 12;
/// Row stride of the table set: the table plus 4 interpolation samples.
const ROW: usize = TABLE_LENGTH + 4;

/// `rosic::MipMappedWaveTable`.
#[derive(Clone, Debug)]
pub struct WaveTable {
    /// Symmetry between 1st and 2nd half-wave.
    symmetry: f64,
    /// Currently chosen native waveform.
    waveform: Waveform,
    prototype_table: Vec<f64>,
    /// `NUM_TABLES` rows of `ROW` samples; row 0 has full bandwidth, each
    /// following row is band-limited one octave lower.
    table_set: Vec<f64>,
    /// Replaces the function-level statics of `generateMipMap`.
    spectrum: Vec<f64>,
    fourier_transformer: FourierTransformerRadix2,
    tanh_shaper_factor: f64,
    tanh_shaper_offset: f64,
    square_phase_shift: f64,
}

impl Default for WaveTable {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveTable {
    pub fn new() -> Self {
        Self {
            symmetry: 0.5,
            waveform: Waveform::Silence,
            // The C++ prototype table is `tableLength` long but its init loop
            // clears `tableLength+4`; the extra samples are never read.
            prototype_table: vec![0.0; ROW],
            table_set: vec![0.0; NUM_TABLES * ROW],
            spectrum: vec![0.0; TABLE_LENGTH],
            fourier_transformer: FourierTransformerRadix2::new(TABLE_LENGTH),
            tanh_shaper_factor: db2amp(36.9),
            tanh_shaper_offset: 4.37,
            square_phase_shift: 180.0,
        }
    }

    // parameter settings

    /// `setWaveform(int)`: renders the waveform when it differs from the
    /// current one.
    pub fn set_waveform(&mut self, new_waveform: Waveform) {
        if new_waveform != self.waveform {
            self.waveform = new_waveform;
            self.render_waveform();
        }
    }

    /// `setSymmetry`.
    pub fn set_symmetry(&mut self, new_symmetry: f64) {
        self.symmetry = new_symmetry;
        self.render_waveform();
    }

    /// `setTanhShaperDriveFor303Square` (drive in dB).
    pub fn set_tanh_shaper_drive_for_303_square(&mut self, new_drive: f64) {
        self.tanh_shaper_factor = db2amp(new_drive);
        self.fill_with_square_303();
    }

    /// `setTanhShaperOffsetFor303Square`.
    pub fn set_tanh_shaper_offset_for_303_square(&mut self, new_offset: f64) {
        self.tanh_shaper_offset = new_offset;
        self.fill_with_square_303();
    }

    /// `set303SquarePhaseShift` (degrees). Like the C++, this renders the
    /// 303 square into the table whatever the current waveform is.
    pub fn set_303_square_phase_shift(&mut self, new_shift: f64) {
        self.square_phase_shift = new_shift;
        self.fill_with_square_303();
    }

    // inquiry

    pub fn tanh_shaper_drive_for_303_square(&self) -> f64 {
        amp2db(self.tanh_shaper_factor)
    }

    pub fn tanh_shaper_offset_for_303_square(&self) -> f64 {
        self.tanh_shaper_offset
    }

    pub fn square_phase_shift_303(&self) -> f64 {
        self.square_phase_shift
    }

    // audio processing

    /// `getValueLinear(int integerPart, double fractionalPart, int tableIndex)`.
    /// Like the C++, an index above `NUM_TABLES` selects table 11 and an
    /// index of 12 exactly is out of range (a panic here, UB there).
    #[inline]
    pub fn get_value_linear_int(
        &self,
        integer_part: i32,
        fractional_part: f64,
        table_index: i32,
    ) -> f64 {
        let table_index = if table_index <= 0 {
            0
        } else if table_index > NUM_TABLES as i32 {
            11
        } else {
            table_index
        } as usize;
        let row = &self.table_set[table_index * ROW..(table_index + 1) * ROW];
        let i = integer_part as usize;
        (1.0 - fractional_part) * row[i] + fractional_part * row[i + 1]
    }

    /// `getValueLinear(double phaseIndex, int tableIndex)`.
    #[inline]
    pub fn get_value_linear(&self, phase_index: f64, table_index: i32) -> f64 {
        let int_index = phase_index.floor() as i32;
        let frac = phase_index - int_index as f64;
        self.get_value_linear_int(int_index, frac, table_index)
    }

    /// Copy of mip-map level `index`, the `TABLE_LENGTH` samples without the
    /// interpolation tail.
    pub fn table(&self, index: usize) -> &[f64] {
        &self.table_set[index * ROW..index * ROW + TABLE_LENGTH]
    }

    // internal functions

    fn render_waveform(&mut self) {
        match self.waveform {
            Waveform::Sine => self.fill_with_sine(),
            Waveform::Triangle => self.fill_with_triangle(),
            Waveform::Square => self.fill_with_square(),
            Waveform::Saw => self.fill_with_saw(),
            Waveform::Square303 => self.fill_with_square_303(),
            Waveform::Saw303 => self.fill_with_saw_303(),
            Waveform::Silence => self.fill_with_sine(),
        }
    }

    fn generate_mip_map(&mut self) {
        // copy the prototype table into the 1st table of the mip-map:
        self.table_set[..TABLE_LENGTH].copy_from_slice(&self.prototype_table[..TABLE_LENGTH]);

        // additional samples for the interpolator:
        for k in 0..4 {
            self.table_set[TABLE_LENGTH + k] = self.table_set[k];
        }

        // get the spectrum from the prototype table:
        self.fourier_transformer
            .transform_real_signal(&self.prototype_table, &mut self.spectrum);

        // ensure that DC and Nyquist are zero:
        self.spectrum[0] = 0.0;
        self.spectrum[1] = 0.0;

        // render the band-limited versions by successively shrinking the
        // spectrum by one octave and transforming it back:
        for t in 1..NUM_TABLES {
            let low_bin = (TABLE_LENGTH as f64 / 2.0f64.powf(t as f64)) as usize;
            let high_bin = (TABLE_LENGTH as f64 / 2.0f64.powf((t - 1) as f64)) as usize;

            for i in low_bin..high_bin {
                self.spectrum[i] = 0.0;
            }

            let row = &mut self.table_set[t * ROW..(t + 1) * ROW];
            self.fourier_transformer
                .transform_symmetric_spectrum(&self.spectrum, row);

            for k in 0..4 {
                row[TABLE_LENGTH + k] = row[k];
            }
        }
    }

    /// The saw ramp shared by `fillWithSaw`, `fillWithSaw303` and
    /// `fillWithSquare303` (the latter two use `k = 0.5`).
    fn fill_saw_ramp(&mut self, k: f64) {
        let n = TABLE_LENGTH as i32;
        let n1 = clip(round_to_int(k * (n - 1) as f64), 1, n - 1);
        let n2 = n - n1;
        let s1 = 1.0 / (n1 - 1) as f64;
        let s2 = 1.0 / n2 as f64;
        for i in 0..n1 {
            self.prototype_table[i as usize] = s1 * i as f64;
        }
        for i in n1..n {
            self.prototype_table[i as usize] = -1.0 + s2 * (i - n1) as f64;
        }
    }

    fn fill_with_sine(&mut self) {
        for i in 0..TABLE_LENGTH {
            self.prototype_table[i] = ((2.0 * PI * i as f64) / TABLE_LENGTH as f64).sin();
        }
        self.generate_mip_map();
    }

    fn fill_with_triangle(&mut self) {
        let tl = TABLE_LENGTH;
        for i in 0..(tl / 4) {
            self.prototype_table[i] = (4 * i) as f64 / tl as f64;
        }
        for i in (tl / 4)..(3 * tl / 4) {
            self.prototype_table[i] = 2.0 - ((4 * i) as f64 / tl as f64);
        }
        for i in (3 * tl / 4)..tl {
            self.prototype_table[i] = -4.0 + ((4 * i) as f64 / tl as f64);
        }
        self.generate_mip_map();
    }

    fn fill_with_square(&mut self) {
        let n = TABLE_LENGTH as i32;
        let k = self.symmetry;
        let n1 = clip(round_to_int(k * (n - 1) as f64), 1, n - 1);
        for i in 0..n1 {
            self.prototype_table[i as usize] = 1.0;
        }
        for i in n1..n {
            self.prototype_table[i as usize] = -1.0;
        }
        self.generate_mip_map();
    }

    fn fill_with_saw(&mut self) {
        self.fill_saw_ramp(self.symmetry);
        self.generate_mip_map();
    }

    fn fill_with_square_303(&mut self) {
        // generate the saw wave:
        self.fill_saw_ramp(0.5);

        // switch polarity and apply tanh-shaping with dc-offset:
        for i in 0..TABLE_LENGTH {
            self.prototype_table[i] = -(self.tanh_shaper_factor * self.prototype_table[i]
                + self.tanh_shaper_offset)
                .tanh();
        }

        // circular shift to phase-align with the saw wave:
        let n_shift = round_to_int(TABLE_LENGTH as f64 * self.square_phase_shift / 360.0);
        circular_shift(&mut self.prototype_table[..TABLE_LENGTH], n_shift);

        self.generate_mip_map();
    }

    fn fill_with_saw_303(&mut self) {
        self.fill_saw_ramp(0.5);
        self.generate_mip_map();
    }
}

/// `circularShift`: positive values shift towards higher indices.
fn circular_shift(buffer: &mut [f64], num_positions: i32) {
    let length = buffer.len();
    let mut na = num_positions.unsigned_abs() as usize;
    while na > length {
        na -= length;
    }
    if num_positions < 0 {
        buffer.rotate_left(na);
    } else if num_positions > 0 {
        buffer.rotate_right(na);
    }
}
