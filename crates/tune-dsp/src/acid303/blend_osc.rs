//! Port of `rosic_BlendOscillator`: a wavetable oscillator that blends
//! linearly between two waveforms with one shared phase accumulator.
//!
//! The C++ borrows two externally owned `MipMappedWaveTable`s through
//! pointers; here the oscillator owns both, so the null checks vanish.

use super::wavetable::{TABLE_LENGTH, WaveTable, Waveform};

/// `rosic::BlendOscillator`.
#[derive(Clone, Debug)]
pub struct BlendOscillator {
    /// `tableLength` as double.
    table_length_dbl: f64,
    /// Current phase index.
    phase_index: f64,
    /// Frequency of the oscillator.
    freq: f64,
    /// Phase increment per sample.
    increment: f64,
    /// Blend factor between the two waveforms.
    blend: f64,
    /// Start phase index (range 0 - tableLength).
    start_index: f64,
    sample_rate: f64,
    sample_rate_rec: f64,
    wave_table_1: WaveTable,
    wave_table_2: WaveTable,
}

impl Default for BlendOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl BlendOscillator {
    /// Constructor. The wavetables start unrendered (silent), like the
    /// C++ ones before `setWaveForm1/2` is called on a wired oscillator.
    pub fn new() -> Self {
        let table_length_dbl = TABLE_LENGTH as f64;
        let freq = 440.0;
        let sample_rate = 44100.0;
        let mut s = Self {
            table_length_dbl,
            phase_index: 0.0,
            freq,
            increment: (table_length_dbl * freq) / sample_rate,
            blend: 0.0,
            start_index: 0.0,
            sample_rate,
            sample_rate_rec: 0.0,
            wave_table_1: WaveTable::new(),
            wave_table_2: WaveTable::new(),
        };
        // somewhat redundant, as in the C++:
        s.set_sample_rate(44100.0);
        s.set_frequency(440.0);
        s.set_start_phase(0.0);
        s.reset_phase();
        s
    }

    // parameter settings

    /// `setSampleRate`.
    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
        }
        self.sample_rate_rec = 1.0 / self.sample_rate;
        self.increment = self.table_length_dbl * self.freq * self.sample_rate_rec;
    }

    /// `setWaveForm1`.
    pub fn set_wave_form_1(&mut self, new_wave_form: Waveform) {
        self.wave_table_1.set_waveform(new_wave_form);
    }

    /// `setWaveForm2`.
    pub fn set_wave_form_2(&mut self, new_wave_form: Waveform) {
        self.wave_table_2.set_waveform(new_wave_form);
    }

    /// `setStartPhase` (range 0 - 360 degrees).
    pub fn set_start_phase(&mut self, start_phase: f64) {
        if (0.0..=360.0).contains(&start_phase) {
            self.start_index = (start_phase / 360.0) * self.table_length_dbl;
        }
    }

    /// `setWaveTable1`: hands the oscillator its 1st wavetable.
    pub fn set_wave_table_1(&mut self, new_wave_table: WaveTable) {
        self.wave_table_1 = new_wave_table;
    }

    /// `setWaveTable2`: hands the oscillator its 2nd wavetable.
    pub fn set_wave_table_2(&mut self, new_wave_table: WaveTable) {
        self.wave_table_2 = new_wave_table;
    }

    /// Access to the 1st wavetable (the C++ keeps a pointer to it).
    pub fn wave_table_1_mut(&mut self) -> &mut WaveTable {
        &mut self.wave_table_1
    }

    /// Access to the 2nd wavetable.
    pub fn wave_table_2_mut(&mut self) -> &mut WaveTable {
        &mut self.wave_table_2
    }

    /// `setBlendFactor`: 0 is waveform 1 only, 1 is waveform 2 only.
    pub fn set_blend_factor(&mut self, new_blend_factor: f64) {
        self.blend = new_blend_factor;
    }

    /// `setFrequency`; values outside (0, 20000) Hz are ignored.
    pub fn set_frequency(&mut self, new_frequency: f64) {
        if new_frequency > 0.0 && new_frequency < 20000.0 {
            self.freq = new_frequency;
        }
    }

    /// `setPulseWidth` (percent).
    pub fn set_pulse_width(&mut self, new_pulse_width: f64) {
        self.wave_table_1.set_symmetry(0.01 * new_pulse_width);
        self.wave_table_2.set_symmetry(0.01 * new_pulse_width);
    }

    /// `setIncrement`.
    pub fn set_increment(&mut self, new_increment: f64) {
        self.increment = new_increment;
    }

    // inquiry

    pub fn blend_factor(&self) -> f64 {
        self.blend
    }

    pub fn increment(&self) -> f64 {
        self.increment
    }

    // audio processing

    /// `getSample`.
    #[inline]
    pub fn get_sample(&mut self) -> f64 {
        // from the increment, decide which table is to be used. EXPOFDBL
        // extracts the biased exponent as unsigned 64 bit, subtracts 1023
        // (wrapping for increments below 1) and the C++ then casts to int:
        let exp = ((self.increment.to_bits() & 0x7FFF_FFFF_FFFF_FFFF) >> 52).wrapping_sub(1023);
        let table_number = (exp as i32).wrapping_add(2); // up to nyquist/4

        // wraparound if necessary:
        while self.phase_index >= self.table_length_dbl {
            self.phase_index -= self.table_length_dbl;
        }

        let int_index = self.phase_index.floor() as i32;
        let frac = self.phase_index - int_index as f64;
        let out1 = (1.0 - self.blend)
            * self
                .wave_table_1
                .get_value_linear_int(int_index, frac, table_number);
        let mut out2 = self.blend
            * self
                .wave_table_2
                .get_value_linear_int(int_index, frac, table_number);

        out2 *= 0.5; // preliminary scaling of the square, as in the C++

        self.phase_index += self.increment;
        out1 + out2
    }

    // others

    /// `calculateIncrement`.
    pub fn calculate_increment(&mut self) {
        self.increment = self.table_length_dbl * self.freq * self.sample_rate_rec;
    }

    /// `resetPhase`.
    pub fn reset_phase(&mut self) {
        self.phase_index = self.start_index;
    }

    /// `setPhase`.
    pub fn set_phase(&mut self, phase_index: f64) {
        self.phase_index = self.start_index + phase_index;
    }
}
