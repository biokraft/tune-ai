// C shim over the vendored Open303 classes. Mechanical: one extern "C" function
// per C++ method, named ref_<module>_<snake_case_method>, taking an opaque
// pointer first. No exceptions cross the boundary (only the constructors can
// throw, and they are guarded; they return nullptr on failure).

#include <climits>
#include <new>

#include "rosic_AnalogEnvelope.h"
#include "rosic_BiquadFilter.h"
#include "rosic_BlendOscillator.h"
#include "rosic_DecayEnvelope.h"
#include "rosic_EllipticQuarterBandFilter.h"
#include "rosic_LeakyIntegrator.h"
#include "rosic_MipMappedWaveTable.h"
#include "rosic_OnePoleFilter.h"
#include "rosic_Open303.h"
#include "rosic_TeeBeeFilter.h"

using namespace rosic;

namespace {

// AcidPattern::numSteps is protected and has no setter; reach it through a
// pointer-to-member formed in a derived class, which is well-defined C++.
struct AcidPatternAccess : public AcidPattern {
  static int AcidPattern::*numStepsMember() { return &AcidPatternAccess::numSteps; }
};

// Open303::triggerNote / slideToNote / releaseNote are protected; reach them
// through pointers-to-member formed in a derived class, as above.
struct Open303Access : public Open303 {
  static void trigger(Open303* o, int note, bool accent) {
    (o->*(&Open303Access::triggerNote))(note, accent);
  }
  static void slide(Open303* o, int note, bool accent) {
    (o->*(&Open303Access::slideToNote))(note, accent);
  }
  static void release(Open303* o, int note) { (o->*(&Open303Access::releaseNote))(note); }
};

// BlendOscillator borrows its two wavetables, so own them next to it.
struct BlendHolder {
  MipMappedWaveTable wt1, wt2;
  BlendOscillator osc;
  BlendHolder() {
    osc.setWaveTable1(&wt1);
    osc.setWaveTable2(&wt2);
  }
};

}  // namespace

#define LIFECYCLE(mod, T)                                   \
  void* ref_##mod##_new() {                                 \
    try { return new T(); } catch (...) { return nullptr; } \
  }                                                         \
  void ref_##mod##_free(void* p) { delete static_cast<T*>(p); }

// void method(arg)
#define SET_D(mod, T, rname, expr) \
  void ref_##mod##_##rname(void* p, double v) { T* o = static_cast<T*>(p); expr; }
#define SET_I(mod, T, rname, expr) \
  void ref_##mod##_##rname(void* p, int v) { T* o = static_cast<T*>(p); expr; }
#define SET_B(mod, T, rname, expr) \
  void ref_##mod##_##rname(void* p, bool v) { T* o = static_cast<T*>(p); expr; }
#define CALL0(mod, T, rname, expr) \
  void ref_##mod##_##rname(void* p) { T* o = static_cast<T*>(p); expr; }
// getters
#define GET_D(mod, T, rname, expr) \
  double ref_##mod##_##rname(void* p) { T* o = static_cast<T*>(p); return expr; }
#define GET_I(mod, T, rname, expr) \
  int ref_##mod##_##rname(void* p) { T* o = static_cast<T*>(p); return expr; }
#define GET_B(mod, T, rname, expr) \
  bool ref_##mod##_##rname(void* p) { T* o = static_cast<T*>(p); return expr; }

