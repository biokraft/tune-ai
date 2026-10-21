//! Text notation for 303 step patterns.
//!
//! One whitespace-separated token per 16th-note step:
//!
//! - `.` is a rest.
//! - A note is a letter `A`-`G`, an optional `#` or `b`, and an octave `-1`
//!   to `9` in scientific pitch (`C4` = MIDI 60, `C2` = 36). It must land in
//!   MIDI range 0..=127 (`C-1`..=`G9`).
//! - Suffixes on a note, in any order: `!` accent, `~` slide (glide from this
//!   step into the next one; sliding into the same pitch ties the note).
//!
//! Example: `C2 . D#2! C3~ Bb1!~`. The canonical form ([`Display`]) spells
//! notes with `#` (never `b`), orders suffixes `!~` and separates steps by
//! single spaces; it parses back to an equal pattern. (An [`AcidStep`] built
//! by hand with a note above 127 prints with an octave above 9, which does
//! not parse; the player clamps such notes to 127.)
//!
//! [`Display`]: std::fmt::Display

use std::fmt;

/// Most steps a pattern may have (four bars of 16ths). The C++ `AcidPattern`
/// stops at 16; longer patterns are a tune-ai extension.
pub const MAX_STEPS: usize = 64;

/// One 16th-note step of an acid line.
#[derive(Clone, Debug, PartialEq)]
pub struct AcidStep {
    /// MIDI note number, or `None` for a rest.
    pub note: Option<u8>,
    /// Accent: harder, brighter hit.
    pub accent: bool,
    /// Slide from this step into the next one (no release in between).
    pub slide: bool,
}

/// A loop of 1..=[`MAX_STEPS`] steps.
#[derive(Clone, Debug, PartialEq)]
pub struct AcidPattern {
    pub steps: Vec<AcidStep>,
}

/// Why a pattern string did not parse.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum PatternError {
    #[error("step {index}: `{token}` is not a note or rest (expected e.g. `C2`, `D#3!~` or `.`)")]
    BadToken { index: usize, token: String },
    #[error("step {index}: a rest cannot carry an accent `!` or slide `~`")]
    ModifierOnRest { index: usize },
    #[error("a pattern needs 1 to {MAX_STEPS} steps, got {0}")]
    Length(usize),
}

impl AcidPattern {
    /// Parses the step notation described in the module docs.
    pub fn parse(s: &str) -> Result<Self, PatternError> {
        let tokens: Vec<&str> = s.split_whitespace().collect();
        if tokens.is_empty() || tokens.len() > MAX_STEPS {
            return Err(PatternError::Length(tokens.len()));
        }
        let steps = tokens
            .iter()
            .enumerate()
            .map(|(index, token)| parse_step(index, token))
            .collect::<Result<_, _>>()?;
        Ok(Self { steps })
    }
}

/// Note names of the 12 pitch classes, as `Display` spells them.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

fn parse_step(index: usize, token: &str) -> Result<AcidStep, PatternError> {
    let bad = || PatternError::BadToken {
        index,
        token: token.to_string(),
    };

    if let Some(rest) = token.strip_prefix('.') {
        return if rest.is_empty() {
            Ok(AcidStep {
                note: None,
                accent: false,
                slide: false,
            })
        } else if rest.chars().all(|c| c == '!' || c == '~') {
            Err(PatternError::ModifierOnRest { index })
        } else {
            Err(bad())
        };
    }

    let mut chars = token.chars().peekable();
    let pitch_class: i32 = match chars.next() {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(bad()),
    };
    let accidental = match chars.peek() {
        Some('#') => 1,
        Some('b') => -1,
        _ => 0,
    };
    if accidental != 0 {
        chars.next();
    }
    let octave: i32 = match chars.next() {
        Some('-') => match chars.next() {
            Some('1') => -1,
            _ => return Err(bad()),
        },
        Some(c @ '0'..='9') => c as i32 - '0' as i32,
        _ => return Err(bad()),
    };

    let (mut accent, mut slide) = (false, false);
    for c in chars {
        let flag = match c {
            '!' => &mut accent,
            '~' => &mut slide,
            _ => return Err(bad()),
        };
        if *flag {
            return Err(bad()); // repeated suffix
        }
        *flag = true;
    }

    let midi = 12 * (octave + 1) + pitch_class + accidental;
    let note = u8::try_from(midi)
        .ok()
        .filter(|&n| n <= 127)
        .ok_or_else(bad)?;
    Ok(AcidStep {
        note: Some(note),
        accent,
        slide,
    })
}

