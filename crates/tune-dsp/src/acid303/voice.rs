//! Port of `rosic_Open303`: the monophonic TB-303 voice, minus its internal
//! `AcidSequencer` (the `sequencer` member and the sequencer branches of
//! `noteOn` and `getSample`). A separate player drives
//! [`Acid303::trigger_note`], [`Acid303::slide_to_note`] and
//! [`Acid303::release_note`] instead.
//!
//! Faithful quirks of the C++ kept on purpose:
//! - The constructor sets `level = -12` but `ampScaler = 1`, so the voice
//!   plays at 0 dB until `set_volume` is called.
//! - The constructor sets the pitch slew limiter's time constant to 60 ms
//!   directly, while `set_slide_time(t)` sets it to `0.2 * t` (12 ms for the
//!   nominal 60 ms).
//! - `set_sample_rate` never updates the `sample_rate` member (it stays
//!   44100); it only feeds the normalizers, which are overwritten with 1.
//! - `release_note` with notes still held slides back with the untuned
//!   `pitchToFreq(currentNote)` (A4 = 440 Hz), ignoring `tuning`.
//! - The C++ leaves `BlendOscillator::blend` and `n1`/`n2` uninitialised;
//!   here they start at 0.0 and 1.0.

use super::analog_env::AnalogEnvelope;
use super::biquad::{BiquadFilter, BiquadMode};
use super::blend_osc::BlendOscillator;
use super::decay_env::DecayEnvelope;
use super::elliptic_qb::EllipticQuarterBandFilter;
use super::leaky_integrator::LeakyIntegrator;
use super::math::{
    amp2db, db2amp, exp_to_lin, lin_to_lin, pitch_offset_to_freq_factor, pitch_to_freq,
    pitch_to_freq_tuned,
};
use super::one_pole::{OnePoleFilter, OnePoleMode};
use super::teebee::{FilterMode, TeeBeeFilter};
use super::wavetable::Waveform;
use crate::param::ParamError;
use crate::param::find_spec;

/// `Open303::oversampling`.
const OVERSAMPLING: i32 = 4;

/// `rosic::MidiNoteEvent`, reduced to the members the voice reads.
/// Equality (as in the C++ `operator==`) is by key only.
#[derive(Clone, Copy, Debug)]
struct MidiNoteEvent {
    key: i32,
    vel: i32,
}

impl MidiNoteEvent {
    /// `MidiNoteEvent(initKey, initVel)`: out-of-range keys and velocities
    /// become 64.
    fn new(init_key: i32, init_vel: i32) -> Self {
        Self {
            key: if (0..=127).contains(&init_key) {
                init_key
            } else {
                64
            },
            vel: if (0..=127).contains(&init_vel) {
                init_vel
            } else {
                64
            },
        }
    }
}

/// `rosic::Open303` without its sequencer.
#[derive(Clone, Debug)]
pub struct Acid303 {
    // embedded objects (the oscillator owns `waveTable1` / `waveTable2`):
    oscillator: BlendOscillator,
    filter: TeeBeeFilter,
    amp_env: AnalogEnvelope,
    main_env: DecayEnvelope,
    pitch_slew_limiter: LeakyIntegrator,
    amp_de_clicker: BiquadFilter,
    rc1: LeakyIntegrator,
    rc2: LeakyIntegrator,
    highpass1: OnePoleFilter,
    highpass2: OnePoleFilter,
    allpass: OnePoleFilter,
    notch: BiquadFilter,
    anti_alias_filter: EllipticQuarterBandFilter,

