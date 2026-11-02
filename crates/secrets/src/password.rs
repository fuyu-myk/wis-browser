//! Vault password generation: a draw from the selected character classes with
//! one character of each guaranteed, taken from the OS CSPRNG.

use std::num::NonZeroUsize;

use rand::seq::SliceRandom;
use rand::Rng;
use zeroize::Zeroizing;

/// The shortest password the generator will produce.
pub const MIN_PASSWORD_LENGTH: usize = 8;
/// The longest password the generator will produce.
pub const MAX_PASSWORD_LENGTH: usize = 128;
/// What the vault form starts at.
pub const DEFAULT_PASSWORD_LENGTH: usize = 20;

const LOWERCASE: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &[u8] = b"0123456789";
/// Symbols sites accept widely. Quotes, backslash, space and the shell and
/// URL metacharacters are left out.
///
/// `+ - = @` stay in even though a spreadsheet reads a cell starting with one
/// as a formula.
pub const DEFAULT_SYMBOLS: &[u8] = b"!#$%&*+-=?@^_~";

/// Which characters a generated password may draw from. Lowercase letters are
/// always included, so the alphabet is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordRecipe {
    pub length: usize,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    /// The longest run of one character a site accepts; `None` is no limit.
    pub max_consecutive: Option<NonZeroUsize>,
}

impl Default for PasswordRecipe {
    fn default() -> Self {
        Self {
            length: DEFAULT_PASSWORD_LENGTH,
            uppercase: true,
            digits: true,
            symbols: true,
            max_consecutive: None,
        }
    }
}

impl PasswordRecipe {
    /// The classes this recipe draws from, lowercase always first.
    fn classes<'a>(&self, symbols: &'a [u8]) -> Vec<&'a [u8]> {
        let mut classes: Vec<&'a [u8]> = vec![LOWERCASE];
        if self.uppercase {
            classes.push(UPPERCASE);
        }
        if self.digits {
            classes.push(DIGITS);
        }
        if self.symbols && !symbols.is_empty() {
            classes.push(symbols);
        }

        classes
    }
}

/// Generate a password. The length is clamped to [`MIN_PASSWORD_LENGTH`]
/// ..=[`MAX_PASSWORD_LENGTH`], and every selected class contributes at least
/// one character.
pub fn generate_password(recipe: &PasswordRecipe) -> Zeroizing<String> {
    generate_password_from(recipe, DEFAULT_SYMBOLS)
}

/// [`generate_password`], drawing symbols from `symbols` rather than the
/// default set. Anything but ASCII punctuation in `symbols` is ignored.
pub fn generate_password_from(recipe: &PasswordRecipe, symbols: &[u8]) -> Zeroizing<String> {
    let symbols: Vec<u8> = symbols
        .iter()
        .copied()
        .filter(u8::is_ascii_punctuation)
        .collect();
    let length = recipe
        .length
        .clamp(MIN_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH);
    let classes = recipe.classes(&symbols);
    let alphabet: Vec<u8> = classes.concat();
    let mut rng = rand::rngs::OsRng;

    // One character per class first, then the rest from the union.
    let mut chars = Zeroizing::new(Vec::<u8>::with_capacity(length));
    for class in &classes {
        chars.push(class[rng.gen_range(0..class.len())]);
    }
    while chars.len() < length {
        chars.push(alphabet[rng.gen_range(0..alphabet.len())]);
    }
    chars.as_mut_slice().shuffle(&mut rng);
    if let Some(limit) = recipe.max_consecutive {
        break_long_runs(chars.as_mut_slice(), &alphabet, limit.get(), &mut rng);
    }

    let mut out = Zeroizing::new(String::with_capacity(length));
    out.push_str(std::str::from_utf8(&chars).expect("the alphabet is ASCII"));
    out
}

