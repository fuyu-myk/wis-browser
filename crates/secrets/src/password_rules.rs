//! Turning a page's `passwordrules` attribute into a generator recipe.
//!
//! The attribute is how a site tells a password manager what it will accept.
//! Only what decides a rejection is honoured: the length bounds, which classes
//! are allowed at all, and a custom symbol set. `max-consecutive` is left out.
//! Lowercase stays in regardless.

use crate::password::{
    PasswordRecipe, DEFAULT_PASSWORD_LENGTH, DEFAULT_SYMBOLS, MAX_PASSWORD_LENGTH,
    MIN_PASSWORD_LENGTH,
};

/// A recipe and the symbols it may draw from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesRecipe {
    pub recipe: PasswordRecipe,
    /// The symbol alphabet, used only when `recipe.symbols` is set.
    pub symbols: Vec<u8>,
}

/// Split on `sep`, except inside a `[...]` set, which may hold it.
fn split_outside_sets(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                parts.push(&text[start..index]);
                start = index + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);

    parts
}

#[derive(Default)]
struct Parsed {
    min_length: Option<usize>,
    max_length: Option<usize>,
    named: bool,
    upper: bool,
    digit: bool,
    special: bool,
    everything: bool,
    custom: Vec<u8>,
}

impl Parsed {
    fn class(&mut self, class: &str) {
        if let Some(set) = class
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            self.named = true;
            for byte in set.bytes().filter(u8::is_ascii_punctuation) {
                if !self.custom.contains(&byte) {
                    self.custom.push(byte);
                }
            }
            return;
        }

        match class.to_ascii_lowercase().as_str() {
            "upper" => self.upper = true,
            "lower" => {}
            "digit" => self.digit = true,
            "special" => self.special = true,
            "ascii-printable" | "unicode" => self.everything = true,
            _ => return,
        }
        self.named = true;
    }

    fn clause(&mut self, clause: &str) {
        let Some((name, value)) = clause.split_once(':') else {
            return;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "minlength" => self.min_length = value.parse().ok().or(self.min_length),
            "maxlength" => {
                if let Ok(max) = value.parse::<usize>() {
                    self.max_length = Some(self.max_length.map_or(max, |held| held.min(max)));
                }
            }
            "required" | "allowed" => {
                for class in split_outside_sets(value, ',') {
                    self.class(class.trim());
                }
            }
            _ => {}
        }
    }
}