    /// Master tuning for A4 in Hz.
    tuning: f64,
    /// Final volume as raw factor.
    amp_scaler: f64,
    /// Frequency of the oscillator (without pitchbend).
    osc_freq: f64,
    /// The (non-oversampled) sample rate; never updated after construction,
    /// as in the C++.
    sample_rate: f64,
    /// Master volume level (in dB).
    level: f64,
    /// Velocity dependence of the level (in dB); unused, as in the C++.
    #[allow(dead_code)]
    level_by_vel: f64,
    /// Scales all "byVel" parameters.
    accent: f64,
    /// Time to slide from one note to another (in ms).
    slide_time: f64,
    /// Nominal cutoff frequency of the filter.
    cutoff: f64,
    /// Strength of the envelope modulation in percent.
    env_mod: f64,
    /// Fraction of the envelope that goes upward (unmeasured mapping only).
    env_up_fraction: f64,
    /// Offset for the normalized envelope.
    env_offset: f64,
    /// Scale factor for the normalized envelope (derived from `env_mod`).
    env_scaler: f64,
    normal_attack: f64,
    accent_attack: f64,
    normal_decay: f64,
    accent_decay: f64,
    normal_amp_release: f64,
    accent_amp_release: f64,
    /// Between 0.0 and 1.0, scales the accent part of the envelopes.
    accent_gain: f64,
    /// Scale factor for the oscillator frequency from the pitch wheel.
    pitch_wheel_factor: f64,
    /// Normalizers for the RCs that are driven by the main envelope.
    n1: f64,
    n2: f64,
    /// Note which is currently played (-1 if none).
    current_note: i32,
    /// Velocity of the currently played note; written but never read, as in
    /// the C++ (its only reader was the sequencer).
    #[allow(dead_code)]
    current_vel: i32,
    /// Nothing to do in `get_sample`.
    idle: bool,

    /// `std::list<MidiNoteEvent>`; index 0 is the front.
    note_list: Vec<MidiNoteEvent>,
}

impl Acid303 {
    /// The C++ constructor followed by `setSampleRate(sample_rate)`.
    pub fn new(sample_rate: f64) -> Self {
        let mut s = Self {
            oscillator: BlendOscillator::new(),
            filter: TeeBeeFilter::new(),
            amp_env: AnalogEnvelope::new(),
            main_env: DecayEnvelope::new(),
            pitch_slew_limiter: LeakyIntegrator::new(),
            amp_de_clicker: BiquadFilter::new(),
            rc1: LeakyIntegrator::new(),
            rc2: LeakyIntegrator::new(),
            highpass1: OnePoleFilter::new(),
            highpass2: OnePoleFilter::new(),
            allpass: OnePoleFilter::new(),
            notch: BiquadFilter::new(),
            anti_alias_filter: EllipticQuarterBandFilter::new(),

            tuning: 440.0,
            amp_scaler: 1.0,
            osc_freq: 440.0,
            sample_rate: 44100.0,
            level: -12.0,
            level_by_vel: 12.0,
            accent: 0.0,
            slide_time: 60.0,
            cutoff: 1000.0,
            env_mod: 0.0, // set below by set_env_mod
            env_up_fraction: 2.0 / 3.0,
            env_offset: 0.0,
            env_scaler: 0.0,
            normal_attack: 3.0,
            accent_attack: 3.0,
            normal_decay: 1000.0,
            accent_decay: 200.0,
            normal_amp_release: 1.0,
            accent_amp_release: 50.0,
            accent_gain: 0.0,
            pitch_wheel_factor: 1.0,
            // Uninitialised in C++ until the first trigger/slide; the
            // normalizer functions always end up writing 1.0.
            n1: 1.0,
            n2: 1.0,
            current_note: -1,
            current_vel: 0,
            idle: true,
            note_list: Vec::new(),
        };

        s.set_env_mod(25.0);

        s.oscillator.set_wave_form_1(Waveform::Saw303);
        s.oscillator.set_wave_form_2(Waveform::Square303);

        s.main_env.set_normalize_sum(false);

        s.amp_env.set_attack(0.0);
        s.amp_env.set_decay(1230.0);
        s.amp_env.set_sustain_level(0.0);
        s.amp_env.set_release(0.5);
        s.amp_env.set_tau_scale(1.0);

        s.pitch_slew_limiter.set_time_constant(60.0);
        s.amp_de_clicker.set_mode(BiquadMode::Lowpass12);
        s.amp_de_clicker.set_gain(amp2db(0.5_f64.sqrt()));
        s.amp_de_clicker.set_frequency(200.0);

        s.rc1.set_time_constant(0.0);
        s.rc2.set_time_constant(15.0);

        s.highpass1.set_mode(OnePoleMode::Highpass);
        s.highpass2.set_mode(OnePoleMode::Highpass);
        s.allpass.set_mode(OnePoleMode::Allpass);
        s.notch.set_mode(BiquadMode::Bandreject);

        s.set_sample_rate(s.sample_rate);

        // tweakables:
        s.oscillator.set_pulse_width(50.0);
        s.highpass1.set_cutoff(44.486);
        s.highpass2.set_cutoff(24.167);
        s.allpass.set_cutoff(14.008);
        s.notch.set_frequency(7.5164);
        s.notch.set_bandwidth(4.7);

        s.filter.set_feedback_highpass_cutoff(150.0);

        // what `RefOpen303::new(sample_rate)` does after constructing:
        s.set_sample_rate(sample_rate);
        s
    }