/// Redraw each character that would make a run longer than `limit` from the
/// rest of the alphabet.
fn break_long_runs(chars: &mut [u8], alphabet: &[u8], limit: usize, rng: &mut impl Rng) {
    let mut run = 0;
    for index in 0..chars.len() {
        run = if index > 0 && chars[index] == chars[index - 1] {
            run + 1
        } else {
            1
        };
        if run <= limit {
            continue;
        }

        let previous = chars[index - 1];
        chars[index] = loop {
            let candidate = alphabet[rng.gen_range(0..alphabet.len())];
            if candidate != previous {
                break candidate;
            }
        };
        run = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Enough draws that a class the generator can emit will show up.
    const DRAWS: usize = 200;

    fn lowercase_only() -> PasswordRecipe {
        PasswordRecipe {
            uppercase: false,
            digits: false,
            symbols: false,
            ..PasswordRecipe::default()
        }
    }

    #[test]
    fn a_short_length_is_raised_to_the_minimum() {
        let recipe = PasswordRecipe {
            length: 1,
            ..PasswordRecipe::default()
        };
        assert_eq!(generate_password(&recipe).len(), MIN_PASSWORD_LENGTH);
    }

    #[test]
    fn a_long_length_is_capped() {
        let recipe = PasswordRecipe {
            length: 10_000,
            ..PasswordRecipe::default()
        };
        assert_eq!(generate_password(&recipe).len(), MAX_PASSWORD_LENGTH);
    }

    #[test]
    fn every_selected_class_appears_in_every_password() {
        let recipe = PasswordRecipe::default();
        for _ in 0..DRAWS {
            let pw = generate_password(&recipe);
            assert!(pw.chars().any(|c| c.is_ascii_lowercase()), "{}", *pw);
            assert!(pw.chars().any(|c| c.is_ascii_uppercase()), "{}", *pw);
            assert!(pw.chars().any(|c| c.is_ascii_digit()), "{}", *pw);
            assert!(
                pw.bytes().any(|b| DEFAULT_SYMBOLS.contains(&b)),
                "no symbol in {}",
                *pw
            );
        }
    }

    #[test]
    fn an_unselected_class_never_appears() {
        let recipe = lowercase_only();
        for _ in 0..DRAWS {
            let pw = generate_password(&recipe);
            assert!(pw.chars().all(|c| c.is_ascii_lowercase()), "{}", *pw);
        }
    }

    fn longest_run(bytes: &[u8]) -> usize {
        bytes
            .chunk_by(|a, b| a == b)
            .map(<[u8]>::len)
            .max()
            .unwrap_or(0)
    }

    fn broken(text: &[u8], limit: usize) -> Vec<u8> {
        let mut chars = text.to_vec();
        let alphabet = [LOWERCASE, b"-"].concat();
        break_long_runs(&mut chars, &alphabet, limit, &mut rand::rngs::OsRng);
        chars
    }

    #[test]
    fn a_run_past_the_limit_is_cut_to_it() {
        for _ in 0..DRAWS {
            assert_eq!(longest_run(&broken(b"aaaaaaaa", 2)), 2);
            let dashes = broken(b"--------", 1);
            assert_eq!(longest_run(&dashes), 1);
            assert!(dashes.contains(&b'-'));
        }
    }

    #[test]
    fn runs_within_the_limit_are_left_alone() {
        assert_eq!(broken(b"aabbaa--", 2), b"aabbaa--");
        assert_eq!(broken(b"abcabcab", 1), b"abcabcab");
    }

    #[test]
    fn no_run_is_longer_than_the_limit() {
        let recipe = PasswordRecipe {
            length: MAX_PASSWORD_LENGTH,
            max_consecutive: NonZeroUsize::new(1),
            ..lowercase_only()
        };
        for _ in 0..DRAWS {
            let pw = generate_password(&recipe);
            assert!(
                pw.as_bytes().windows(2).all(|pair| pair[0] != pair[1]),
                "{}",
                *pw
            );
        }
    }

    #[test]
    fn a_limit_keeps_every_selected_class() {
        let recipe = PasswordRecipe {
            length: MIN_PASSWORD_LENGTH,
            max_consecutive: NonZeroUsize::new(1),
            ..PasswordRecipe::default()
        };
        for _ in 0..DRAWS {
            let pw = generate_password_from(&recipe, b"-");
            assert!(pw.chars().any(|c| c.is_ascii_lowercase()), "{}", *pw);
            assert!(pw.chars().any(|c| c.is_ascii_uppercase()), "{}", *pw);
            assert!(pw.chars().any(|c| c.is_ascii_digit()), "{}", *pw);
            assert!(pw.contains('-'), "{}", *pw);
            assert!(
                pw.as_bytes().windows(2).all(|pair| pair[0] != pair[1]),
                "{}",
                *pw
            );
        }
    }

    #[test]
    fn the_guaranteed_characters_do_not_stay_in_class_order() {
        // Without the shuffle the first characters are one per class, in
        // class order, every single time.
        let recipe = PasswordRecipe::default();
        let ordered = (0..DRAWS)
            .filter(|_| {
                let pw = generate_password(&recipe);
                let mut chars = pw.chars();
                chars.next().is_some_and(|c| c.is_ascii_lowercase())
                    && chars.next().is_some_and(|c| c.is_ascii_uppercase())
                    && chars.next().is_some_and(|c| c.is_ascii_digit())
            })
            .count();
        assert!(
            ordered < DRAWS / 10,
            "{ordered} of {DRAWS} kept the class order; is the shuffle running?"
        );
    }

    #[test]
    fn the_class_sizes_stay_what_the_settings_hint_assumes() {
        // `PasswordsSection.tsx` mirrors these to show a recipe's entropy.
        // Changing an alphabet here means changing that hint too.
        assert_eq!(LOWERCASE.len(), 26);
        assert_eq!(UPPERCASE.len(), 26);
        assert_eq!(DIGITS.len(), 10);
        assert_eq!(DEFAULT_SYMBOLS.len(), 14);
    }

    #[test]
    fn no_character_sits_in_two_classes() {
        let all = [LOWERCASE, UPPERCASE, DIGITS, DEFAULT_SYMBOLS].concat();
        let mut unique = all.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            all.len(),
            "a character in two classes would be drawn twice as often"
        );
    }

    #[test]
    fn two_passwords_differ() {
        let recipe = PasswordRecipe::default();
        assert_ne!(*generate_password(&recipe), *generate_password(&recipe));
    }
}
