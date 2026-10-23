//! A payment card, as kept in the password vault. The security code is
//! never kept.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{WisError, WisResult};

pub const MAX_CARDS: usize = 100;

#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Card {
    /// Digits only once tidied.
    pub number: Zeroizing<String>,
    pub name: String,
    pub exp_month: u8,
    /// Four digits once tidied.
    pub exp_year: u16,
    pub nickname: String,
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Card { .. }")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardBrand {
    Visa,
    Mastercard,
    Amex,
    Discover,
    Jcb,
    Diners,
    Unionpay,
    Other,
}

/// What a saved card shows without the vault key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSummary {
    pub nickname: String,
    pub brand: CardBrand,
    pub last4: String,
    pub exp_month: u8,
    pub exp_year: u16,
}

const MAX_NAME_CHARS: usize = 100;

impl Card {
    /// Digits only and passing the check digit, with a real month, a four-digit
    /// year and trimmed printable names.
    pub fn tidied(self) -> WisResult<Card> {
        if self.number.len() > 64 {
            return Err(invalid("This card number isn't valid"));
        }

        let mut number = Zeroizing::new(String::with_capacity(self.number.len()));
        number.extend(self.number.chars().filter(|c| !matches!(c, ' ' | '-')));
        let valid = number.chars().all(|c| c.is_ascii_digit())
            && (12..=19).contains(&number.len())
            && (CardBrand::of(&number) == CardBrand::Unionpay || luhn(&number));
        if !valid {
            return Err(invalid("This card number isn't valid"));
        }
        if !(1..=12).contains(&self.exp_month) {
            return Err(invalid("The expiry month isn't valid"));
        }

        let exp_year = match self.exp_year {
            year @ 1..=99 => year + 2000,
            year => year,
        };
        if !(2000..=2099).contains(&exp_year) {
            return Err(invalid("The expiry year isn't valid"));
        }

        Ok(Card {
            number,
            name: short(&self.name)?,
            exp_month: self.exp_month,
            exp_year,
            nickname: short(&self.nickname)?,
        })
    }

    pub fn summary(&self) -> CardSummary {
        let start = self.number.len().saturating_sub(4);
        CardSummary {
            nickname: self.nickname.clone(),
            brand: CardBrand::of(&self.number),
            last4: self.number[start..].to_string(),
            exp_month: self.exp_month,
            exp_year: self.exp_year,
        }
    }
}

impl CardBrand {
    const ALL: [CardBrand; 8] = [
        CardBrand::Visa,
        CardBrand::Mastercard,
        CardBrand::Amex,
        CardBrand::Discover,
        CardBrand::Jcb,
        CardBrand::Diners,
        CardBrand::Unionpay,
        CardBrand::Other,
    ];

    /// The network a card number's issuer prefix belongs to.
    pub fn of(number: &str) -> CardBrand {
        let prefix = |len: usize| number.get(..len).and_then(|p| p.parse::<u32>().ok());
        let within = |len: usize, low: u32, high: u32| {
            prefix(len).is_some_and(|p| (low..=high).contains(&p))
        };
        if number.starts_with('4') {
            CardBrand::Visa
        } else if within(2, 51, 55) || within(4, 2221, 2720) {
            CardBrand::Mastercard
        } else if within(2, 34, 34) || within(2, 37, 37) {
            CardBrand::Amex
        } else if within(4, 6011, 6011) || within(3, 644, 649) || within(2, 65, 65) {
            CardBrand::Discover
        } else if within(4, 3528, 3589) {
            CardBrand::Jcb
        } else if within(3, 300, 305)
            || within(4, 3095, 3095)
            || within(2, 36, 36)
            || within(2, 38, 39)
        {
            CardBrand::Diners
        } else if within(2, 62, 62) || within(2, 81, 81) {
            CardBrand::Unionpay
        } else {
            CardBrand::Other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CardBrand::Visa => "visa",
            CardBrand::Mastercard => "mastercard",
            CardBrand::Amex => "amex",
            CardBrand::Discover => "discover",
            CardBrand::Jcb => "jcb",
            CardBrand::Diners => "diners",
            CardBrand::Unionpay => "unionpay",
            CardBrand::Other => "other",
        }
    }

    pub fn parse(text: &str) -> CardBrand {
        CardBrand::ALL
            .into_iter()
            .find(|brand| brand.as_str() == text)
            .unwrap_or(CardBrand::Other)
    }
}

fn luhn(digits: &str) -> bool {
    let sum: u32 = digits
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = u32::from(b - b'0');
            match (i % 2 == 1, d * 2) {
                (true, doubled) if doubled > 9 => doubled - 9,
                (true, doubled) => doubled,
                (false, _) => d,
            }
        })
        .sum();
    sum % 10 == 0
}