impl fmt::Display for AcidPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, step) in self.steps.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            match step.note {
                None => f.write_str(".")?,
                Some(n) => {
                    let octave = i32::from(n / 12) - 1;
                    write!(f, "{}{octave}", NAMES[usize::from(n % 12)])?;
                    if step.accent {
                        f.write_str("!")?;
                    }
                    if step.slide {
                        f.write_str("~")?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(note: Option<u8>, accent: bool, slide: bool) -> AcidStep {
        AcidStep {
            note,
            accent,
            slide,
        }
    }

    /// Every pattern used by the tests in this crate, for the round-trip check.
    const TEST_PATTERNS: &[&str] = &[
        "C2 . D#2! C3~ Bb1!~",
        ".",
        "C-1!",
        "G9~",
        "C2~ C2 . .",
        "C2! D2~ E2~ F2 . G2!~ A2 B2 C3~ C2 . . C1!~ C4~ C2 G1~",
        "C2 C2! . D#2~ F2 C3!~ C3 . G1 F2~ . F#2 . C2~ D#2! A#1",
        ". C2~ C2~ C2 G2! . C1~ C4! . D2 D2~ E2! . A#3~ G3 C2~",
        "E#2 B#1 Cb2 Fb3 Ab0 Db4",
    ];

    #[test]
    fn parses_the_example() {
        let p = AcidPattern::parse("C2 . D#2! C3~ Bb1!~").unwrap();
        assert_eq!(
            p.steps,
            vec![
                step(Some(36), false, false),
                step(None, false, false),
                step(Some(39), true, false),
                step(Some(48), false, true),
                // Bb1 = A#1 = 34 (C2 = 36). The task brief listed 46 here,
                // which is Bb2 and contradicts both C2 = 36 and the briefed
                // canonical form `A#1!~`.
                step(Some(34), true, true),
            ]
        );
    }

    #[test]
    fn displays_canonical_form() {
        let p = AcidPattern::parse("C2 . D#2! C3~ Bb1!~").unwrap();
        assert_eq!(p.to_string(), "C2 . D#2! C3~ A#1!~");
        // suffix order, flats and extra whitespace are normalised:
        let q = AcidPattern::parse("  Db3~!\t.\n  Eb-1 ").unwrap();
        assert_eq!(q.to_string(), "C#3!~ . D#-1");
    }

    #[test]
    fn round_trips_every_test_pattern() {
        for s in TEST_PATTERNS {
            let p = AcidPattern::parse(s).unwrap();
            let canon = p.to_string();
            let q = AcidPattern::parse(&canon).unwrap();
            assert_eq!(p, q, "{s} -> {canon}");
            assert_eq!(q.to_string(), canon, "{s}: canonical form is not stable");
        }
    }

    #[test]
    fn scientific_pitch_numbers() {
        let notes = |s: &str| -> Vec<Option<u8>> {
            AcidPattern::parse(s)
                .unwrap()
                .steps
                .into_iter()
                .map(|st| st.note)
                .collect()
        };
        assert_eq!(
            notes("C-1 C4 A4 G9 B3 E#2 B#1 Cb2"),
            vec![
                Some(0),
                Some(60),
                Some(69),
                Some(127),
                Some(59),
                Some(41),
                Some(36),
                Some(35)
            ]
        );
    }

    #[test]
    fn bad_tokens() {
        assert!(matches!(
            AcidPattern::parse("H2"),
            Err(PatternError::BadToken { index: 0, .. })
        ));
        for (s, index) in [
            ("C2 c2", 1),      // lower-case letter
            ("C2 . C", 2),     // no octave
            ("C10", 0),        // octave above 9
            ("C-2", 0),        // octave below -1
            ("G#9", 0),        // MIDI 128
            ("Cb-1", 0),       // MIDI -1
            ("C2!!", 0),       // repeated suffix
            ("C2~x", 0),       // junk suffix
            ("C#b2", 0),       // two accidentals
            ("C2 . .x", 2),    // junk after a rest
            ("C2 D2 E2 -", 3), // not a token at all
        ] {
            match AcidPattern::parse(s) {
                Err(PatternError::BadToken { index: i, token }) => {
                    assert_eq!(i, index, "{s}: wrong index for `{token}`");
                }
                other => panic!("{s}: expected BadToken, got {other:?}"),
            }
        }
    }

    #[test]
    fn modifier_on_rest() {
        assert_eq!(
            AcidPattern::parse(".!"),
            Err(PatternError::ModifierOnRest { index: 0 })
        );
        assert_eq!(
            AcidPattern::parse("C2 .~"),
            Err(PatternError::ModifierOnRest { index: 1 })
        );
        assert_eq!(
            AcidPattern::parse("C2 . .~!"),
            Err(PatternError::ModifierOnRest { index: 2 })
        );
    }

    #[test]
    fn length_limits() {
        assert_eq!(AcidPattern::parse(""), Err(PatternError::Length(0)));
        assert_eq!(AcidPattern::parse(" \n\t"), Err(PatternError::Length(0)));
        let tokens = |n: usize| vec!["C2"; n].join(" ");
        assert_eq!(
            AcidPattern::parse(&tokens(65)),
            Err(PatternError::Length(65))
        );
        assert_eq!(AcidPattern::parse(&tokens(64)).unwrap().steps.len(), 64);
        assert_eq!(AcidPattern::parse(&tokens(1)).unwrap().steps.len(), 1);
    }

    #[test]
    fn errors_read_clearly() {
        let e = AcidPattern::parse("C2 H2").unwrap_err();
        assert!(e.to_string().contains("step 1"), "{e}");
        assert!(e.to_string().contains("`H2`"), "{e}");
        let e = AcidPattern::parse(&vec!["."; 65].join(" ")).unwrap_err();
        assert!(e.to_string().contains("65"), "{e}");
    }
}
