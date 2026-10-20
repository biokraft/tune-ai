//! Safe wrappers over a C++ shim around the vendored Open303 sources.
//!
//! This crate is test infrastructure: the Rust port in `tune-dsp` is checked
//! sample-for-sample against these handles. Each `Ref*` type owns one C++
//! object and frees it on `Drop`. Method names mirror the C++ method names in
//! snake case; every call forwards directly to the C++ method (see
//! `shim/shim.cpp`).

use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// The vendored `MipMappedWaveTable::generateMipMap` keeps its scratch
/// spectrum in a function-level `static`, so two threads rebuilding tables at
/// once corrupt each other (cargo runs tests on parallel threads). Every call
/// into C++ takes this lock to make the reference deterministic and
/// thread-safe; the per-call cost is negligible next to the DSP work.
static CPP_LOCK: Mutex<()> = Mutex::new(());

fn cpp_lock() -> MutexGuard<'static, ()> {
    CPP_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Enumeration values of the C++ classes, for driving the handles.
pub mod consts {
    /// `OnePoleFilter::modes`.
    pub mod one_pole_mode {
        pub const BYPASS: i32 = 0;
        pub const LOWPASS: i32 = 1;
        pub const HIGHPASS: i32 = 2;
        pub const LOWSHELV: i32 = 3;
        pub const HIGHSHELV: i32 = 4;
        pub const ALLPASS: i32 = 5;
    }
    /// `BiquadFilter::modes`.
    pub mod biquad_mode {
        pub const BYPASS: i32 = 0;
        pub const LOWPASS6: i32 = 1;
        pub const LOWPASS12: i32 = 2;
        pub const HIGHPASS6: i32 = 3;
        pub const HIGHPASS12: i32 = 4;
        pub const BANDPASS: i32 = 5;
        pub const BANDREJECT: i32 = 6;
        pub const PEAK: i32 = 7;
        pub const LOW_SHELF: i32 = 8;
    }
    /// `MipMappedWaveTable::waveforms`.
    pub mod waveform {
        pub const SILENCE: i32 = 0;
        pub const SINE: i32 = 1;
        pub const TRIANGLE: i32 = 2;
        pub const SQUARE: i32 = 3;
        pub const SAW: i32 = 4;
        pub const SQUARE303: i32 = 5;
        pub const SAW303: i32 = 6;
    }
    /// `TeeBeeFilter::modes` (only the ones other tests are likely to need).
    pub mod tee_bee_mode {
        pub const FLAT: i32 = 0;
        pub const LP_6: i32 = 1;
        pub const LP_12: i32 = 2;
        pub const LP_18: i32 = 3;
        pub const LP_24: i32 = 4;
        pub const TB_303: i32 = 15;
    }
    /// `AcidSequencer::sequencerModes`.
    pub mod seq_mode {
        pub const OFF: i32 = 0;
        pub const KEY_SYNC: i32 = 1;
        pub const HOST_SYNC: i32 = 2;
    }
}

/// Length of one `MipMappedWaveTable` table (`tableLength`).
pub const TABLE_LENGTH: usize = 2048;
/// Number of mip-map levels in a `MipMappedWaveTable` (`numTables`).
pub const NUM_TABLES: usize = 12;