fn short(text: &str) -> WisResult<String> {
    if text.len() > 4 * MAX_NAME_CHARS {
        return Err(invalid("A card name is too long"));
    }
    let kept = crate::vault::address::printable(text.trim());
    if kept.chars().count() > MAX_NAME_CHARS {
        return Err(invalid("A card name is too long"));
    }
    Ok(kept)
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(digits: &str) -> Zeroizing<String> {
        Zeroizing::new(digits.to_string())
    }

    fn visa() -> Card {
        Card {
            number: number("4242 4242 4242 4242"),
            name: " Ada Lovelace ".into(),
            exp_month: 8,
            exp_year: 28,
            nickname: " Travel ".into(),
        }
    }

    #[test]
    fn a_card_is_tidied_to_digits_a_full_year_and_trimmed_names() {
        let card = visa().tidied().unwrap();
        assert_eq!(*card.number, "4242424242424242");
        assert_eq!(card.name, "Ada Lovelace");
        assert_eq!(card.nickname, "Travel");
        assert_eq!(card.exp_year, 2028);
        let dashed = Card {
            number: number("4242-4242-4242-4242"),
            exp_year: 2031,
            ..visa()
        };
        assert_eq!(dashed.tidied().unwrap().exp_year, 2031);
    }

    #[test]
    fn a_number_failing_the_check_digit_or_length_is_refused() {
        for digits in [
            "4242 4242 4242 4241",
            "4242",
            "42424242424242424242",
            "4242 4242 4242 424x",
            "",
        ] {
            assert!(
                Card {
                    number: number(digits),
                    ..visa()
                }
                .tidied()
                .is_err(),
                "{digits}"
            );
        }
    }

    /// Some UnionPay cards are issued without a valid check digit.
    #[test]
    fn a_unionpay_number_skips_the_check_digit() {
        let card = Card {
            number: number("6200 0000 0000 0001"),
            ..visa()
        };
        assert_eq!(*card.tidied().unwrap().number, "6200000000000001");
    }

    #[test]
    fn an_impossible_expiry_is_refused() {
        assert!(Card {
            exp_year: 0,
            ..visa()
        }
        .tidied()
        .is_err());
        assert!(Card {
            exp_month: 0,
            ..visa()
        }
        .tidied()
        .is_err());
        assert!(Card {
            exp_month: 13,
            ..visa()
        }
        .tidied()
        .is_err());
        assert!(Card {
            exp_year: 1999,
            ..visa()
        }
        .tidied()
        .is_err());
        assert!(Card {
            exp_year: 2100,
            ..visa()
        }
        .tidied()
        .is_err());
    }

    #[test]
    fn an_overlong_name_is_refused_and_control_characters_dropped() {
        assert!(Card {
            name: "a".repeat(101),
            ..visa()
        }
        .tidied()
        .is_err());
        let card = Card {
            nickname: "Wo\u{0007}rk".into(),
            ..visa()
        }
        .tidied()
        .unwrap();
        assert_eq!(card.nickname, "Work");
    }

    #[test]
    fn the_brand_follows_the_issuer_prefix() {
        let cases = [
            ("4111111111111111", CardBrand::Visa),
            ("5555555555554444", CardBrand::Mastercard),
            ("2223003122003222", CardBrand::Mastercard),
            ("378282246310005", CardBrand::Amex),
            ("6011111111111117", CardBrand::Discover),
            ("6445644564456445", CardBrand::Discover),
            ("3566002020360505", CardBrand::Jcb),
            ("36227206271667", CardBrand::Diners),
            ("6200000000000005", CardBrand::Unionpay),
            ("8171999927660000", CardBrand::Unionpay),
            ("30950000000000", CardBrand::Diners),
            ("1234567812345670", CardBrand::Other),
        ];
        for (number, brand) in cases {
            assert_eq!(CardBrand::of(number), brand, "{number}");
            assert_eq!(CardBrand::parse(brand.as_str()), brand);
        }
        assert_eq!(CardBrand::parse("nonsense"), CardBrand::Other);
    }

    #[test]
    fn the_summary_holds_nothing_beyond_the_last_four_digits() {
        let summary = visa().tidied().unwrap().summary();
        assert_eq!(
            summary,
            CardSummary {
                nickname: "Travel".into(),
                brand: CardBrand::Visa,
                last4: "4242".into(),
                exp_month: 8,
                exp_year: 2028,
            }
        );
    }

    #[test]
    fn debug_output_never_shows_the_number() {
        assert_eq!(format!("{:?}", visa()), "Card { .. }");
    }
}