extern "C" {

// ---------------------------------------------------------------- LeakyIntegrator
LIFECYCLE(leaky, LeakyIntegrator)
SET_D(leaky, LeakyIntegrator, set_sample_rate, o->setSampleRate(v))
SET_D(leaky, LeakyIntegrator, set_time_constant, o->setTimeConstant(v))
SET_D(leaky, LeakyIntegrator, set_state, o->setState(v))
GET_D(leaky, LeakyIntegrator, get_time_constant, o->getTimeConstant())
CALL0(leaky, LeakyIntegrator, reset, o->reset())
double ref_leaky_get_normalizer(double tau1, double tau2, double sample_rate) {
  return LeakyIntegrator::getNormalizer(tau1, tau2, sample_rate);
}
double ref_leaky_get_sample(void* p, double in) {
  return static_cast<LeakyIntegrator*>(p)->getSample(in);
}

// ---------------------------------------------------------------- OnePoleFilter
LIFECYCLE(onepole, OnePoleFilter)
SET_D(onepole, OnePoleFilter, set_sample_rate, o->setSampleRate(v))
SET_I(onepole, OnePoleFilter, set_mode, o->setMode(v))
SET_D(onepole, OnePoleFilter, set_cutoff, o->setCutoff(v))
SET_D(onepole, OnePoleFilter, set_lowpass_time_constant, o->setLowpassTimeConstant(v))
SET_D(onepole, OnePoleFilter, set_shelving_gain, o->setShelvingGain(v))
SET_D(onepole, OnePoleFilter, set_shelving_gain_in_decibels, o->setShelvingGainInDecibels(v))
GET_D(onepole, OnePoleFilter, get_cutoff, o->getCutoff())
CALL0(onepole, OnePoleFilter, reset, o->reset())
void ref_onepole_set_coefficients(void* p, double b0, double b1, double a1) {
  static_cast<OnePoleFilter*>(p)->setCoefficients(b0, b1, a1);
}
void ref_onepole_set_internal_state(void* p, double x1, double y1) {
  static_cast<OnePoleFilter*>(p)->setInternalState(x1, y1);
}
double ref_onepole_get_sample(void* p, double in) {
  return static_cast<OnePoleFilter*>(p)->getSample(in);
}

// ---------------------------------------------------------------- BiquadFilter
LIFECYCLE(biquad, BiquadFilter)
SET_D(biquad, BiquadFilter, set_sample_rate, o->setSampleRate(v))
SET_I(biquad, BiquadFilter, set_mode, o->setMode(v))
SET_D(biquad, BiquadFilter, set_frequency, o->setFrequency(v))
SET_D(biquad, BiquadFilter, set_gain, o->setGain(v))
SET_D(biquad, BiquadFilter, set_bandwidth, o->setBandwidth(v))
GET_I(biquad, BiquadFilter, get_mode, o->getMode())
GET_D(biquad, BiquadFilter, get_frequency, o->getFrequency())
GET_D(biquad, BiquadFilter, get_gain, o->getGain())
GET_D(biquad, BiquadFilter, get_bandwidth, o->getBandwidth())
CALL0(biquad, BiquadFilter, reset, o->reset())
double ref_biquad_get_sample(void* p, double in) {
  return static_cast<BiquadFilter*>(p)->getSample(in);
}

// ---------------------------------------------------------------- EllipticQuarterBandFilter
LIFECYCLE(ellipticqb, EllipticQuarterBandFilter)
CALL0(ellipticqb, EllipticQuarterBandFilter, reset, o->reset())
double ref_ellipticqb_get_sample(void* p, double in) {
  return static_cast<EllipticQuarterBandFilter*>(p)->getSample(in);
}

// ---------------------------------------------------------------- DecayEnvelope
LIFECYCLE(decayenv, DecayEnvelope)
SET_D(decayenv, DecayEnvelope, set_sample_rate, o->setSampleRate(v))
SET_D(decayenv, DecayEnvelope, set_decay_time_constant, o->setDecayTimeConstant(v))
SET_B(decayenv, DecayEnvelope, set_normalize_sum, o->setNormalizeSum(v))
GET_D(decayenv, DecayEnvelope, get_decay_time_constant, o->getDecayTimeConstant())
CALL0(decayenv, DecayEnvelope, trigger, o->trigger())
bool ref_decayenv_end_is_reached(void* p, double threshold) {
  return static_cast<DecayEnvelope*>(p)->endIsReached(threshold);
}
double ref_decayenv_get_sample(void* p) {
  return static_cast<DecayEnvelope*>(p)->getSample();
}

// ---------------------------------------------------------------- AnalogEnvelope
LIFECYCLE(analogenv, AnalogEnvelope)
SET_D(analogenv, AnalogEnvelope, set_sample_rate, o->setSampleRate(v))
SET_D(analogenv, AnalogEnvelope, set_start_level, o->setStartLevel(v))
SET_D(analogenv, AnalogEnvelope, set_start_in_decibels, o->setStartInDecibels(v))
SET_D(analogenv, AnalogEnvelope, set_start_in_semitones, o->setStartInSemitones(v))
SET_D(analogenv, AnalogEnvelope, set_peak_level, o->setPeakLevel(v))
SET_D(analogenv, AnalogEnvelope, set_peak_in_decibels, o->setPeakInDecibels(v))
SET_D(analogenv, AnalogEnvelope, set_peak_in_semitones, o->setPeakInSemitones(v))
SET_D(analogenv, AnalogEnvelope, set_peak_level_by_vel, o->setPeakLevelByVel(v))
SET_D(analogenv, AnalogEnvelope, set_peak_by_vel_in_decibels, o->setPeakByVelInDecibels(v))
SET_D(analogenv, AnalogEnvelope, set_peak_by_vel_in_semitones, o->setPeakByVelInSemitones(v))
SET_D(analogenv, AnalogEnvelope, set_sustain_level, o->setSustainLevel(v))
SET_D(analogenv, AnalogEnvelope, set_sustain_in_decibels, o->setSustainInDecibels(v))
SET_D(analogenv, AnalogEnvelope, set_sustain_in_semitones, o->setSustainInSemitones(v))
SET_D(analogenv, AnalogEnvelope, set_end_level, o->setEndLevel(v))
SET_D(analogenv, AnalogEnvelope, set_end_in_decibels, o->setEndInDecibels(v))
SET_D(analogenv, AnalogEnvelope, set_end_in_semitones, o->setEndInSemitones(v))
SET_D(analogenv, AnalogEnvelope, set_attack, o->setAttack(v))
SET_D(analogenv, AnalogEnvelope, set_hold, o->setHold(v))
SET_D(analogenv, AnalogEnvelope, set_decay, o->setDecay(v))
SET_D(analogenv, AnalogEnvelope, set_release, o->setRelease(v))
SET_D(analogenv, AnalogEnvelope, set_time_scale, o->setTimeScale(v))
SET_D(analogenv, AnalogEnvelope, set_tau_scale, o->setTauScale(v))
SET_D(analogenv, AnalogEnvelope, set_peak_scale, o->setPeakScale(v))
SET_D(analogenv, AnalogEnvelope, set_internal_state, o->setInternalState(v))
GET_D(analogenv, AnalogEnvelope, get_attack, o->getAttack())
GET_D(analogenv, AnalogEnvelope, get_decay, o->getDecay())
GET_D(analogenv, AnalogEnvelope, get_sustain, o->getSustain())
GET_D(analogenv, AnalogEnvelope, get_release, o->getRelease())
GET_B(analogenv, AnalogEnvelope, is_note_on, o->isNoteOn())
GET_B(analogenv, AnalogEnvelope, end_is_reached, o->endIsReached())
CALL0(analogenv, AnalogEnvelope, note_off, o->noteOff())
CALL0(analogenv, AnalogEnvelope, reset, o->reset())
void ref_analogenv_note_on(void* p, bool start_from_current_level, int key, int vel) {
  static_cast<AnalogEnvelope*>(p)->noteOn(start_from_current_level, key, vel);
}
double ref_analogenv_get_sample(void* p) {
  return static_cast<AnalogEnvelope*>(p)->getSample();
}

// ---------------------------------------------------------------- MipMappedWaveTable
LIFECYCLE(wavetable, MipMappedWaveTable)
SET_I(wavetable, MipMappedWaveTable, set_waveform, o->setWaveform(v))
SET_D(wavetable, MipMappedWaveTable, set_symmetry, o->setSymmetry(v))
SET_D(wavetable, MipMappedWaveTable, set_tanh_shaper_drive_for_303_square,
      o->setTanhShaperDriveFor303Square(v))
SET_D(wavetable, MipMappedWaveTable, set_tanh_shaper_offset_for_303_square,
      o->setTanhShaperOffsetFor303Square(v))
SET_D(wavetable, MipMappedWaveTable, set_303_square_phase_shift, o->set303SquarePhaseShift(v))
GET_D(wavetable, MipMappedWaveTable, get_tanh_shaper_drive_for_303_square,
      o->getTanhShaperDriveFor303Square())
GET_D(wavetable, MipMappedWaveTable, get_tanh_shaper_offset_for_303_square,
      o->getTanhShaperOffsetFor303Square())
GET_D(wavetable, MipMappedWaveTable, get_303_square_phase_shift, o->get303SquarePhaseShift())
// getValueLinear(int, double, int): the table values are read with frac = 0.
double ref_wavetable_get_value_linear_int(void* p, int integer_part, double fractional_part,
                                          int table_index) {
  return static_cast<MipMappedWaveTable*>(p)->getValueLinear(integer_part, fractional_part,
                                                             table_index);
}
// getValueLinear(double, int).
double ref_wavetable_get_value_linear(void* p, double phase_index, int table_index) {
  return static_cast<MipMappedWaveTable*>(p)->getValueLinear(phase_index, table_index);
}

// ---------------------------------------------------------------- BlendOscillator
LIFECYCLE(blendosc, BlendHolder)
#define BO(expr) BlendHolder* o = static_cast<BlendHolder*>(p); expr
void ref_blendosc_set_sample_rate(void* p, double v) { BO(o->osc.setSampleRate(v)); }
void ref_blendosc_set_wave_form1(void* p, int v) { BO(o->osc.setWaveForm1(v)); }
void ref_blendosc_set_wave_form2(void* p, int v) { BO(o->osc.setWaveForm2(v)); }
void ref_blendosc_set_start_phase(void* p, double v) { BO(o->osc.setStartPhase(v)); }
void ref_blendosc_set_blend_factor(void* p, double v) { BO(o->osc.setBlendFactor(v)); }
void ref_blendosc_set_frequency(void* p, double v) { BO(o->osc.setFrequency(v)); }
void ref_blendosc_set_pulse_width(void* p, double v) { BO(o->osc.setPulseWidth(v)); }
void ref_blendosc_set_increment(void* p, double v) { BO(o->osc.setIncrement(v)); }
void ref_blendosc_calculate_increment(void* p) { BO(o->osc.calculateIncrement()); }
void ref_blendosc_reset_phase(void* p) { BO(o->osc.resetPhase()); }
void ref_blendosc_set_phase(void* p, double v) { BO(o->osc.setPhase(v)); }
double ref_blendosc_get_blend_factor(void* p) { BO(return o->osc.getBlendFactor()); }
double ref_blendosc_get_increment(void* p) { BO(return o->osc.getIncrement()); }
double ref_blendosc_get_sample(void* p) { BO(return o->osc.getSample()); }
// Setters of the two owned wavetables (the square-303 shaping lives on them).
void ref_blendosc_wt_set_tanh_shaper_drive(void* p, int which, double v) {
  BO((which == 1 ? o->wt1 : o->wt2).setTanhShaperDriveFor303Square(v));
}
void ref_blendosc_wt_set_tanh_shaper_offset(void* p, int which, double v) {
  BO((which == 1 ? o->wt1 : o->wt2).setTanhShaperOffsetFor303Square(v));
}
void ref_blendosc_wt_set_square_phase_shift(void* p, int which, double v) {
  BO((which == 1 ? o->wt1 : o->wt2).set303SquarePhaseShift(v));
}
#undef BO

// ---------------------------------------------------------------- TeeBeeFilter
LIFECYCLE(teebee, TeeBeeFilter)
SET_D(teebee, TeeBeeFilter, set_sample_rate, o->setSampleRate(v))
SET_D(teebee, TeeBeeFilter, set_drive, o->setDrive(v))
SET_I(teebee, TeeBeeFilter, set_mode, o->setMode(v))
SET_D(teebee, TeeBeeFilter, set_feedback_highpass_cutoff, o->setFeedbackHighpassCutoff(v))
GET_D(teebee, TeeBeeFilter, get_cutoff, o->getCutoff())
GET_D(teebee, TeeBeeFilter, get_resonance, o->getResonance())
GET_D(teebee, TeeBeeFilter, get_drive, o->getDrive())
GET_I(teebee, TeeBeeFilter, get_mode, o->getMode())
GET_D(teebee, TeeBeeFilter, get_feedback_highpass_cutoff, o->getFeedbackHighpassCutoff())
CALL0(teebee, TeeBeeFilter, calculate_coefficients_exact, o->calculateCoefficientsExact())
CALL0(teebee, TeeBeeFilter, calculate_coefficients_approx4, o->calculateCoefficientsApprox4())
CALL0(teebee, TeeBeeFilter, reset, o->reset())
void ref_teebee_set_cutoff(void* p, double v, bool update_coefficients) {
  static_cast<TeeBeeFilter*>(p)->setCutoff(v, update_coefficients);
}
void ref_teebee_set_resonance(void* p, double v, bool update_coefficients) {
  static_cast<TeeBeeFilter*>(p)->setResonance(v, update_coefficients);
}
double ref_teebee_shape(void* p, double x) { return static_cast<TeeBeeFilter*>(p)->shape(x); }
double ref_teebee_get_sample(void* p, double in) {
  return static_cast<TeeBeeFilter*>(p)->getSample(in);
}

// ---------------------------------------------------------------- Open303
LIFECYCLE(open303, Open303)
SET_D(open303, Open303, set_sample_rate, o->setSampleRate(v))
SET_I(open303, Open303, set_filter_mode, o->filter.setMode(v))
SET_D(open303, Open303, set_pitch_bend, o->setPitchBend(v))
CALL0(open303, Open303, all_notes_off, o->allNotesOff())

// param ids must match RefParam in src/lib.rs.
void ref_open303_set(void* p, int param, double v) {
  Open303* o = static_cast<Open303*>(p);
  switch (param) {
    case 0: o->setWaveform(v); break;
    case 1: o->setTuning(v); break;
    case 2: o->setCutoff(v); break;
    case 3: o->setResonance(v); break;
    case 4: o->setEnvMod(v); break;
    case 5: o->setDecay(v); break;
    case 6: o->setAccent(v); break;
    case 7: o->setVolume(v); break;
    case 8: o->setAmpSustain(v); break;
    case 9: o->setTanhShaperDrive(v); break;
    case 10: o->setTanhShaperOffset(v); break;
    case 11: o->setPreFilterHighpass(v); break;
    case 12: o->setFeedbackHighpass(v); break;
    case 13: o->setPostFilterHighpass(v); break;
    case 14: o->setSquarePhaseShift(v); break;
    case 15: o->setSlideTime(v); break;
    case 16: o->setNormalAttack(v); break;
    case 17: o->setAccentAttack(v); break;
    case 18: o->setAccentDecay(v); break;
    case 19: o->setAmpDecay(v); break;
    case 20: o->setAmpRelease(v); break;
    default: break;
  }
}

double ref_open303_get(void* p, int param) {
  Open303* o = static_cast<Open303*>(p);
  switch (param) {
    case 0: return o->getWaveform();
    case 1: return o->getTuning();
    case 2: return o->getCutoff();
    case 3: return o->getResonance();
    case 4: return o->getEnvMod();
    case 5: return o->getDecay();
    case 6: return o->getAccent();
    case 7: return o->getVolume();
    case 8: return o->getAmpSustain();
    case 9: return o->getTanhShaperDrive();
    case 10: return o->getTanhShaperOffset();
    case 11: return o->getPreFilterHighpass();
    case 12: return o->getFeedbackHighpass();
    case 13: return o->getPostFilterHighpass();
    case 14: return o->getSquarePhaseShift();
    case 15: return o->getSlideTime();
    case 16: return o->getNormalAttack();
    case 17: return o->getAccentAttack();
    case 18: return o->getAccentDecay();
    case 19: return o->getAmpDecay();
    case 20: return o->getAmpRelease();
    default: return 0.0;
  }
}

void ref_open303_note_on(void* p, int note, int velocity, double detune) {
  static_cast<Open303*>(p)->noteOn(note, velocity, detune);
}
double ref_open303_get_sample(void* p) { return static_cast<Open303*>(p)->getSample(); }
void ref_open303_trigger_note(void* p, int note, bool accent) {
  Open303Access::trigger(static_cast<Open303*>(p), note, accent);
}
void ref_open303_slide_to_note(void* p, int note, bool accent) {
  Open303Access::slide(static_cast<Open303*>(p), note, accent);
}
void ref_open303_release_note(void* p, int note) {
  Open303Access::release(static_cast<Open303*>(p), note);
}

// Sequencer access (pattern 0 only, steps 0..15; out-of-range steps are ignored).
void ref_open303_seq_set_mode(void* p, int mode) {
  static_cast<Open303*>(p)->sequencer.setMode(mode);
}
void ref_open303_seq_set_tempo(void* p, double bpm) {
  static_cast<Open303*>(p)->sequencer.setTempo(bpm);
}
void ref_open303_seq_set_step_length(void* p, double step_length) {
  static_cast<Open303*>(p)->sequencer.setStepLength(step_length);
}
void ref_open303_seq_set_step(void* p, int i, int key, int octave, bool accent, bool slide,
                              bool gate) {
  if (i < 0 || i >= AcidPattern::getMaxNumSteps()) return;
  // AcidSequencer::setKey & co. are declared upstream but never defined, so go
  // through the pattern (the sequencer's active pattern is 0 by default).
  AcidPattern* pat = static_cast<Open303*>(p)->sequencer.getPattern(0);
  pat->setKey(i, key);
  pat->setOctave(i, octave);
  pat->setAccent(i, accent);
  pat->setSlide(i, slide);
  pat->setGate(i, gate);
}
void ref_open303_seq_set_num_steps(void* p, int n) {
  if (n < 1 || n > AcidPattern::getMaxNumSteps()) return;
  AcidPattern* pat = static_cast<Open303*>(p)->sequencer.getPattern(0);
  pat->*AcidPatternAccess::numStepsMember() = n;
}
int ref_seq_max_num_steps() { return AcidPattern::getMaxNumSteps(); }

}  // extern "C"
