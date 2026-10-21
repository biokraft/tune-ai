//! Port of `rosic_AnalogEnvelope`: an RC-style exponential envelope with
//! adjustable start, attack, peak, hold, decay, sustain, release and end.

use super::math::{db2amp, pitch_offset_to_freq_factor};

/// `rosic::AnalogEnvelope`.
#[derive(Clone, Debug)]
pub struct AnalogEnvelope {
    // level and time parameters:
    start_level: f64,
    peak_level: f64,
    sustain_level: f64,
    end_level: f64,
    attack_time: f64,
    hold_time: f64,
    decay_time: f64,
    release_time: f64,
    // unused by the C++ audio path, kept for structural parity:
    #[allow(dead_code)]
    peak_by_vel: f64,
    #[allow(dead_code)]
    peak_by_key: f64,
    #[allow(dead_code)]
    time_scale_by_vel: f64,
    #[allow(dead_code)]
    time_scale_by_key: f64,

    // accumulated time values:
    att_plus_hld: f64,
    att_plus_hld_plus_dec: f64,
    #[allow(dead_code)]
    att_plus_hld_plus_dec_plus_rel: f64,

    time: f64,
    time_scale: f64,
    increment: f64,
    tau_scale: f64,
    peak_scale: f64,

    attack_coeff: f64,
    decay_coeff: f64,
    release_coeff: f64,
    previous_output: f64,
    sample_rate: f64,
    #[allow(dead_code)]
    output_is_zero: bool,
    note_is_on: bool,
}

impl Default for AnalogEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalogEnvelope {
    pub fn new() -> Self {
        let sample_rate = 44100.0;
        let time_scale = 1.0;
        let mut s = Self {
            start_level: 0.0,
            peak_level: 1.0,
            sustain_level: 0.5,
            end_level: 0.0,
            attack_time: 0.0,
            hold_time: 0.0,
            decay_time: 0.1,
            release_time: 0.01,
            peak_by_vel: 1.0,
            peak_by_key: 1.0,
            time_scale_by_vel: 1.0,
            time_scale_by_key: 1.0,
            att_plus_hld: 0.0,
            att_plus_hld_plus_dec: 0.0,
            att_plus_hld_plus_dec_plus_rel: 0.0,
            time: 0.0,
            time_scale,
            increment: 1000.0 * time_scale / sample_rate,
            tau_scale: 1.0,
            peak_scale: 1.0,
            attack_coeff: 0.0,
            decay_coeff: 0.0,
            release_coeff: 0.0,
            previous_output: 0.0,
            sample_rate,
            output_is_zero: true,
            note_is_on: false,
        };
        // trigger the coefficient calculations:
        s.set_attack(s.attack_time);
        s.set_decay(s.decay_time);
        s.set_release(s.release_time);
        s
    }

    pub fn set_sample_rate(&mut self, new_sample_rate: f64) {
        if new_sample_rate > 0.0 {
            self.sample_rate = new_sample_rate;
        }

        // adjust time increment:
        self.increment = 1000.0 * self.time_scale / self.sample_rate;

        // re-calculate coefficients for the 3 filters:
        self.set_attack(self.attack_time);
        self.set_decay(self.decay_time);
        self.set_release(self.release_time);
    }

    pub fn set_start_level(&mut self, new_start: f64) {
        self.start_level = new_start;
    }

    pub fn set_start_in_decibels(&mut self, new_start: f64) {
        self.set_start_level(db2amp(new_start));
    }

    pub fn set_start_in_semitones(&mut self, new_start: f64) {
        self.set_start_level(pitch_offset_to_freq_factor(new_start));
    }

    pub fn set_peak_level(&mut self, new_peak: f64) {
        self.peak_level = new_peak;
    }

    pub fn set_peak_in_decibels(&mut self, new_peak: f64) {
        self.set_peak_level(db2amp(new_peak));
    }

    pub fn set_peak_in_semitones(&mut self, new_peak: f64) {
        self.set_peak_level(pitch_offset_to_freq_factor(new_peak));
    }

    pub fn set_peak_level_by_vel(&mut self, new_peak_by_vel: f64) {
        self.peak_by_vel = new_peak_by_vel;
    }

    pub fn set_peak_by_vel_in_decibels(&mut self, new_peak_by_vel: f64) {
        self.set_peak_level_by_vel(db2amp(new_peak_by_vel));
    }

    pub fn set_peak_by_vel_in_semitones(&mut self, new_peak_by_vel: f64) {
        self.set_peak_level_by_vel(pitch_offset_to_freq_factor(new_peak_by_vel));
    }

    pub fn set_sustain_level(&mut self, new_sustain: f64) {
        self.sustain_level = new_sustain;
    }

    pub fn set_sustain_in_decibels(&mut self, new_sustain: f64) {
        self.set_sustain_level(db2amp(new_sustain));
    }

    pub fn set_sustain_in_semitones(&mut self, new_sustain: f64) {
        self.set_sustain_level(pitch_offset_to_freq_factor(new_sustain));
    }

    pub fn set_end_level(&mut self, new_end: f64) {
        self.end_level = new_end;
    }

    pub fn set_end_in_decibels(&mut self, new_end: f64) {
        self.set_end_level(db2amp(new_end));
    }

    pub fn set_end_in_semitones(&mut self, new_end: f64) {
        self.set_end_level(pitch_offset_to_freq_factor(new_end));
    }

