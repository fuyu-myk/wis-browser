//! A postal address with contact details, as kept in the password vault for
//! filling forms.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{WisError, WisResult};

pub const MAX_FIELD_CHARS: usize = 200;
pub const MAX_STREET_CHARS: usize = 1000;
pub const MAX_ADDRESSES: usize = 500;

#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Address {
    pub given_name: String,
    pub family_name: String,
    pub organization: String,
    /// Street lines, separated by `\n`.
    pub street_address: String,
    pub city: String,
    /// State, province, county or region.
    pub region: String,
    pub postal_code: String,
    /// ISO 3166-1 alpha-2, uppercase, or empty.
    pub country: String,
    pub tel: String,
    pub email: String,
}

impl fmt::Debug for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Address { .. }")
    }
}

impl Address {
    /// Trimmed, free of control characters, with a two-letter uppercase
    /// country and within the size limits. An address with nothing in it is
    /// refused.
    pub fn tidied(self) -> WisResult<Address> {
        let oversized = [
            &self.given_name,
            &self.family_name,
            &self.organization,
            &self.city,
            &self.region,
            &self.postal_code,
            &self.country,
            &self.tel,
            &self.email,
        ]
        .iter()
        .any(|field| field.trim().len() > 4 * MAX_FIELD_CHARS)
            || self.street_address.trim().len() > 4 * MAX_STREET_CHARS;
        if oversized {
            return Err(invalid("An address field is too long"));
        }

        let line = |text: String| -> WisResult<String> {
            let kept = printable(text.trim());
            if kept.chars().count() > MAX_FIELD_CHARS {
                return Err(invalid("An address field is too long"));
            }
            Ok(kept)
        };
        let street = self
            .street_address
            .replace(['\u{2028}', '\u{2029}', '\r'], "\n")
            .lines()
            .map(|part| printable(part.trim()))
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if street.chars().count() > MAX_STREET_CHARS {
            return Err(invalid("The street address is too long"));
        }

        let country = self.country.trim().to_ascii_uppercase();
        if !country.is_empty()
            && !(country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(invalid("The country must be a two-letter code"));
        }

        let tidied = Address {
            given_name: line(self.given_name)?,
            family_name: line(self.family_name)?,
            organization: line(self.organization)?,
            street_address: street,
            city: line(self.city)?,
            region: line(self.region)?,
            postal_code: line(self.postal_code)?,
            country,
            tel: line(self.tel)?,
            email: line(self.email)?,
        };
        if tidied.is_empty() {
            return Err(invalid("An address needs at least one field"));
        }

        Ok(tidied)
    }

    pub fn is_empty(&self) -> bool {
        [
            &self.given_name,
            &self.family_name,
            &self.organization,
            &self.street_address,
            &self.city,
            &self.region,
            &self.postal_code,
            &self.country,
            &self.tel,
            &self.email,
        ]
        .iter()
        .all(|field| field.is_empty())
    }

    /// Whether this address holds every value `other` has. Case and punctuation,
    /// spacing in a postal code, nor how a phone number is written count.
    pub fn covers(&self, other: &Address) -> bool {
        let words = |mine: &str, theirs: &str| theirs.is_empty() || same_words(mine, theirs);
        let compact = |mine: &str, theirs: &str| {
            theirs.is_empty() || alphanumeric(mine).eq(alphanumeric(theirs))
        };
        words(&self.given_name, &other.given_name)
            && words(&self.family_name, &other.family_name)
            && words(&self.organization, &other.organization)
            && words(&self.street_address, &other.street_address)
            && words(&self.city, &other.city)
            && words(&self.region, &other.region)
            && compact(&self.postal_code, &other.postal_code)
            && compact(&self.country, &other.country)
            && (other.tel.is_empty() || same_phone(&self.tel, &other.tel))
            && words(&self.email, &other.email)
    }

