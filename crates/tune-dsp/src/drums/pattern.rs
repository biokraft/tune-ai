//! Drum step notation: `x` hit, `X` accented hit, `.` rest, one 16th per char.

use std::fmt;

/// One 16th step of a drum pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumHit {
    Rest,
    Hit,
    Accent,
}

/// Why a drum pattern was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DrumPatternError {
    /// `index` is the step index of the offending character (whitespace excluded).
    #[error("invalid drum step `{ch}` at step {index} (use x, X or .)")]
    BadChar { index: usize, ch: char },
    #[error("drum pattern has {0} steps, expected 1..=64")]
    Length(usize),
}

/// A validated drum pattern of 1..=64 steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrumPattern {
    hits: Vec<DrumHit>,
}

impl DrumPattern {
    pub const MAX_STEPS: usize = 64;

    pub fn new(hits: Vec<DrumHit>) -> Result<Self, DrumPatternError> {
        if hits.is_empty() || hits.len() > Self::MAX_STEPS {
            return Err(DrumPatternError::Length(hits.len()));
        }
        Ok(Self { hits })
    }

    /// Parses `x`/`X`/`.` notation; whitespace is ignored.
    pub fn parse(s: &str) -> Result<Self, DrumPatternError> {
        let mut hits = Vec::new();
        for ch in s.chars().filter(|c| !c.is_whitespace()) {
            hits.push(match ch {
                'x' => DrumHit::Hit,
                'X' => DrumHit::Accent,
                '.' => DrumHit::Rest,
                ch => {
                    return Err(DrumPatternError::BadChar {
                        index: hits.len(),
                        ch,
                    });
                }
            });
        }
        Self::new(hits)
    }

    pub fn hits(&self) -> &[DrumHit] {
        &self.hits
    }
}

impl fmt::Display for DrumPattern {
    /// Canonical form: groups of 4 steps separated by one space.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, h) in self.hits.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                f.write_str(" ")?;
            }
            f.write_str(match h {
                DrumHit::Rest => ".",
                DrumHit::Hit => "x",
                DrumHit::Accent => "X",
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays() {
        let p = DrumPattern::parse("x...x...X... x...").unwrap();
        assert_eq!(p.hits().len(), 16);
        assert_eq!(p.hits()[0], DrumHit::Hit);
        assert_eq!(p.hits()[1], DrumHit::Rest);
        assert_eq!(p.hits()[8], DrumHit::Accent);
        assert_eq!(p.to_string(), "x... x... X... x...");
        assert_eq!(DrumPattern::parse(&p.to_string()).unwrap(), p);
    }

    #[test]
    fn rejects_bad_char_and_length() {
        assert_eq!(
            DrumPattern::parse("x.o."),
            Err(DrumPatternError::BadChar { index: 2, ch: 'o' })
        );
        assert_eq!(DrumPattern::parse(""), Err(DrumPatternError::Length(0)));
        assert_eq!(
            DrumPattern::parse(&"x".repeat(65)),
            Err(DrumPatternError::Length(65))
        );
        assert!(DrumPattern::parse(&"x".repeat(64)).is_ok());
    }

    #[test]
    fn bad_char_index_ignores_whitespace() {
        assert_eq!(
            DrumPattern::parse("x. .o"),
            Err(DrumPatternError::BadChar { index: 3, ch: 'o' })
        );
    }
}