    /// Sets the length of the attack phase in milliseconds.
    pub fn set_attack(&mut self, new_attack_time: f64) {
        if new_attack_time > 0.0 {
            self.attack_time = new_attack_time;
            let tau =
                (self.sample_rate * 0.001 * self.attack_time) * self.tau_scale / self.time_scale;
            self.attack_coeff = 1.0 - (-1.0 / tau).exp();
        } else {
            self.attack_time = 0.0;
            self.attack_coeff = 1.0;
        }
        self.calculate_accumulated_times();
    }

    /// Sets the hold time in milliseconds.
    pub fn set_hold(&mut self, new_hold_time: f64) {
        if new_hold_time >= 0.0 {
            self.hold_time = new_hold_time;
        }
        self.calculate_accumulated_times();
    }

    /// Sets the length of the decay phase in milliseconds.
    pub fn set_decay(&mut self, new_decay_time: f64) {
        if new_decay_time > 0.0 {
            self.decay_time = new_decay_time;
            let tau =
                (self.sample_rate * 0.001 * self.decay_time) * self.tau_scale / self.time_scale;
            self.decay_coeff = 1.0 - (-1.0 / tau).exp();
        } else {
            self.decay_time = 0.0;
            self.decay_coeff = 1.0;
        }
        self.calculate_accumulated_times();
    }

    /// Sets the length of the release phase in milliseconds.
    pub fn set_release(&mut self, new_release_time: f64) {
        if new_release_time > 0.0 {
            self.release_time = new_release_time;
            let tau =
                (self.sample_rate * 0.001 * self.release_time) * self.tau_scale / self.time_scale;
            self.release_coeff = 1.0 - (-1.0 / tau).exp();
        } else {
            self.release_time = 0.0;
            self.release_coeff = 1.0;
        }
        self.calculate_accumulated_times();
    }

    pub fn set_time_scale(&mut self, new_time_scale: f64) {
        if new_time_scale > 0.0 {
            self.time_scale = new_time_scale;
        }

        self.increment = 1000.0 * self.time_scale / self.sample_rate;

        // re-calculate coefficients for the 3 filters:
        self.set_attack(self.attack_time);
        self.set_decay(self.decay_time);
        self.set_release(self.release_time);
    }

    /// Scales the time constants tau (to reach other values than 63.2 %).
    pub fn set_tau_scale(&mut self, new_tau_scale: f64) {
        if new_tau_scale > 0.0 {
            self.tau_scale = new_tau_scale;
        }

        self.set_attack(self.attack_time);
        self.set_decay(self.decay_time);
        self.set_release(self.release_time);
    }

    pub fn set_peak_scale(&mut self, new_peak_scale: f64) {
        if new_peak_scale > 0.0 {
            self.peak_scale = new_peak_scale;
        }
    }

    /// Sets the internal state of the RC filter.
    pub fn set_internal_state(&mut self, new_state: f64) {
        self.previous_output = new_state;
    }

    pub fn get_attack(&self) -> f64 {
        self.attack_time
    }

    pub fn get_decay(&self) -> f64 {
        self.decay_time
    }

    pub fn get_sustain(&self) -> f64 {
        self.sustain_level
    }

    pub fn get_release(&self) -> f64 {
        self.release_time
    }

    pub fn is_note_on(&self) -> bool {
        self.note_is_on
    }

    pub fn end_is_reached(&self) -> bool {
        !self.note_is_on && self.previous_output < 0.000001
    }

    #[inline]
    pub fn get_sample(&mut self) -> f64 {
        let out;

        // attack or hold phase:
        if self.time <= self.att_plus_hld {
            out = self.previous_output
                + self.attack_coeff * (self.peak_scale * self.peak_level - self.previous_output);
            self.time += self.increment;
        }
        // decay phase:
        else if self.time <= self.att_plus_hld_plus_dec {
            out = self.previous_output
                + self.decay_coeff * (self.sustain_level - self.previous_output);
            self.time += self.increment;
        }
        // sustain phase (time is not incremented):
        else if self.note_is_on {
            out = self.previous_output
                + self.decay_coeff * (self.sustain_level - self.previous_output);
        }
        // release phase:
        else {
            out =
                self.previous_output + self.release_coeff * (self.end_level - self.previous_output);
            self.time += self.increment;
        }

        // store output sample for next call:
        self.previous_output = out;

        out
    }

    /// Starts the attack phase. With `start_from_current_level` the state is
    /// not reset to the start level.
    pub fn note_on(&mut self, start_from_current_level: bool, _new_key: i32, _new_vel: i32) {
        if !start_from_current_level {
            self.previous_output = self.start_level; // may lead to clicks
        }

        // reset time for the new note:
        self.time = 0.0;
        self.note_is_on = true;
        self.output_is_zero = false;
    }

    /// Starts the release phase.
    pub fn note_off(&mut self) {
        self.note_is_on = false;

        // advance time to the beginning of the release phase:
        self.time = self.attack_time + self.hold_time + self.decay_time + self.increment;
    }

    /// Resets the time variable.
    pub fn reset(&mut self) {
        self.time = 0.0;
    }

    fn calculate_accumulated_times(&mut self) {
        self.att_plus_hld = self.attack_time + self.hold_time;
        self.att_plus_hld_plus_dec = self.att_plus_hld + self.decay_time;
        self.att_plus_hld_plus_dec_plus_rel = self.att_plus_hld_plus_dec + self.release_time;
    }
}