    /// Whether `other` contains the same street, and the same postal code,
    /// or the same city when either lacks a postal code.
    pub fn same_place(&self, other: &Address) -> bool {
        let filled = |a: &str, b: &str| !a.is_empty() && !b.is_empty();
        let placed = if filled(&self.postal_code, &other.postal_code) {
            alphanumeric(&self.postal_code).eq(alphanumeric(&other.postal_code))
        } else {
            filled(&self.city, &other.city) && same_words(&self.city, &other.city)
        };
        filled(&self.street_address, &other.street_address)
            && same_words(&self.street_address, &other.street_address)
            && placed
    }

    /// This address with every value `other` has that this one lacks or holds
    /// differently. A value only written differently stays as saved.
    pub fn updated_by(&self, other: &Address) -> Address {
        let pick = |mine: &str, theirs: &str, same: bool| {
            if theirs.is_empty() || same {
                mine.to_string()
            } else {
                theirs.to_string()
            }
        };
        let words = |mine: &str, theirs: &str| pick(mine, theirs, same_words(mine, theirs));
        let compact = |mine: &str, theirs: &str| {
            pick(mine, theirs, alphanumeric(mine).eq(alphanumeric(theirs)))
        };
        Address {
            given_name: words(&self.given_name, &other.given_name),
            family_name: words(&self.family_name, &other.family_name),
            organization: words(&self.organization, &other.organization),
            street_address: words(&self.street_address, &other.street_address),
            city: words(&self.city, &other.city),
            region: words(&self.region, &other.region),
            postal_code: compact(&self.postal_code, &other.postal_code),
            country: compact(&self.country, &other.country),
            tel: pick(&self.tel, &other.tel, same_phone(&self.tel, &other.tel)),
            email: words(&self.email, &other.email),
        }
    }

    /// Whether `name`, written whole, is this address's given and family name,
    /// in either order, with or without a space between.
    pub fn named(&self, name: &str) -> bool {
        let (given, family) = (&self.given_name, &self.family_name);
        [format!("{given} {family}"), format!("{family} {given}")]
            .iter()
            .any(|whole| same_words(whole, name) || alphanumeric(whole).eq(alphanumeric(name)))
    }
}

fn alphanumeric(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
}

/// The same words, ignoring case and what lies between them.
fn same_words(a: &str, b: &str) -> bool {
    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    words(a) == words(b)
}

/// The same number, written with or without the country code or the trunk
/// prefix: the shorter, less a leading 0, ends the longer, if it holds seven
/// digits or more. Full-width digits count; without digits, the words must
/// match.
fn same_phone(a: &str, b: &str) -> bool {
    let digits = |text: &str| -> String {
        text.chars()
            .filter_map(|c| match c {
                '0'..='9' => Some(c),
                '\u{ff10}'..='\u{ff19}' => char::from_u32(c as u32 - 0xff10 + u32::from(b'0')),
                _ => None,
            })
            .collect()
    };
    let (a_digits, b_digits) = (digits(a), digits(b));
    if b_digits.is_empty() {
        return same_words(a, b);
    }

    let (short, long) = if a_digits.len() <= b_digits.len() {
        (a_digits, b_digits)
    } else {
        (b_digits, a_digits)
    };
    if short == long {
        return true;
    }

    let short = short.strip_prefix('0').unwrap_or(&short);
    short.len() >= 7 && long.ends_with(short)
}

pub(crate) fn printable(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() && !invisible(*c))
        .collect()
}