/// The recipe a field's `passwordrules`, and its own `maxlength`, call for.
/// Rules that cannot be read leave the default recipe in place.
pub fn recipe_for_rules(rules: Option<&str>, field_max_length: Option<usize>) -> RulesRecipe {
    let mut parsed = Parsed {
        max_length: field_max_length,
        ..Parsed::default()
    };
    for clause in split_outside_sets(rules.unwrap_or_default(), ';') {
        parsed.clause(clause);
    }

    let floor = parsed.min_length.unwrap_or(0).max(MIN_PASSWORD_LENGTH);
    let ceiling = parsed
        .max_length
        .unwrap_or(MAX_PASSWORD_LENGTH)
        .clamp(MIN_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH);
    let length = if floor > ceiling {
        ceiling
    } else {
        DEFAULT_PASSWORD_LENGTH.clamp(floor, ceiling)
    };

    let default = PasswordRecipe::default();
    if !parsed.named || parsed.everything {
        return RulesRecipe {
            recipe: PasswordRecipe { length, ..default },
            symbols: DEFAULT_SYMBOLS.to_vec(),
        };
    }

    let mut symbols = if parsed.special {
        DEFAULT_SYMBOLS.to_vec()
    } else {
        Vec::new()
    };
    for byte in parsed.custom {
        if !symbols.contains(&byte) {
            symbols.push(byte);
        }
    }

    RulesRecipe {
        recipe: PasswordRecipe {
            length,
            uppercase: parsed.upper,
            digits: parsed.digit,
            symbols: !symbols.is_empty(),
        },
        symbols,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(rules: &str) -> RulesRecipe {
        recipe_for_rules(Some(rules), None)
    }

    #[test]
    fn no_rules_keeps_the_default_recipe() {
        let got = recipe_for_rules(None, None);
        assert_eq!(got.recipe, PasswordRecipe::default());
        assert_eq!(got.symbols, DEFAULT_SYMBOLS);
    }

    /// GitHub's signup field.
    #[test]
    fn a_minimum_above_the_default_raises_the_length() {
        assert_eq!(
            recipe("minlength: 15; allowed: unicode;").recipe.length,
            DEFAULT_PASSWORD_LENGTH
        );
        assert_eq!(recipe("minlength: 24;").recipe.length, 24);
    }

    #[test]
    fn a_maximum_below_the_default_lowers_the_length() {
        assert_eq!(recipe("maxlength: 16;").recipe.length, 16);
    }

    #[test]
    fn the_fields_own_maxlength_also_bounds_it() {
        assert_eq!(recipe_for_rules(None, Some(12)).recipe.length, 12);
        assert_eq!(
            recipe_for_rules(Some("maxlength: 16;"), Some(12))
                .recipe
                .length,
            12
        );
    }

    #[test]
    fn contradictory_bounds_resolve_to_the_maximum() {
        assert_eq!(recipe("minlength: 30; maxlength: 18;").recipe.length, 18);
    }

    #[test]
    fn naming_classes_drops_the_ones_left_unnamed() {
        let got = recipe("required: lower; required: upper; required: digit;").recipe;
        assert!(got.uppercase);
        assert!(got.digits);
        assert!(!got.symbols, "symbols were never allowed");
    }

    #[test]
    fn special_allows_the_default_symbols() {
        let got = recipe("required: lower; allowed: special;");
        assert!(got.recipe.symbols);
        assert_eq!(got.symbols, DEFAULT_SYMBOLS);
    }

    #[test]
    fn a_custom_set_replaces_the_symbols() {
        let got = recipe("required: upper, lower, digit; required: [-!@];");
        assert!(got.recipe.symbols);
        assert_eq!(got.symbols, b"-!@");
    }

    /// Separators inside a set belong to the set.
    #[test]
    fn a_set_may_contain_the_separators() {
        let got = recipe("allowed: lower, [;,.]; minlength: 10;");
        assert_eq!(got.symbols, b";,.");
        assert_eq!(got.recipe.length, DEFAULT_PASSWORD_LENGTH);
    }

    #[test]
    fn ascii_printable_and_unicode_allow_every_class() {
        for rules in ["allowed: ascii-printable;", "allowed: unicode;"] {
            let got = recipe(rules).recipe;
            assert!(got.uppercase && got.digits && got.symbols, "{rules}");
        }
    }

    #[test]
    fn names_are_case_insensitive_and_whitespace_tolerant() {
        let got = recipe("  REQUIRED : Upper ;Allowed:DIGIT ; MaxLength : 14 ").recipe;
        assert!(got.uppercase && got.digits && !got.symbols);
        assert_eq!(got.length, 14);
    }

    #[test]
    fn a_space_in_a_custom_set_is_not_drawn() {
        assert_eq!(recipe("required: [ !];").symbols, b"!");
    }

    /// What the rules are for: nothing the site would reject is drawn.
    #[test]
    fn a_generated_password_draws_only_the_declared_symbols() {
        let got = recipe("required: upper, lower, digit; required: [-!@];");
        for _ in 0..200 {
            let password = crate::password::generate_password_from(&got.recipe, &got.symbols);
            for byte in password.bytes().filter(u8::is_ascii_punctuation) {
                assert!(b"-!@".contains(&byte), "drew {}", byte as char);
            }
        }
    }

    #[test]
    fn a_generated_password_has_no_symbols_when_none_are_allowed() {
        let got = recipe("required: upper; required: lower; required: digit;");
        for _ in 0..200 {
            let password = crate::password::generate_password_from(&got.recipe, &got.symbols);
            assert!(
                password.bytes().all(|byte| byte.is_ascii_alphanumeric()),
                "{}",
                &*password
            );
        }
    }

    #[test]
    fn a_generated_password_respects_the_maximum() {
        let got = recipe("maxlength: 12;");
        let password = crate::password::generate_password_from(&got.recipe, &got.symbols);
        assert_eq!(password.len(), 12);
    }

    #[test]
    fn nonsense_falls_back_to_the_default() {
        let got = recipe("this is not; a: rules string");
        assert_eq!(got.recipe, PasswordRecipe::default());
    }
}
