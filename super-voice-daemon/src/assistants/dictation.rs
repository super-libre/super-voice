// SPDX-License-Identifier: GPL-3.0-only
//! What the user last dictated into a window with Super Voice's shortcut,
//! and whether a prompt holds it: an assistant with hooks alone learns from
//! it that a turn was spoken. There is one for the whole daemon: a dictation
//! belongs to no session, and may have gone to any window.

use parking_lot::Mutex;
use std::time::{Duration, Instant};

/// How long after a dictation a prompt that holds it counts as spoken: time
/// to look it over and send it. Kept short, as the dictation may have gone
/// to any window: a prompt typed long after is the user's own.
pub const DICTATION_FRESH: Duration = Duration::from_secs(90);

/// What the user last dictated into a window, and when.
#[derive(Default)]
pub struct Dictation {
    last: Mutex<Option<(Vec<String>, Instant)>>,
}

impl Dictation {
    /// The user dictated `text` into a window, as a prompt may be.
    pub fn remember(&self, text: &str) {
        let text = words(text);
        if !text.is_empty() {
            *self.last.lock() = Some((text, Instant::now()));
        }
    }

    /// Whether `prompt` holds the user's last dictation, made within
    /// [`DICTATION_FRESH`] of `now`, as [`holds`] says: a word or two the
    /// user corrected by hand still counts. A match uses the dictation up,
    /// so the next prompt is not spoken for it too.
    pub fn matches(&self, prompt: &str, now: Instant) -> bool {
        let prompt = words(prompt);
        let mut dictated = self.last.lock();
        let matches = dictated.as_ref().is_some_and(|(text, at)| {
            now.saturating_duration_since(*at) < DICTATION_FRESH && holds(&prompt, text)
        });
        if matches {
            *dictated = None;
        }
        matches
    }
}