    // parameter settings

    /// `setSampleRate` (in Hz). The `(float)` narrowings are the C++ ones.
    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        let narrowed = new_sample_rate as f32 as f64;
        self.main_env.set_sample_rate(new_sample_rate);
        self.amp_env.set_sample_rate(new_sample_rate);
        self.pitch_slew_limiter.set_sample_rate(narrowed);
        self.amp_de_clicker.set_sample_rate(narrowed);
        self.rc1.set_sample_rate(narrowed);
        self.rc2.set_sample_rate(narrowed);

        self.highpass2.set_sample_rate(new_sample_rate);
        self.allpass.set_sample_rate(new_sample_rate);
        self.notch.set_sample_rate(new_sample_rate);

        let oversampled = OVERSAMPLING as f64 * new_sample_rate;
        self.highpass1.set_sample_rate(oversampled);

        self.oscillator.set_sample_rate(oversampled);
        self.filter.set_sample_rate(oversampled);
    }

    /// `setWaveform`: 0 is pure saw, 1 is pure square.
    pub fn set_waveform(&mut self, new_waveform: f64) {
        self.oscillator.set_blend_factor(new_waveform);
    }

    /// `setTuning`: master tuning for A4 in Hz.
    pub fn set_tuning(&mut self, new_tuning: f64) {
        self.tuning = new_tuning;
    }

    /// `setCutoff`: nominal filter cutoff in Hz.
    pub fn set_cutoff(&mut self, new_cutoff: f64) {
        self.cutoff = new_cutoff;
        self.calculate_env_mod_scaler_and_offset();
    }

    /// `setResonance` in percent.
    pub fn set_resonance(&mut self, new_resonance: f64) {
        self.filter.set_resonance(new_resonance, true);
    }

    /// `setEnvMod`: filter envelope depth in percent.
    pub fn set_env_mod(&mut self, new_env_mod: f64) {
        self.env_mod = new_env_mod;
        self.calculate_env_mod_scaler_and_offset();
    }

    /// `setDecay`: filter envelope decay for non-accented notes, in ms.
    pub fn set_decay(&mut self, new_decay: f64) {
        self.normal_decay = new_decay;
    }

    /// `setAccent` in percent.
    pub fn set_accent(&mut self, new_accent: f64) {
        self.accent = 0.01 * new_accent;
    }

    /// `setVolume` in dB.
    pub fn set_volume(&mut self, new_level: f64) {
        self.level = new_level;
        self.amp_scaler = db2amp(self.level);
    }

    /// `setAmpSustain` in dB.
    pub fn set_amp_sustain(&mut self, new_amp_sustain: f64) {
        self.amp_env.set_sustain_in_decibels(new_amp_sustain);
    }

    /// `setTanhShaperDrive` (dB) of the 303 square table.
    pub fn set_tanh_shaper_drive(&mut self, new_drive: f64) {
        self.oscillator
            .wave_table_2_mut()
            .set_tanh_shaper_drive_for_303_square(new_drive);
    }

    /// `setTanhShaperOffset` of the 303 square table.
    pub fn set_tanh_shaper_offset(&mut self, new_offset: f64) {
        self.oscillator
            .wave_table_2_mut()
            .set_tanh_shaper_offset_for_303_square(new_offset);
    }

    /// `setPreFilterHighpass` cutoff in Hz.
    pub fn set_pre_filter_highpass(&mut self, new_cutoff: f64) {
        self.highpass1.set_cutoff(new_cutoff);
    }

    /// `setFeedbackHighpass` cutoff in Hz.
    pub fn set_feedback_highpass(&mut self, new_cutoff: f64) {
        self.filter.set_feedback_highpass_cutoff(new_cutoff);
    }

    /// `setPostFilterHighpass` cutoff in Hz.
    pub fn set_post_filter_highpass(&mut self, new_cutoff: f64) {
        self.highpass2.set_cutoff(new_cutoff);
    }

    /// `setSquarePhaseShift` in degrees.
    pub fn set_square_phase_shift(&mut self, new_shift: f64) {
        self.oscillator
            .wave_table_2_mut()
            .set_303_square_phase_shift(new_shift);
    }

    /// `setSlideTime` in ms; negative values are ignored.
    pub fn set_slide_time(&mut self, new_slide_time: f64) {
        if new_slide_time >= 0.0 {
            self.slide_time = new_slide_time;
            self.pitch_slew_limiter
                .set_time_constant((0.2 * self.slide_time) as f32 as f64);
        }
    }

    /// `setNormalAttack`: filter envelope attack for non-accented notes, ms.
    pub fn set_normal_attack(&mut self, new_normal_attack: f64) {
        self.normal_attack = new_normal_attack;
        self.rc1.set_time_constant(self.normal_attack);
    }

    /// `setAccentAttack`: filter envelope attack for accented notes, ms.
    pub fn set_accent_attack(&mut self, new_accent_attack: f64) {
        self.accent_attack = new_accent_attack;
        self.rc2.set_time_constant(self.accent_attack);
    }

    /// `setAccentDecay`: filter envelope decay for accented notes, ms.
    pub fn set_accent_decay(&mut self, new_accent_decay: f64) {
        self.accent_decay = new_accent_decay;
    }

    /// `setAmpDecay` in ms.
    pub fn set_amp_decay(&mut self, new_amp_decay: f64) {
        self.amp_env.set_decay(new_amp_decay);
    }

    /// `setAmpRelease` in ms (for non-accented notes).
    pub fn set_amp_release(&mut self, new_amp_release: f64) {
        self.normal_amp_release = new_amp_release;
        self.amp_env.set_release(new_amp_release);
    }

    /// `filter.setMode`.
    pub fn set_filter_mode(&mut self, mode: FilterMode) {
        self.filter.set_mode(mode);
    }

    /// `setPitchBend` in semitones.
    pub fn set_pitch_bend(&mut self, new_pitch_bend: f64) {
        self.pitch_wheel_factor = pitch_offset_to_freq_factor(new_pitch_bend);
    }

    /// Sets a user parameter from [`ACID303_PARAMS`] by id, in plain units.
    /// Unknown ids and values outside `min..=max` are rejected unchanged.
    ///
    /// [`ACID303_PARAMS`]: super::ACID303_PARAMS
    pub fn set_param(&mut self, id: &str, plain: f64) -> Result<(), ParamError> {
        let (index, spec) = find_spec(super::ACID303_PARAMS, id)?;
        spec.check(plain)?;
        self.set_param_at(index, plain);
        Ok(())
    }

    /// Sets the parameter at `index` of [`ACID303_PARAMS`] in plain units,
    /// clamped to its range (NaN is treated as the minimum). Infallible, for
    /// callers that resolved the index up front.
    ///
    /// # Panics
    /// If `index` is out of range of [`ACID303_PARAMS`].
    ///
    /// [`ACID303_PARAMS`]: super::ACID303_PARAMS
    pub fn set_param_at(&mut self, index: usize, plain: f64) {
        let spec = &super::ACID303_PARAMS[index];
        let plain = if plain.is_nan() {
            spec.min
        } else {
            plain.clamp(spec.min, spec.max)
        };
        match spec.id {
            "acid303.waveform" => self.set_waveform(plain),
            "acid303.tuning" => self.set_tuning(plain),
            "acid303.cutoff" => self.set_cutoff(plain),
            "acid303.resonance" => self.set_resonance(plain),
            "acid303.env_mod" => self.set_env_mod(plain),
            "acid303.decay" => self.set_decay(plain),
            "acid303.accent" => self.set_accent(plain),
            "acid303.volume" => self.set_volume(plain),
            "acid303.slide_time" => self.set_slide_time(plain),
            id => unreachable!("ACID303_PARAMS id `{id}` has no setter"),
        }
    }

    /// [`Acid303::new`] (identical to the C++ constructor) followed by
    /// `set_param(id, default)` for every entry of [`ACID303_PARAMS`], so the
    /// voice sits at its documented defaults.
    ///
    /// [`ACID303_PARAMS`]: super::ACID303_PARAMS
    pub fn with_param_defaults(sample_rate: f64) -> Self {
        let mut s = Self::new(sample_rate);
        for p in super::ACID303_PARAMS {
            s.set_param(p.id, p.default)
                .expect("ACID303_PARAMS defaults are within their own ranges");
        }
        s
    }

    // inquiry

    pub fn waveform(&self) -> f64 {
        self.oscillator.blend_factor()
    }

    pub fn tuning(&self) -> f64 {
        self.tuning
    }

    pub fn cutoff(&self) -> f64 {
        self.cutoff
    }

    pub fn resonance(&self) -> f64 {
        self.filter.get_resonance()
    }

    pub fn env_mod(&self) -> f64 {
        self.env_mod
    }

    pub fn decay(&self) -> f64 {
        self.normal_decay
    }

    pub fn accent(&self) -> f64 {
        100.0 * self.accent
    }

    pub fn volume(&self) -> f64 {
        self.level
    }

    pub fn amp_sustain(&self) -> f64 {
        amp2db(self.amp_env.get_sustain())
    }

    /// Accessor for `tanh_shaper_drive_for_303_square` of the 2nd wavetable.
    pub fn tanh_shaper_drive(&self) -> f64 {
        self.oscillator
            .wave_table_2()
            .tanh_shaper_drive_for_303_square()
    }

    pub fn tanh_shaper_offset(&self) -> f64 {
        self.oscillator
            .wave_table_2()
            .tanh_shaper_offset_for_303_square()
    }

    pub fn pre_filter_highpass(&self) -> f64 {
        self.highpass1.get_cutoff()
    }

    pub fn feedback_highpass(&self) -> f64 {
        self.filter.get_feedback_highpass_cutoff()
    }

    pub fn post_filter_highpass(&self) -> f64 {
        self.highpass2.get_cutoff()
    }

    pub fn square_phase_shift(&self) -> f64 {
        self.oscillator.wave_table_2().square_phase_shift_303()
    }

    pub fn slide_time(&self) -> f64 {
        self.slide_time
    }

    pub fn normal_attack(&self) -> f64 {
        self.normal_attack
    }

    pub fn accent_attack(&self) -> f64 {
        self.accent_attack
    }

    pub fn accent_decay(&self) -> f64 {
        self.accent_decay
    }

    pub fn amp_decay(&self) -> f64 {
        self.amp_env.get_decay()
    }

    pub fn amp_release(&self) -> f64 {
        self.normal_amp_release
    }

    // audio processing

    /// Clears all DSP state: the voice then sounds exactly like a freshly
    /// constructed one with the same parameters. Parameters (cutoff,
    /// resonance, levels, slide time, filter mode, tuning, ...) are kept.
    ///
    /// Cleared: oscillator phase, filter and highpass/allpass/notch/anti-alias
    /// memories, the amp and main envelopes, the pitch slew limiter and the
    /// two envelope RCs, the note list and current note, the accent gain, and
    /// `idle` (set true, so the next trigger also resets phase and filters).
    pub fn reset(&mut self) {
        self.oscillator.reset_phase();
        self.filter.reset();
        self.amp_env.reset_state();
        self.main_env.reset_state();
        self.pitch_slew_limiter.reset();
        self.amp_de_clicker.reset();
        self.rc1.reset();
        self.rc2.reset();
        self.highpass1.reset();
        self.highpass2.reset();
        self.allpass.reset();
        self.notch.reset();
        self.anti_alias_filter.reset();

        self.osc_freq = 440.0;
        self.accent_gain = 0.0;
        self.current_note = -1;
        self.current_vel = 0;
        self.idle = true;
        self.note_list.clear();
    }

    /// `getSample` without the sequencer branch.
    #[inline]
    pub fn get_sample(&mut self) -> f64 {
        if self.idle {
            return 0.0;
        }

        // calculate instantaneous oscillator frequency and set up the oscillator:
        let inst_freq = self.pitch_slew_limiter.get_sample(self.osc_freq);
        self.oscillator
            .set_frequency(inst_freq * self.pitch_wheel_factor);
        self.oscillator.calculate_increment();

        // calculate instantaneous cutoff frequency from the nominal cutoff and
        // all its modifiers and set up the filter:
        let main_env_out = self.main_env.get_sample();
        let mut tmp1 = self.n1 * self.rc1.get_sample(main_env_out);
        let mut tmp2 = 0.0;
        if self.accent_gain > 0.0 {
            tmp2 = main_env_out;
        }
        tmp2 = self.n2 * self.rc2.get_sample(tmp2);
        tmp1 = self.env_scaler * (tmp1 - self.env_offset);
        tmp2 *= self.accent_gain;
        // C++ `pow(2.0, tmp1+tmp2)`: clang -O2 rewrites it to `exp2` (the
        // reference object calls `_exp2`, never `_pow`), so call `exp2`
        // explicitly; `powf` would match only where LLVM does the same rewrite
        // (release builds) and differ by an ulp in debug builds.
        let inst_cutoff = self.cutoff * (tmp1 + tmp2).exp2();
        self.filter.set_cutoff(inst_cutoff, true);

        let mut amp_env_out = self.amp_env.get_sample();
        if self.amp_env.is_note_on() {
            amp_env_out += 0.45 * main_env_out + self.accent_gain * 4.0 * main_env_out;
        }
        amp_env_out = self.amp_de_clicker.get_sample(amp_env_out);

        // oversampled calculations:
        let mut tmp = 0.0;
        for _ in 1..=OVERSAMPLING {
            tmp = -self.oscillator.get_sample(); // the raw oscillator signal
            tmp = self.highpass1.get_sample(tmp); // pre-filter highpass
            tmp = self.filter.get_sample(tmp); // now it's filtered
            tmp = self.anti_alias_filter.get_sample(tmp); // anti-aliasing filtered
        }

        // these filters may operate without oversampling (they are reset in
        // trigger_note to avoid clicks):
        tmp = self.allpass.get_sample(tmp);
        tmp = self.highpass2.get_sample(tmp);
        tmp = self.notch.get_sample(tmp);
        tmp *= amp_env_out; // amplified
        tmp *= self.amp_scaler;

        self.idle = false;
        tmp
    }

    // event handling

    /// `noteOn` (MIDI path): velocity 0 is a note-off, velocity >= 100 an
    /// accent. A note-on while another note is held slides to it.
    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let note_number = i32::from(note);
        let velocity = i32::from(velocity);
        if velocity == 0 {
            // velocity zero indicates note-off events; std::list::remove
            // drops every element equal by key:
            let released_note = MidiNoteEvent::new(note_number, 0);
            self.note_list.retain(|n| n.key != released_note.key);
            match self.note_list.first() {
                None => {
                    self.current_note = -1;
                    self.current_vel = 0;
                }
                Some(front) => {
                    self.current_note = front.key;
                    self.current_vel = front.vel;
                }
            }
            self.release_note_number(note_number);
        } else {
            // an empty note list means no note is playing: trigger, else slide.
            if self.note_list.is_empty() {
                self.trigger_note_number(note_number, velocity >= 100);
            } else {
                self.slide_to_note_number(note_number, velocity >= 100);
            }

            self.current_note = note_number;
            self.current_vel = 64;

            self.note_list
                .insert(0, MidiNoteEvent::new(note_number, velocity));
        }
        self.idle = false;
    }

    /// `allNotesOff`.
    pub fn all_notes_off(&mut self) {
        self.note_list.clear();
        self.amp_env.note_off();
        self.current_note = -1;
        self.current_vel = 0;
    }

    /// Marks the voice active without playing a note: the `idle = false` that
    /// the sequencer branch of `Open303::noteOn` sets when the C++ sequencer
    /// starts. From then on the DSP chain runs through rests (the oscillator
    /// keeps its phase running), and `trigger_note` no longer resets the
    /// oscillator phase and filter states. `AcidPlayer` calls it to match the
    /// C++ sequencer.
    pub fn wake(&mut self) {
        self.idle = false;
    }

    /// `triggerNote`: starts a note, retriggering both envelopes.
    pub fn trigger_note(&mut self, note: u8, accent: bool) {
        self.trigger_note_number(i32::from(note), accent);
    }

    /// `slideToNote`: glides to a note without retriggering the envelopes.
    pub fn slide_to_note(&mut self, note: u8, accent: bool) {
        self.slide_to_note_number(i32::from(note), accent);
    }

    /// `releaseNote(currentNote)`: releases the amp envelope when no MIDI
    /// note is held, otherwise slides back to the most recent held note.
    pub fn release_note(&mut self) {
        self.release_note_number(self.current_note);
    }

    fn trigger_note_number(&mut self, note_number: i32, has_accent: bool) {
        // retrigger osc and reset filter buffers only if amplitude is near
        // zero (to avoid clicks):
        if self.idle {
            self.oscillator.reset_phase();
            self.filter.reset();
            self.highpass1.reset();
            self.highpass2.reset();
            self.allpass.reset();
            self.notch.reset();
            self.anti_alias_filter.reset();
            self.amp_de_clicker.reset();
        }

        self.apply_accent(has_accent);

        self.osc_freq = pitch_to_freq_tuned(f64::from(note_number), self.tuning);
        self.pitch_slew_limiter.set_state(self.osc_freq);
        self.main_env.trigger();
        self.amp_env.note_on(true, note_number, 64);
        self.idle = false;
    }

    fn slide_to_note_number(&mut self, note_number: i32, has_accent: bool) {
        self.osc_freq = pitch_to_freq_tuned(f64::from(note_number), self.tuning);
        self.apply_accent(has_accent);
        self.idle = false;
    }

    /// `releaseNote(noteNumber)`; the C++ ignores its argument.
    fn release_note_number(&mut self, _note_number: i32) {
        if self.note_list.is_empty() {
            self.amp_env.note_off();
        } else {
            // initiate slide back (untuned, as in the C++):
            self.osc_freq = pitch_to_freq(f64::from(self.current_note));
        }
    }

    /// The accent block shared by `triggerNote` and `slideToNote`.
    fn apply_accent(&mut self, has_accent: bool) {
        if has_accent {
            self.accent_gain = self.accent;
            self.set_main_env_decay(self.accent_decay);
            self.amp_env.set_release(self.accent_amp_release);
        } else {
            self.accent_gain = 0.0;
            self.set_main_env_decay(self.normal_decay);
            self.amp_env.set_release(self.normal_amp_release);
        }
    }

    fn set_main_env_decay(&mut self, new_decay: f64) {
        self.main_env.set_decay_time_constant(new_decay);
        self.update_normalizer1();
        self.update_normalizer2();
    }

    fn calculate_env_mod_scaler_and_offset(&mut self) {
        let use_measured_mapping = true; // might be shown as user parameter later
        if use_measured_mapping {
            // constants that arise from the measurements:
            const C0: f64 = 3.138152786059267e+002; // lowest nominal cutoff
            const C1: f64 = 2.394411986817546e+003; // highest nominal cutoff
            const O_F: f64 = 0.048292930943553; // factor in line equation for offset
            const O_C: f64 = 0.294391201442418; // constant in line equation for offset
            const S_LO_F: f64 = 3.773996325111173; // factor for scaler at low cutoff
            const S_LO_C: f64 = 0.736965594166206; // constant for scaler at low cutoff
            const S_HI_F: f64 = 4.194548788411135; // factor for scaler at high cutoff
            const S_HI_C: f64 = 0.864344900642434; // constant for scaler at high cutoff

            let e = lin_to_lin(self.env_mod, 0.0, 100.0, 0.0, 1.0);
            let c = exp_to_lin(self.cutoff, C0, C1, 0.0, 1.0);
            let s_lo = S_LO_F * e + S_LO_C;
            let s_hi = S_HI_F * e + S_HI_C;
            self.env_scaler = (1.0 - c) * s_lo + c * s_hi;
            self.env_offset = O_F * c + O_C;
        } else {
            let up_ratio = pitch_offset_to_freq_factor(self.env_up_fraction * self.env_mod);
            let down_ratio =
                pitch_offset_to_freq_factor(-(1.0 - self.env_up_fraction) * self.env_mod);
            self.env_scaler = up_ratio - down_ratio;
            self.env_offset = if self.env_scaler != 0.0 {
                -(down_ratio - 1.0) / (up_ratio - down_ratio)
            } else {
                0.0
            };
        }
    }

    fn update_normalizer1(&mut self) {
        self.n1 = LeakyIntegrator::get_normalizer(
            self.main_env.get_decay_time_constant(),
            self.rc1.get_time_constant(),
            self.sample_rate,
        );
        self.n1 = 1.0; // "test" in the C++
    }

    fn update_normalizer2(&mut self) {
        self.n2 = LeakyIntegrator::get_normalizer(
            self.main_env.get_decay_time_constant(),
            self.rc2.get_time_constant(),
            self.sample_rate,
        );
        self.n2 = 1.0; // "test" in the C++
    }
}
