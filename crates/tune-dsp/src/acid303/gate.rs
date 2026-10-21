//! The gate, slide and tie decision of the 303 sequencer, shared by every
//! driver of an [`Acid303`] (the pattern player here, the song engine's acid
//! track).
//!
//! This is the gate half of the sequencer branch of `rosic::Open303::getSample`:
//! the `noteOffCountDown` that closes the gate, and the choice between
//! `triggerNote` and `slideToNote` that `slideToNextNote` carries from one
//! step to the next. Step timing stays with the caller.

use super::math::clip;
use super::pattern::AcidStep;
use super::voice::Acid303;

/// Gate countdown plus the pending-slide flag.
///
/// Per sample, call [`tick`](Self::tick) before the caller's step timing; on
/// each step boundary call [`on_step`](Self::on_step).
#[derive(Clone, Debug)]
pub struct AcidGate {
    /// `Open303::noteOffCountDown`: samples until the gate closes.
    note_off_count_down: i32,
    /// `Open303::slideToNextNote`.
    slide_to_next_note: bool,
}

impl Default for AcidGate {
    fn default() -> Self {
        Self::new()
    }
}

impl AcidGate {
    /// No gate pending (count down `i32::MAX`), no slide pending.
    pub fn new() -> Self {
        Self {
            note_off_count_down: i32::MAX,
            slide_to_next_note: false,
        }
    }

    /// Back to the state of [`new`](Self::new).
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Advances the gate by one sample and releases the note when the
    /// countdown reaches 0. The C++ int would overflow after ~2^31 samples of
    /// an endless tie; wrap like it does in practice.
    pub fn tick(&mut self, voice: &mut Acid303) {
        self.note_off_count_down = self.note_off_count_down.wrapping_sub(1);
        if self.note_off_count_down == 0 {
            voice.release_note();
        }
    }

    /// Plays one step: triggers, or slides when the previous step slid into
    /// this one, then arms the gate: held (`i32::MAX`) when this step slides
    /// and the next step has a note, else closing after `gate_samples`. A rest
    /// after a pending slide releases the held note.
    ///
    /// Notes above 127 are clipped (an [`AcidPattern`] never holds one).
    ///
    /// [`AcidPattern`]: super::pattern::AcidPattern
    pub fn on_step(
        &mut self,
        voice: &mut Acid303,
        step: &AcidStep,
        next_has_gate: bool,
        gate_samples: i32,
    ) {
        match step.note {
            Some(n) => {
                let key = clip(i32::from(n), 0, 127) as u8;
                if !self.slide_to_next_note {
                    voice.trigger_note(key, step.accent);
                } else {
                    voice.slide_to_note(key, step.accent);
                }

                if step.slide && next_has_gate {
                    self.note_off_count_down = i32::MAX;
                    self.slide_to_next_note = true;
                } else {
                    self.note_off_count_down = gate_samples;
                    self.slide_to_next_note = false;
                }
            }
            None if self.slide_to_next_note => {
                // Only reachable when the pattern changed under a slide (the
                // C++ would hang the note): end it here.
                voice.release_note();
                self.note_off_count_down = i32::MAX;
                self.slide_to_next_note = false;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u8, slide: bool) -> AcidStep {
        AcidStep {
            note: Some(n),
            accent: false,
            slide,
        }
    }

    #[test]
    fn gate_closes_after_the_countdown() {
        let mut v = Acid303::with_param_defaults(48_000.0);
        let mut g = AcidGate::new();
        g.on_step(&mut v, &note(36, false), false, 100);
        assert_eq!(g.note_off_count_down, 100);
        for _ in 0..99 {
            g.tick(&mut v);
        }
        assert_eq!(g.note_off_count_down, 1);
        g.tick(&mut v);
        assert_eq!(g.note_off_count_down, 0);
    }

    #[test]
    fn slide_into_a_note_holds_the_gate_and_a_rest_ends_it() {
        let mut v = Acid303::with_param_defaults(48_000.0);
        let mut g = AcidGate::new();
        g.on_step(&mut v, &note(36, true), true, 100);
        assert_eq!(g.note_off_count_down, i32::MAX);
        assert!(g.slide_to_next_note);
        // slide into a rest: the held note is released
        g.on_step(
            &mut v,
            &AcidStep {
                note: None,
                accent: false,
                slide: false,
            },
            false,
            100,
        );
        assert!(!g.slide_to_next_note);
        // a slide with no note after it does not hold
        g.on_step(&mut v, &note(36, true), false, 100);
        assert_eq!(g.note_off_count_down, 100);
        assert!(!g.slide_to_next_note);
    }
}
