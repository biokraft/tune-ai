//! Plays an [`AcidPattern`] on an [`Acid303`] voice.
//!
//! A port of `rosic::AcidSequencer::getNote` (16th-note step timing with
//! `roundToInt` and drift compensation) together with the sequencer branch of
//! `rosic::Open303::getSample` (gate countdown, trigger vs. slide). The C++
//! pattern stores keys relative to a held MIDI note; here every step carries
//! its absolute note, so the player calls `trigger_note` / `slide_to_note` /
//! `release_note` directly.

use super::gate::AcidGate;
use super::math::{beats_to_seconds, round_to_int};
use super::pattern::AcidPattern;
use super::voice::Acid303;

/// Gate length in steps (`AcidPattern::stepLength` defaults to 0.5).
const STEP_LENGTH: f64 = 0.5;

/// Step sequencer driving an [`Acid303`].
///
/// Call [`tick`](Self::tick) exactly once before each `voice.get_sample()`.
/// The first tick plays step 0.
pub struct AcidPlayer {
    pattern: AcidPattern,
    /// Set by [`set_pattern`](Self::set_pattern), swapped in at the next step.
    pending: Option<AcidPattern>,
    bpm: f64,
    sample_rate: f64,
    /// `AcidSequencer::step`: index of the next step to play.
    step: usize,
    /// `AcidSequencer::countDown`: samples until the next step.
    count_down: i32,
    /// `AcidSequencer::driftError`.
    drift_error: f64,
    /// Gate countdown and pending slide (`noteOffCountDown`, `slideToNextNote`).
    gate: AcidGate,
    /// Whether the first tick has started the voice.
    started: bool,
}

impl AcidPlayer {
    /// A player at step 0, in the state `AcidSequencer::start` and the
    /// sequencer branch of `Open303::noteOn` leave.
    pub fn new(pattern: AcidPattern, bpm: f64, sample_rate: f64) -> Self {
        Self {
            pattern,
            pending: None,
            bpm,
            sample_rate,
            step: 0,
            count_down: -1,
            drift_error: 0.0,
            gate: AcidGate::new(),
            started: false,
        }
    }

    /// Replaces the pattern from the next step boundary on, keeping the step
    /// position (wrapped to the new length).
    pub fn set_pattern(&mut self, pattern: AcidPattern) {
        self.pending = Some(pattern);
    }

    /// `AcidSequencer::getStepLengthInSamples`: the gate length.
    fn step_length_in_samples(&self) -> i32 {
        round_to_int(self.sample_rate * STEP_LENGTH * beats_to_seconds(0.25, self.bpm))
    }

    /// Advances one sample: closes the gate when it runs out and plays the
    /// next step when it is due.
    pub fn tick(&mut self, voice: &mut Acid303) {
        if !self.started {
            // Open303::noteOn, sequencer branch, on the note that starts the
            // C++ sequencer: `allNotesOff()` (the mode has just changed),
            // which moves the amp envelope out of its initial attack phase,
            // then `idle = false`, so the DSP chain runs through leading
            // rests exactly like the C++.
            voice.all_notes_off();
            voice.wake();
            self.started = true;
        }

        // Open303::getSample, sequencer branch: close the gate when due.
        self.gate.tick(voice);

        // AcidSequencer::getNote:
        if self.count_down > 0 {
            self.count_down -= 1;
            return;
        }
        let seconds_to_next_step = beats_to_seconds(0.25, self.bpm);
        let samples_to_next_step = seconds_to_next_step * self.sample_rate;
        self.count_down = round_to_int(samples_to_next_step);

        // keep track of accumulating error due to rounding and compensate
        // when the accumulated error exceeds half a sample:
        self.drift_error += f64::from(self.count_down) - samples_to_next_step;
        if self.drift_error < -0.5 {
            // negative errors indicate that we are too early
            self.drift_error += 1.0;
            self.count_down += 1;
        } else if self.drift_error >= 0.5 {
            self.drift_error -= 1.0;
            self.count_down -= 1;
        }

        if let Some(p) = self.pending.take() {
            self.pattern = p;
            self.step %= self.pattern.steps().len();
        }
        let len = self.pattern.steps().len();
        let note = &self.pattern.steps()[self.step];
        self.step = (self.step + 1) % len;
        // AcidSequencer::getNextScheduledNote:
        let next_has_gate = self.pattern.steps()[self.step].note.is_some();

        // back in Open303::getSample: trigger or slide, and arm the gate.
        let gate_samples = self.step_length_in_samples();
        self.gate.on_step(voice, note, next_has_gate, gate_samples);
    }
}