/// The words of `text` as they are compared: lower case, letters and
/// digits. In a script written without spaces between words, as Chinese,
/// Japanese and Thai are, each character counts as a word, so a corrected
/// word there costs a character or two rather than its whole clause.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if is_written_without_spaces(c) {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            words.push(c.to_string());
        } else if c.is_alphanumeric() {
            word.push(c);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// Whether `c` belongs to a script written without spaces between words:
/// Thai, Lao, Burmese, Khmer, Japanese kana, and the Chinese characters
/// Chinese and Japanese share. Korean puts spaces between its words.
fn is_written_without_spaces(c: char) -> bool {
    matches!(c,
        '\u{0E00}'..='\u{0EFF}' // Thai, Lao
        | '\u{1000}'..='\u{109F}' // Burmese
        | '\u{1780}'..='\u{17FF}' // Khmer
        | '\u{3040}'..='\u{30FF}' // Hiragana, Katakana
        | '\u{31F0}'..='\u{31FF}' // Katakana for Ainu
        | '\u{3400}'..='\u{4DBF}' // Chinese characters, extension A
        | '\u{4E00}'..='\u{9FFF}' // Chinese characters
        | '\u{F900}'..='\u{FAFF}' // Chinese characters for compatibility
        | '\u{FF66}'..='\u{FF9F}' // Halfwidth Katakana
        | '\u{20000}'..='\u{3FFFF}' // Chinese characters, later extensions
    )
}

/// Whether `prompt` holds `said`, what the user said into the prompt box, as
/// [`holds`] says: a word or two corrected by hand still counts.
#[must_use]
pub fn holds_what_was_said(prompt: &str, said: &str) -> bool {
    holds(&words(prompt), &words(said))
}

/// Whether a prompt of `prompt`'s words holds a dictation of `dictated`'s:
/// all of it, in order, but for about a word in four the user corrected by
/// hand, with anything they typed around it; or the prompt is the dictation
/// cut down, keeping at least half of it. A dictation of three words or
/// fewer is matched whole: cut down, its few words are too common to tell
/// the user's typing from it.
fn holds(prompt: &[String], dictated: &[String]) -> bool {
    /// The longest dictation that must be held whole.
    const SHORT: usize = 3;
    if prompt.is_empty() || dictated.is_empty() {
        return false;
    }
    let kept = words_in_common(prompt, dictated);
    let nearly_all = |of: usize| kept >= of - of / 4;
    let cut_down =
        dictated.len() > SHORT && 2 * prompt.len() >= dictated.len() && nearly_all(prompt.len());
    nearly_all(dictated.len()) || cut_down
}

/// How many words `a` and `b` share in the same order: the length of their
/// longest common subsequence.
fn words_in_common(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0; b.len() + 1];
    for word in a {
        let mut diagonal = 0;
        for (j, other) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if word == other {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prompt that holds the user's fresh dictation was spoken, once; one
    /// typed, or sent long after, was not.
    #[test]
    fn a_prompt_holding_a_fresh_dictation_was_spoken() {
        let dictation = Dictation::default();
        let now = Instant::now();
        assert!(!dictation.matches("Open the release notes.", now));
        dictation.remember("Open the release notes");
        assert!(dictation.matches("  open the RELEASE notes! ", now));
        assert!(
            !dictation.matches("Open the release notes.", now),
            "used up"
        );

        dictation.remember("open the notes");
        assert!(!dictation.matches("something else", now));
        assert!(!dictation.matches(
            "open the notes",
            now + DICTATION_FRESH + Duration::from_secs(1)
        ));
        // A minute to look it over is enough.
        dictation.remember("open the notes");
        assert!(dictation.matches("open the notes", now + Duration::from_secs(60)));
        dictation.remember("   ");
        assert!(!dictation.matches("", now));
    }

    /// A dictation the user touched up by hand is still theirs: a word or
    /// two corrected, text typed around it, or the end cut off. A prompt that
    /// only shares a few of its words, or a short one, is not.
    #[test]
    fn a_dictation_corrected_by_hand_still_counts() {
        let dictation = "Rename the super boys plugin folder to super voice please";
        let spoken = |prompt: &str| {
            let memory = Dictation::default();
            memory.remember(dictation);
            memory.matches(prompt, Instant::now())
        };
        // Ten words: two may be corrected.
        assert!(spoken(
            "Rename the Super Voice plugin folder to super-voice, please."
        ));
        assert!(spoken(
            "In this repo: rename the super boys plugin folder to super voice please. Thanks!"
        ));
        assert!(spoken("Rename the super boys plugin folder to super voice"));
        assert!(!spoken(
            "Rename the voice plugin directory to voice, thanks"
        ));
        assert!(!spoken("please"));
        assert!(!spoken("Delete the plugin folder"));

        // Chinese and Japanese are written without spaces: a corrected word
        // costs its characters, not the whole clause.
        let said = |dictation: &str, prompt: &str| {
            let memory = Dictation::default();
            memory.remember(dictation);
            memory.matches(prompt, Instant::now())
        };
        let misheard = "把插件文件夹重命名为超级鱼音";
        assert!(said(misheard, "把插件文件夹重命名为超级语音"));
        assert!(said(misheard, "请把插件文件夹重命名为超级鱼音谢谢"));
        assert!(!said(misheard, "删除插件文件夹"));
        assert!(said(
            "スーパーボーイズの設定を開いて",
            "スーパーボイスの設定を開いて"
        ));
        assert_eq!(
            words("打开 README 文件。"),
            ["打", "开", "readme", "文", "件"]
        );

        // Three words or fewer: whole, or not at all. Cut down, they would
        // be a word or two the user may well have typed.
        let dictation = Dictation::default();
        dictation.remember("Open the notes");
        assert!(!dictation.matches("Open my notes", Instant::now()));
        assert!(!dictation.matches("open notes", Instant::now()));
        assert!(dictation.matches("open the notes!", Instant::now()));
        assert!(!said("Yes please", "yes"));
        assert!(said("Yes please", "Yes, please."));
    }
}
