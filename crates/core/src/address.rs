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
}

fn printable(text: &str) -> String {
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
}