/// Characters that change how text displays without showing and spell
/// nothing. This includes the zero-width space, direction overrides and
/// isolates, line separators and the byte-order mark. Joiners and direction
/// marks are excluded.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
    )
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> Address {
        Address {
            given_name: " Ada ".into(),
            family_name: "Lovelace".into(),
            street_address: "  12 St James's Square \r\n\r\n  Flat 3 ".into(),
            city: "London".into(),
            postal_code: "SW1Y 4JH".into(),
            country: "gb".into(),
            email: "ada@example.org".into(),
            ..Address::default()
        }
    }

    #[test]
    fn an_address_is_tidied_field_by_field() {
        let tidied = home().tidied().unwrap();
        assert_eq!(tidied.given_name, "Ada");
        assert_eq!(tidied.street_address, "12 St James's Square\nFlat 3");
        assert_eq!(tidied.country, "GB");

        let odd = Address {
            given_name: "A\u{0}da\u{7}".into(),
            ..Address::default()
        };
        assert_eq!(odd.tidied().unwrap().given_name, "Ada");
    }

    #[test]
    fn an_empty_oversized_or_misplaced_address_is_refused() {
        assert!(Address::default().tidied().is_err());
        let blank = Address {
            city: "   ".into(),
            ..Address::default()
        };
        assert!(blank.tidied().is_err());

        for country in ["GBR", "1A", "ü"] {
            let wrong = Address {
                country: country.into(),
                ..home()
            };
            assert!(wrong.tidied().is_err(), "{country}");
        }

        let long_name = Address {
            given_name: "a".repeat(MAX_FIELD_CHARS + 1),
            ..home()
        };
        assert!(long_name.tidied().is_err());
        let long_street = Address {
            street_address: "a".repeat(MAX_STREET_CHARS + 1),
            ..home()
        };
        assert!(long_street.tidied().is_err());
    }

    #[test]
    fn invisible_and_direction_marks_are_dropped_and_separators_break_lines() {
        let sneaky = Address {
            given_name: "A\u{202e}d\u{200b}a\u{2028}".into(),
            street_address: "1 Main St\u{2028}Unit 2\rRear".into(),
            ..Address::default()
        }
        .tidied()
        .unwrap();
        assert_eq!(sneaky.given_name, "Ada");
        assert_eq!(sneaky.street_address, "1 Main St\nUnit 2\nRear");
    }

    #[test]
    fn joiners_and_direction_marks_that_spell_real_text_are_kept() {
        for text in [
            "\u{0dc1}\u{0dca}\u{200d}\u{0dbb}\u{0dd3}",
            "\u{06a9}\u{0648}\u{0686}\u{0647}\u{200c}\u{06cc}",
            "\u{05e8}\u{05d7}\u{05d5}\u{05d1}\u{200f} 12",
            "\u{0634}\u{0627}\u{0631}\u{0639}\u{061c} 5",
        ] {
            let kept = Address {
                street_address: text.into(),
                ..Address::default()
            }
            .tidied()
            .unwrap();
            assert_eq!(kept.street_address, text);
        }
    }

    #[test]
    fn surrounding_space_does_not_count_against_a_full_field() {
        let full = Address {
            given_name: format!("{} ", "\u{1f600}".repeat(MAX_FIELD_CHARS)),
            ..Address::default()
        };
        assert!(full.tidied().is_ok());
    }

    #[test]
    fn the_limits_count_characters_and_refuse_with_a_clear_error() {
        let at_limit = Address {
            given_name: "\u{e9}".repeat(MAX_FIELD_CHARS),
            street_address: "a".repeat(MAX_STREET_CHARS),
            ..Address::default()
        };
        assert!(at_limit.tidied().is_ok());

        let padded = Address {
            given_name: format!("{}a", "\u{0}".repeat(5 * MAX_FIELD_CHARS)),
            ..Address::default()
        };
        assert!(matches!(
            padded.tidied(),
            Err(WisError::InvalidInput { .. })
        ));
    }

    #[test]
    fn debug_output_keeps_the_address_private() {
        let shown = format!("{:?}", home());
        assert!(!shown.contains("Lovelace"));
        assert!(!shown.contains("London"));
    }

    #[test]
    fn an_address_covers_another_whose_every_value_it_holds() {
        let saved = Address {
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            street_address: "12 St James Square\nFlat 3".into(),
            city: "London".into(),
            postal_code: "SW1Y 4JH".into(),
            country: "GB".into(),
            tel: "+44 20 7946 0000".into(),
            ..Address::default()
        };
        let typed = Address {
            given_name: "ada".into(),
            street_address: "12 St James Square, Flat 3".into(),
            postal_code: "sw1y4jh".into(),
            country: "GB".into(),
            ..Address::default()
        };
        assert!(saved.covers(&typed));
        assert!(!saved.covers(&Address {
            city: "Paris".into(),
            ..typed.clone()
        }));
        assert!(!typed.covers(&saved), "an address with less covers no more");
        assert!(saved.covers(&Address::default()));
    }

    #[test]
    fn covering_keeps_word_breaks_and_reads_a_phone_number_in_either_form() {
        let flat = Address {
            street_address: "Apt 12, 3 Oak Rd".into(),
            tel: "+44 20 7946 0000".into(),
            ..Address::default()
        };
        assert!(!flat.covers(&Address {
            street_address: "Apt 1, 23 Oak Rd".into(),
            ..Address::default()
        }));
        assert!(flat.covers(&Address {
            tel: "020 7946 0000".into(),
            ..Address::default()
        }));
        assert!(!flat.covers(&Address {
            tel: "020 7946 0001".into(),
            ..Address::default()
        }));
    }

    #[test]
    fn a_whole_name_is_read_against_the_given_and_family_names() {
        let saved = Address {
            given_name: "Juan".into(),
            family_name: "García López".into(),
            ..Address::default()
        };
        assert!(saved.named("juan garcía  lópez"));
        assert!(!saved.named("Juan García"));
        let japanese = Address {
            given_name: "太郎".into(),
            family_name: "山田".into(),
            ..Address::default()
        };
        assert!(japanese.named("山田 太郎"));
        assert!(japanese.named("山田太郎"));
        assert!(japanese.named("太郎 山田"));
    }

    #[test]
    fn an_address_at_the_same_place_takes_only_what_changed() {
        let saved = Address {
            given_name: "Ada".into(),
            family_name: "Lovelace".into(),
            street_address: "12 St James Square\nFlat 3".into(),
            city: "London".into(),
            postal_code: "SW1Y 4JH".into(),
            country: "GB".into(),
            tel: "+44 20 7946 0000".into(),
            ..Address::default()
        };
        let typed = Address {
            given_name: "ada".into(),
            street_address: "12 St James Square, Flat 3".into(),
            postal_code: "sw1y4jh".into(),
            tel: "+44 20 7946 9999".into(),
            email: "ada@example.org".into(),
            ..Address::default()
        };
        assert!(saved.same_place(&typed));
        let updated = saved.updated_by(&typed);
        assert_eq!(updated.tel, "+44 20 7946 9999");
        assert_eq!(updated.email, "ada@example.org");
        assert_eq!(
            updated.given_name, "Ada",
            "a value written differently stays as saved"
        );
        assert_eq!(updated.street_address, saved.street_address);
        assert_eq!(updated.family_name, "Lovelace");

        let elsewhere = Address {
            street_address: "9 Elm Road".into(),
            postal_code: "SW1Y 4JH".into(),
            ..Address::default()
        };
        assert!(!saved.same_place(&elsewhere));
        let street_only = Address {
            street_address: "12 St James Square\nFlat 3".into(),
            ..Address::default()
        };
        assert!(
            !saved.same_place(&street_only),
            "a street alone does not place it"
        );
        let by_city = Address {
            city: "london".into(),
            ..street_only
        };
        assert!(saved.same_place(&by_city));
        let other_district = Address {
            postal_code: "SW1A 1AA".into(),
            ..by_city
        };
        assert!(
            !saved.same_place(&other_district),
            "two postcodes decide on their own"
        );
    }

    #[test]
    fn a_phone_number_is_read_by_its_digits_whatever_their_width() {
        let phone = |tel: &str| Address {
            tel: tel.into(),
            ..Address::default()
        };
        assert!(phone("090-1234-5678").covers(&phone("０９０-１２３４-５６７８")));
        assert!(!phone("").covers(&phone("n/a")));
        assert!(!phone("+1 212 555 0123").covers(&phone("312-555-0123")));
        assert!(phone("+1 212 555 0123").covers(&phone("(212) 555-0123")));
    }
}