/// Defines a handle type that owns one C++ object plus its forwarding methods.
///
/// Each method line names the C shim symbol it forwards to. The extern
/// declaration is generated next to the call so signature and wrapper cannot
/// drift apart.
macro_rules! ref_handle {
    (
        $(#[$meta:meta])*
        $name:ident { new: $new:ident, free: $free:ident;
            $( $(#[$mm:meta])* fn $m:ident(&mut self $(, $a:ident: $t:ty)*) $(-> $r:ty)? = $c:ident; )*
        }
    ) => {
        $(#[$meta])*
        pub struct $name {
            ptr: *mut c_void,
        }

        impl $name {
            /// Creates the C++ object with its default constructor.
            pub fn new() -> Self {
                unsafe extern "C" {
                    fn $new() -> *mut c_void;
                }
                let _guard = cpp_lock();
                // SAFETY: the shim constructor takes no arguments and returns
                // an owned pointer or null.
                let ptr = unsafe { $new() };
                assert!(!ptr.is_null(), concat!(stringify!($name), ": C++ allocation failed"));
                Self { ptr }
            }

            $(
                $(#[$mm])*
                pub fn $m(&mut self $(, $a: $t)*) $(-> $r)? {
                    unsafe extern "C" {
                        fn $c(p: *mut c_void $(, $a: $t)*) $(-> $r)?;
                    }
                    let _guard = cpp_lock();
                    // SAFETY: `self.ptr` is a live object created by `$new`
                    // and only freed in `Drop`.
                    unsafe { $c(self.ptr $(, $a)*) }
                }
            )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                unsafe extern "C" {
                    fn $free(p: *mut c_void);
                }
                let _guard = cpp_lock();
                // SAFETY: `self.ptr` came from the matching constructor and
                // is freed exactly once.
                unsafe { $free(self.ptr) }
            }
        }
    };
}

ref_handle! {
    /// `rosic::LeakyIntegrator`.
    RefLeakyIntegrator { new: ref_leaky_new, free: ref_leaky_free;
        fn set_sample_rate(&mut self, v: f64) = ref_leaky_set_sample_rate;
        fn set_time_constant(&mut self, v: f64) = ref_leaky_set_time_constant;
        fn set_state(&mut self, v: f64) = ref_leaky_set_state;
        fn get_time_constant(&mut self) -> f64 = ref_leaky_get_time_constant;
        fn reset(&mut self) = ref_leaky_reset;
        fn get_sample(&mut self, input: f64) -> f64 = ref_leaky_get_sample;
    }
}

impl RefLeakyIntegrator {
    /// `LeakyIntegrator::getNormalizer` (static).
    pub fn get_normalizer(tau1: f64, tau2: f64, sample_rate: f64) -> f64 {
        unsafe extern "C" {
            fn ref_leaky_get_normalizer(tau1: f64, tau2: f64, sample_rate: f64) -> f64;
        }
        // SAFETY: pure function of its arguments.
        unsafe { ref_leaky_get_normalizer(tau1, tau2, sample_rate) }
    }
}

ref_handle! {
    /// `rosic::OnePoleFilter`.
    RefOnePole { new: ref_onepole_new, free: ref_onepole_free;
        fn set_sample_rate(&mut self, v: f64) = ref_onepole_set_sample_rate;
        fn set_mode(&mut self, v: i32) = ref_onepole_set_mode;
        fn set_cutoff(&mut self, v: f64) = ref_onepole_set_cutoff;
        fn set_lowpass_time_constant(&mut self, v: f64) = ref_onepole_set_lowpass_time_constant;
        fn set_shelving_gain(&mut self, v: f64) = ref_onepole_set_shelving_gain;
        fn set_shelving_gain_in_decibels(&mut self, v: f64) = ref_onepole_set_shelving_gain_in_decibels;
        fn set_coefficients(&mut self, b0: f64, b1: f64, a1: f64) = ref_onepole_set_coefficients;
        fn set_internal_state(&mut self, x1: f64, y1: f64) = ref_onepole_set_internal_state;
        fn get_cutoff(&mut self) -> f64 = ref_onepole_get_cutoff;
        fn reset(&mut self) = ref_onepole_reset;
        fn get_sample(&mut self, input: f64) -> f64 = ref_onepole_get_sample;
    }
}

ref_handle! {
    /// `rosic::BiquadFilter`.
    RefBiquad { new: ref_biquad_new, free: ref_biquad_free;
        fn set_sample_rate(&mut self, v: f64) = ref_biquad_set_sample_rate;
        fn set_mode(&mut self, v: i32) = ref_biquad_set_mode;
        fn set_frequency(&mut self, v: f64) = ref_biquad_set_frequency;
        fn set_gain(&mut self, v: f64) = ref_biquad_set_gain;
        fn set_bandwidth(&mut self, v: f64) = ref_biquad_set_bandwidth;
        fn get_mode(&mut self) -> i32 = ref_biquad_get_mode;
        fn get_frequency(&mut self) -> f64 = ref_biquad_get_frequency;
        fn get_gain(&mut self) -> f64 = ref_biquad_get_gain;
        fn get_bandwidth(&mut self) -> f64 = ref_biquad_get_bandwidth;
        fn reset(&mut self) = ref_biquad_reset;
        fn get_sample(&mut self, input: f64) -> f64 = ref_biquad_get_sample;
    }
}

ref_handle! {
    /// `rosic::EllipticQuarterBandFilter`.
    RefEllipticQB { new: ref_ellipticqb_new, free: ref_ellipticqb_free;
        fn reset(&mut self) = ref_ellipticqb_reset;
        fn get_sample(&mut self, input: f64) -> f64 = ref_ellipticqb_get_sample;
    }
}

ref_handle! {
    /// `rosic::DecayEnvelope`.
    RefDecayEnv { new: ref_decayenv_new, free: ref_decayenv_free;
        fn set_sample_rate(&mut self, v: f64) = ref_decayenv_set_sample_rate;
        fn set_decay_time_constant(&mut self, v: f64) = ref_decayenv_set_decay_time_constant;
        fn set_normalize_sum(&mut self, v: bool) = ref_decayenv_set_normalize_sum;
        fn get_decay_time_constant(&mut self) -> f64 = ref_decayenv_get_decay_time_constant;
        fn end_is_reached(&mut self, threshold: f64) -> bool = ref_decayenv_end_is_reached;
        fn trigger(&mut self) = ref_decayenv_trigger;
        fn get_sample(&mut self) -> f64 = ref_decayenv_get_sample;
    }
}

ref_handle! {
    /// `rosic::AnalogEnvelope`.
    RefAnalogEnv { new: ref_analogenv_new, free: ref_analogenv_free;
        fn set_sample_rate(&mut self, v: f64) = ref_analogenv_set_sample_rate;
        fn set_start_level(&mut self, v: f64) = ref_analogenv_set_start_level;
        fn set_start_in_decibels(&mut self, v: f64) = ref_analogenv_set_start_in_decibels;
        fn set_start_in_semitones(&mut self, v: f64) = ref_analogenv_set_start_in_semitones;
        fn set_peak_level(&mut self, v: f64) = ref_analogenv_set_peak_level;
        fn set_peak_in_decibels(&mut self, v: f64) = ref_analogenv_set_peak_in_decibels;
        fn set_peak_in_semitones(&mut self, v: f64) = ref_analogenv_set_peak_in_semitones;
        fn set_peak_level_by_vel(&mut self, v: f64) = ref_analogenv_set_peak_level_by_vel;
        fn set_peak_by_vel_in_decibels(&mut self, v: f64) = ref_analogenv_set_peak_by_vel_in_decibels;
        fn set_peak_by_vel_in_semitones(&mut self, v: f64) = ref_analogenv_set_peak_by_vel_in_semitones;
        fn set_sustain_level(&mut self, v: f64) = ref_analogenv_set_sustain_level;
        fn set_sustain_in_decibels(&mut self, v: f64) = ref_analogenv_set_sustain_in_decibels;
        fn set_sustain_in_semitones(&mut self, v: f64) = ref_analogenv_set_sustain_in_semitones;
        fn set_end_level(&mut self, v: f64) = ref_analogenv_set_end_level;
        fn set_end_in_decibels(&mut self, v: f64) = ref_analogenv_set_end_in_decibels;
        fn set_end_in_semitones(&mut self, v: f64) = ref_analogenv_set_end_in_semitones;
        fn set_attack(&mut self, v: f64) = ref_analogenv_set_attack;
        fn set_hold(&mut self, v: f64) = ref_analogenv_set_hold;
        fn set_decay(&mut self, v: f64) = ref_analogenv_set_decay;
        fn set_release(&mut self, v: f64) = ref_analogenv_set_release;
        fn set_time_scale(&mut self, v: f64) = ref_analogenv_set_time_scale;
        fn set_tau_scale(&mut self, v: f64) = ref_analogenv_set_tau_scale;
        fn set_peak_scale(&mut self, v: f64) = ref_analogenv_set_peak_scale;
        fn set_internal_state(&mut self, v: f64) = ref_analogenv_set_internal_state;
        fn get_attack(&mut self) -> f64 = ref_analogenv_get_attack;
        fn get_decay(&mut self) -> f64 = ref_analogenv_get_decay;
        fn get_sustain(&mut self) -> f64 = ref_analogenv_get_sustain;
        fn get_release(&mut self) -> f64 = ref_analogenv_get_release;
        fn is_note_on(&mut self) -> bool = ref_analogenv_is_note_on;
        fn end_is_reached(&mut self) -> bool = ref_analogenv_end_is_reached;
        fn note_on(&mut self, start_from_current_level: bool, key: i32, vel: i32) = ref_analogenv_note_on;
        fn note_off(&mut self) = ref_analogenv_note_off;
        fn reset(&mut self) = ref_analogenv_reset;
        fn get_sample(&mut self) -> f64 = ref_analogenv_get_sample;
    }
}

ref_handle! {
    /// `rosic::MipMappedWaveTable`.
    RefWaveTable { new: ref_wavetable_new, free: ref_wavetable_free;
        fn set_waveform(&mut self, v: i32) = ref_wavetable_set_waveform;
        fn set_symmetry(&mut self, v: f64) = ref_wavetable_set_symmetry;
        fn set_tanh_shaper_drive_for_303_square(&mut self, v: f64) = ref_wavetable_set_tanh_shaper_drive_for_303_square;
        fn set_tanh_shaper_offset_for_303_square(&mut self, v: f64) = ref_wavetable_set_tanh_shaper_offset_for_303_square;
        fn set_303_square_phase_shift(&mut self, v: f64) = ref_wavetable_set_303_square_phase_shift;
        fn get_tanh_shaper_drive_for_303_square(&mut self) -> f64 = ref_wavetable_get_tanh_shaper_drive_for_303_square;
        fn get_tanh_shaper_offset_for_303_square(&mut self) -> f64 = ref_wavetable_get_tanh_shaper_offset_for_303_square;
        fn get_303_square_phase_shift(&mut self) -> f64 = ref_wavetable_get_303_square_phase_shift;
        /// `getValueLinear(int integerPart, double fractionalPart, int tableIndex)`.
        fn get_value_linear_int(&mut self, integer_part: i32, fractional_part: f64, table_index: i32) -> f64 = ref_wavetable_get_value_linear_int;
        /// `getValueLinear(double phaseIndex, int tableIndex)`.
        fn get_value_linear(&mut self, phase_index: f64, table_index: i32) -> f64 = ref_wavetable_get_value_linear;
    }
}

impl RefWaveTable {
    /// Selects `waveform` and returns mip-map level `index` (`0..NUM_TABLES`)
    /// as its `TABLE_LENGTH` samples, read through `getValueLinear` with a
    /// zero fractional part. Current symmetry and 303 shaper settings apply.
    pub fn sample_table(&mut self, waveform: i32, index: i32) -> Vec<f64> {
        self.set_waveform(waveform);
        (0..TABLE_LENGTH as i32)
            .map(|i| self.get_value_linear_int(i, 0.0, index))
            .collect()
    }
}

ref_handle! {
    /// `rosic::BlendOscillator` together with the two `MipMappedWaveTable`s it
    /// borrows (owned by the handle, wired as `waveTable1` / `waveTable2`).
    RefBlendOsc { new: ref_blendosc_new, free: ref_blendosc_free;
        fn set_sample_rate(&mut self, v: f64) = ref_blendosc_set_sample_rate;
        fn set_wave_form1(&mut self, v: i32) = ref_blendosc_set_wave_form1;
        fn set_wave_form2(&mut self, v: i32) = ref_blendosc_set_wave_form2;
        fn set_start_phase(&mut self, v: f64) = ref_blendosc_set_start_phase;
        fn set_blend_factor(&mut self, v: f64) = ref_blendosc_set_blend_factor;
        fn set_frequency(&mut self, v: f64) = ref_blendosc_set_frequency;
        fn set_pulse_width(&mut self, v: f64) = ref_blendosc_set_pulse_width;
        fn set_increment(&mut self, v: f64) = ref_blendosc_set_increment;
        fn calculate_increment(&mut self) = ref_blendosc_calculate_increment;
        fn reset_phase(&mut self) = ref_blendosc_reset_phase;
        fn set_phase(&mut self, v: f64) = ref_blendosc_set_phase;
        fn get_blend_factor(&mut self) -> f64 = ref_blendosc_get_blend_factor;
        fn get_increment(&mut self) -> f64 = ref_blendosc_get_increment;
        fn get_sample(&mut self) -> f64 = ref_blendosc_get_sample;
        /// `setTanhShaperDriveFor303Square` on wavetable `which` (1 or 2).
        fn wt_set_tanh_shaper_drive(&mut self, which: i32, v: f64) = ref_blendosc_wt_set_tanh_shaper_drive;
        /// `setTanhShaperOffsetFor303Square` on wavetable `which` (1 or 2).
        fn wt_set_tanh_shaper_offset(&mut self, which: i32, v: f64) = ref_blendosc_wt_set_tanh_shaper_offset;
        /// `set303SquarePhaseShift` on wavetable `which` (1 or 2).
        fn wt_set_square_phase_shift(&mut self, which: i32, v: f64) = ref_blendosc_wt_set_square_phase_shift;
    }
}

ref_handle! {
    /// `rosic::TeeBeeFilter`.
    RefTeeBee { new: ref_teebee_new, free: ref_teebee_free;
        fn set_sample_rate(&mut self, v: f64) = ref_teebee_set_sample_rate;
        /// `setCutoff(newCutoff, updateCoefficients)`.
        fn set_cutoff(&mut self, v: f64, update_coefficients: bool) = ref_teebee_set_cutoff;
        /// `setResonance(newResonance, updateCoefficients)`.
        fn set_resonance(&mut self, v: f64, update_coefficients: bool) = ref_teebee_set_resonance;
        fn set_drive(&mut self, v: f64) = ref_teebee_set_drive;
        fn set_mode(&mut self, v: i32) = ref_teebee_set_mode;
        fn set_feedback_highpass_cutoff(&mut self, v: f64) = ref_teebee_set_feedback_highpass_cutoff;
        fn get_cutoff(&mut self) -> f64 = ref_teebee_get_cutoff;
        fn get_resonance(&mut self) -> f64 = ref_teebee_get_resonance;
        fn get_drive(&mut self) -> f64 = ref_teebee_get_drive;
        fn get_mode(&mut self) -> i32 = ref_teebee_get_mode;
        fn get_feedback_highpass_cutoff(&mut self) -> f64 = ref_teebee_get_feedback_highpass_cutoff;
        fn calculate_coefficients_exact(&mut self) = ref_teebee_calculate_coefficients_exact;
        fn calculate_coefficients_approx4(&mut self) = ref_teebee_calculate_coefficients_approx4;
        fn shape(&mut self, x: f64) -> f64 = ref_teebee_shape;
        fn reset(&mut self) = ref_teebee_reset;
        fn get_sample(&mut self, input: f64) -> f64 = ref_teebee_get_sample;
    }
}

/// Every `Open303` setter listed in `rosic_Open303.h`. Discriminants match
/// the ids in `ref_open303_set` / `ref_open303_get` in the shim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum RefParam {
    Waveform = 0,
    Tuning = 1,
    Cutoff = 2,
    Resonance = 3,
    EnvMod = 4,
    Decay = 5,
    Accent = 6,
    Volume = 7,
    AmpSustain = 8,
    TanhShaperDrive = 9,
    TanhShaperOffset = 10,
    PreFilterHighpass = 11,
    FeedbackHighpass = 12,
    PostFilterHighpass = 13,
    SquarePhaseShift = 14,
    SlideTime = 15,
    NormalAttack = 16,
    AccentAttack = 17,
    AccentDecay = 18,
    AmpDecay = 19,
    AmpRelease = 20,
}

ref_handle! {
    /// Raw `rosic::Open303` object; wrapped by [`RefOpen303`].
    RawOpen303 { new: ref_open303_new, free: ref_open303_free;
        fn set_sample_rate(&mut self, v: f64) = ref_open303_set_sample_rate;
        fn set_filter_mode(&mut self, mode: i32) = ref_open303_set_filter_mode;
        fn set_pitch_bend(&mut self, v: f64) = ref_open303_set_pitch_bend;
        fn all_notes_off(&mut self) = ref_open303_all_notes_off;
        fn set_raw(&mut self, param: i32, v: f64) = ref_open303_set;
        fn get_raw(&mut self, param: i32) -> f64 = ref_open303_get;
        fn note_on_raw(&mut self, note: i32, velocity: i32, detune: f64) = ref_open303_note_on;
        fn get_sample(&mut self) -> f64 = ref_open303_get_sample;
        fn seq_set_mode(&mut self, mode: i32) = ref_open303_seq_set_mode;
        fn seq_set_tempo(&mut self, bpm: f64) = ref_open303_seq_set_tempo;
        fn seq_set_step_length(&mut self, step_length: f64) = ref_open303_seq_set_step_length;
        fn seq_set_step_raw(&mut self, i: i32, key: i32, octave: i32, accent: bool, slide: bool, gate: bool) = ref_open303_seq_set_step;
        fn seq_set_num_steps_raw(&mut self, n: i32) = ref_open303_seq_set_num_steps;
    }
}

/// Maximum number of steps of a C++ `AcidPattern` (`maxNumSteps`).
pub const SEQ_MAX_STEPS: usize = 16;

/// The full C++ `Open303` voice, including its built-in `AcidSequencer`.
pub struct RefOpen303 {
    raw: RawOpen303,
}

impl RefOpen303 {
    /// Creates an `Open303` and calls `setSampleRate(sample_rate)`.
    pub fn new(sample_rate: f64) -> Self {
        let mut raw = RawOpen303::new();
        raw.set_sample_rate(sample_rate);
        Self { raw }
    }

    /// Calls the `Open303` setter that `param` names, with a plain (not
    /// normalized) value in the unit that setter expects.
    pub fn set(&mut self, p: RefParam, v: f64) {
        self.raw.set_raw(p as i32, v);
    }

    /// Calls the matching `Open303` getter.
    pub fn get(&mut self, p: RefParam) -> f64 {
        self.raw.get_raw(p as i32)
    }

    /// `filter.setMode(mode)` (a `TeeBeeFilter::modes` value).
    pub fn set_filter_mode(&mut self, mode: i32) {
        self.raw.set_filter_mode(mode);
    }

    /// `setPitchBend`.
    pub fn set_pitch_bend(&mut self, v: f64) {
        self.raw.set_pitch_bend(v);
    }

    /// `noteOn(note, velocity, 0.0)`; velocity 0 is a note-off.
    pub fn note_on(&mut self, note: i32, velocity: i32) {
        self.raw.note_on_raw(note, velocity, 0.0);
    }

    /// `noteOn(note, velocity, detune)`.
    pub fn note_on_detuned(&mut self, note: i32, velocity: i32, detune: f64) {
        self.raw.note_on_raw(note, velocity, detune);
    }

    /// `allNotesOff()`.
    pub fn all_notes_off(&mut self) {
        self.raw.all_notes_off();
    }

    /// `getSample()`.
    pub fn get_sample(&mut self) -> f64 {
        self.raw.get_sample()
    }

    /// Calls `getSample()` `n` times.
    pub fn render(&mut self, n: usize) -> Vec<f64> {
        (0..n).map(|_| self.get_sample()).collect()
    }

    /// `sequencer.setMode(mode)` (see [`consts::seq_mode`]).
    pub fn seq_set_mode(&mut self, mode: i32) {
        self.raw.seq_set_mode(mode);
    }

    /// `sequencer.setTempo(bpm)`.
    pub fn seq_set_bpm(&mut self, bpm: f64) {
        self.raw.seq_set_tempo(bpm);
    }

    /// `sequencer.setStepLength(step_length)` (in 16th-note steps; C++ default 0.5).
    pub fn seq_set_step_length(&mut self, step_length: f64) {
        self.raw.seq_set_step_length(step_length);
    }

    /// Sets key, octave, accent, slide and gate of step `i` of pattern 0.
    /// Steps at or beyond [`SEQ_MAX_STEPS`] are ignored.
    pub fn seq_set_step(
        &mut self,
        i: usize,
        key: i32,
        octave: i32,
        accent: bool,
        slide: bool,
        gate: bool,
    ) {
        if i < SEQ_MAX_STEPS {
            self.raw
                .seq_set_step_raw(i as i32, key, octave, accent, slide, gate);
        }
    }

    /// Sets the step count of pattern 0 (1..=[`SEQ_MAX_STEPS`]; other values
    /// are ignored). The vendored `AcidPattern` has no setter, so the shim
    /// writes its protected `numSteps` member.
    pub fn seq_set_num_steps(&mut self, n: usize) {
        if (1..=SEQ_MAX_STEPS).contains(&n) {
            self.raw.seq_set_num_steps_raw(n as i32);
        }
    }
}
