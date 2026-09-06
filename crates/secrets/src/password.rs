//! Vault password generation: a draw from the selected character classes with
//! one character of each guaranteed, taken from the OS CSPRNG.

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
const SYMBOLS: &[u8] = b"!#$%&*+-=?@^_~";

/// Which characters a generated password may draw from. Lowercase letters are
/// always included, so the alphabet is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordRecipe {
    pub length: usize,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
}

impl Default for PasswordRecipe {
    fn default() -> Self {
        Self {
            length: DEFAULT_PASSWORD_LENGTH,
            uppercase: true,
            digits: true,
            symbols: true,
        }
    }
}

impl PasswordRecipe {
    /// The classes this recipe draws from, lowercase always first.
    fn classes(&self) -> Vec<&'static [u8]> {
        let mut classes = vec![LOWERCASE];
        if self.uppercase {
            classes.push(UPPERCASE);
        }
        if self.digits {
            classes.push(DIGITS);
        }
        if self.symbols {
            classes.push(SYMBOLS);
        }

        classes
    }
}

/// Generate a password. The length is clamped to [`MIN_PASSWORD_LENGTH`]
/// ..=[`MAX_PASSWORD_LENGTH`], and every selected class contributes at least
/// one character.
pub fn generate_password(recipe: &PasswordRecipe) -> Zeroizing<String> {
    let length = recipe
        .length
        .clamp(MIN_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH);
    let classes = recipe.classes();
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

    let mut out = Zeroizing::new(String::with_capacity(length));
    out.push_str(std::str::from_utf8(&chars).expect("the alphabet is ASCII"));
    out
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
                pw.bytes().any(|b| SYMBOLS.contains(&b)),
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
        assert_eq!(SYMBOLS.len(), 14);
    }

    #[test]
    fn no_character_sits_in_two_classes() {
        let all = [LOWERCASE, UPPERCASE, DIGITS, SYMBOLS].concat();
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
