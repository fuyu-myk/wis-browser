//! Whether Wis offers a generated password when a signup form appears.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasswordOffer {
    /// Show the offer and let the user decide, each time.
    #[default]
    Ask,
    /// Fill a generated password without asking, but only into an empty field.
    Always,
    /// Never offer.
    Never,
}

impl PasswordOffer {
    /// Whether a signup form should produce anything at all.
    pub fn offers(self) -> bool {
        !matches!(self, Self::Never)
    }

    /// The standing choice a remembered decision produces.
    pub fn remembered(accepted: bool) -> Self {
        if accepted {
            Self::Always
        } else {
            Self::Never
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_is_the_default() {
        assert_eq!(PasswordOffer::default(), PasswordOffer::Ask);
    }

    #[test]
    fn only_never_stops_offering() {
        assert!(PasswordOffer::Ask.offers());
        assert!(PasswordOffer::Always.offers());
        assert!(!PasswordOffer::Never.offers());
    }

    #[test]
    fn remembering_carries_the_choice_that_was_made() {
        assert_eq!(PasswordOffer::remembered(true), PasswordOffer::Always);
        assert_eq!(PasswordOffer::remembered(false), PasswordOffer::Never);
    }
}
