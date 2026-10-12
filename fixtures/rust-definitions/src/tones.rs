use crate::shelf::Tone;

/// The tone a new widget gets.
pub fn default_tone() -> Tone {
    Tone::Slate
}

/// How many tones there are.
pub const TONE_COUNT: usize = 2;
