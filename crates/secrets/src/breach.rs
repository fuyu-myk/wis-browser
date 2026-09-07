//! Checking a password against known breaches without disclosure.
//!
//! The scheme is k-anonymity, as the Pwned Passwords range API defines it: the
//! password is hashed with SHA-1, only the first five hex characters of that
//! hash are sent, and the service answers with every hash it holds sharing
//! that prefix. The remaining 35 characters are compared locally, so the
//! password itself and even its full hash never leave.
//!
//! SHA-1 is not a defensible password hash and is not used as one. It is the
//! identifier the published data set is keyed by, and the comparison is a
//! set-membership test against a public list.

use sha1::{Digest, Sha1};

/// Hex characters of the hash the service is allowed to see. The rest is
/// compared locally.
pub const RANGE_PREFIX_LEN: usize = 5;

/// Split a password's SHA-1 into the querying prefix and the suffix to
/// compare against the answer. Both are uppercase hex.
pub fn range_split(password: &str) -> (String, String) {
    let digest = Sha1::digest(password.as_bytes());
    let hash = digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let (prefix, suffix) = hash.split_at(RANGE_PREFIX_LEN);
    (prefix.to_string(), suffix.to_string())
}

/// How many known breaches hold this suffix, from a range response. Zero when
/// the password is not in the data set.
///
/// Each line is `SUFFIX:COUNT`. Requests ask for padding, so the answer also
/// carries decoy suffixes with a count of zero.
pub fn count_in_range(body: &str, suffix: &str) -> u32 {
    for line in body.lines() {
        let Some((listed, count)) = line.trim().split_once(':') else {
            continue;
        };
        if !listed.trim().eq_ignore_ascii_case(suffix) {
            continue;
        }
        // A line whose count will not parse is treated as noise.
        if let Ok(count) = count.trim().parse::<u32>() {
            return count;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The most breached password there is, and its published hash.
    const KNOWN_HASH: &str = "5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8";

    #[test]
    fn a_password_splits_into_a_five_character_prefix_and_the_rest() {
        let (prefix, suffix) = range_split("password");
        assert_eq!(prefix, &KNOWN_HASH[..RANGE_PREFIX_LEN]);
        assert_eq!(suffix, &KNOWN_HASH[RANGE_PREFIX_LEN..]);
        assert_eq!(format!("{prefix}{suffix}"), KNOWN_HASH);
    }

    #[test]
    fn the_prefix_never_grows_whatever_the_password() {
        for password in ["", "a", "correct horse battery staple", "🔐 emoji"] {
            let (prefix, suffix) = range_split(password);
            assert_eq!(prefix.len(), RANGE_PREFIX_LEN, "prefix for {password:?}");
            assert_eq!(prefix.len() + suffix.len(), 40, "hash for {password:?}");
            assert!(
                prefix
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_lowercase()),
                "prefix must be uppercase hex"
            );
        }
    }

    #[test]
    fn different_passwords_ask_with_different_hashes() {
        let (_, one) = range_split("correct horse battery staple");
        let (_, two) = range_split("password");
        assert_ne!(one, two);
    }

    #[test]
    fn a_listed_suffix_reports_its_count() {
        let body = "0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n\
                    1E4C9B93F3F0682250B6CF8331B7EE68FD8:9659365\r\n\
                    011053FD0102E94D6AE2F8B83D76FAF94F6:1\r\n";
        assert_eq!(
            count_in_range(body, &KNOWN_HASH[RANGE_PREFIX_LEN..]),
            9_659_365
        );
    }

    #[test]
    fn a_suffix_the_answer_does_not_hold_is_not_breached() {
        let body = "0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n";
        assert_eq!(count_in_range(body, &KNOWN_HASH[RANGE_PREFIX_LEN..]), 0);
    }

    #[test]
    fn the_comparison_ignores_case_and_stray_whitespace() {
        let suffix = &KNOWN_HASH[RANGE_PREFIX_LEN..];
        let body = format!("  {}:42  \n", suffix.to_lowercase());
        assert_eq!(count_in_range(&body, suffix), 42);
    }

    #[test]
    fn padding_decoys_are_reported_as_the_zero_they_claim() {
        // Requests ask for padding, so the answer carries decoys at count 0.
        let suffix = &KNOWN_HASH[RANGE_PREFIX_LEN..];
        assert_eq!(count_in_range(&format!("{suffix}:0\r\n"), suffix), 0);
    }

    #[test]
    fn malformed_lines_are_skipped_rather_than_counted() {
        let suffix = &KNOWN_HASH[RANGE_PREFIX_LEN..];
        let body = format!("garbage\r\n\r\n{suffix}:not-a-number\r\n{suffix}:7\r\n");
        assert_eq!(count_in_range(&body, suffix), 7);
    }
}
