//! Password CSV (Chrome/Firefox/Safari export format) parsing and
//! serialization.
//!
//! This module is the pure core: an RFC 4180 reader with header sniffing over
//! the CSV shapes password managers export, and the serializer for Wis's own
//! export. The shell owns all I/O, sealing, and storage.
//!
//! Rows keep only what the vault stores: a bare `www.`-stripped host derived
//! from the `url` column (http(s) only), the username, and the password
//! verbatim. Everything else (names, notes, timestamps, guids) is dropped.

use std::fmt;

use url::Url;

use crate::error::{WisError, WisResult};

/// Cap on an imported CSV file, rejected before parsing.
pub const MAX_PASSWORD_CSV_BYTES: usize = 8 * 1024 * 1024;
/// Cap on how many credentials one import may carry.
pub const MAX_IMPORT_CREDENTIALS: usize = 20_000;

/// Hosts and usernames are clamped to this many characters.
const FIELD_CAP: usize = 1024;
/// Rows with passwords beyond this are treated as malformed.
const MAX_PASSWORD_CHARS: usize = 4096;
/// Total fields across the whole file.
const MAX_TOTAL_FIELDS: usize = 200_000;

/// One credential from a CSV row. The password is plaintext: safe only in
/// memory on its way into the sealed vault (import) or into a user-confirmed
/// plaintext export.
#[derive(Clone, PartialEq, Eq)]
pub struct ParsedCredential {
    pub host: String,
    pub username: String,
    pub password: String,
}

// Hand-written so a Debug print of a credential can never leak the password
// into logs or error output.
impl fmt::Debug for ParsedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedCredential")
            .field("host", &self.host)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Parse result: usable credentials plus how many rows were dropped
/// (unparseable, non-web URL, missing or oversized fields).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedCredentials {
    pub credentials: Vec<ParsedCredential>,
    pub skipped: usize,
}

/// Parse a password CSV. The input is untrusted: the header must carry
/// `url`/`username`/`password` columns (any order, any casing), malformed
/// rows are counted and skipped, and the credential count is capped.
pub fn parse_password_csv(source: &str) -> WisResult<ParsedCredentials> {
    if source.len() > MAX_PASSWORD_CSV_BYTES {
        return Err(WisError::InvalidInput {
            message: "password file is too large".to_string(),
        });
    }

    // Firefox writes a UTF-8 BOM.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);

    let mut records = read_records(source)?.into_iter();
    let header_error = || WisError::InvalidInput {
        message: "not a password CSV (missing url/username/password columns)".to_string(),
    };
    let header = records.next().ok_or_else(header_error)?;
    let column = |name: &str| {
        header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let (Some(url_col), Some(user_col), Some(pass_col)) =
        (column("url"), column("username"), column("password"))
    else {
        return Err(header_error());
    };

    let mut out = ParsedCredentials::default();
    for row in records {
        let host = row.get(url_col).and_then(|u| accept_web_host(u));
        let username = row.get(user_col).map(|s| s.trim()).unwrap_or_default();
        let password = row.get(pass_col).map(String::as_str).unwrap_or_default();
        let (Some(host), false) = (host, password.is_empty()) else {
            out.skipped += 1;
            continue;
        };
        if username.chars().count() > FIELD_CAP || password.chars().count() > MAX_PASSWORD_CHARS {
            out.skipped += 1;
            continue;
        }

        out.credentials.push(ParsedCredential {
            host,
            username: username.to_string(),
            password: password.to_string(),
        });
        if out.credentials.len() > MAX_IMPORT_CREDENTIALS {
            return Err(WisError::InvalidInput {
                message: format!(
                    "password file holds more than {MAX_IMPORT_CREDENTIALS} credentials"
                ),
            });
        }
    }

    Ok(out)
}

/// Serialize credentials in Chrome's `name,url,username,password,note` shape,
/// the layout every password manager imports.
pub fn to_password_csv(rows: &[ParsedCredential]) -> String {
    let mut out = String::from("name,url,username,password,note\n");
    for row in rows {
        let url = format!("https://{}/", row.host);
        out.push_str(&format!(
            "{},{},{},{},\n",
            csv_field(&row.host),
            csv_field(&url),
            csv_field(&row.username),
            csv_field(&row.password)
        ));
    }

    out
}

/// Reduce a CSV `url` cell to the vault's stored host form: http(s) only
/// (a schemeless cell is assumed https), bare `www.`-stripped lowercase host.
fn accept_web_host(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 8 * 1024 {
        return None;
    }

    let parsed = match Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => Url::parse(&format!("https://{trimmed}")).ok()?,
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }

    let host = parsed.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);

    (!host.is_empty() && host.len() <= FIELD_CAP).then(|| host.to_ascii_lowercase())
}

/// Lenient RFC 4180 reader: `"` quoting with `""` escapes, embedded newlines
/// inside quotes, LF or CRLF records, blank lines skipped. A quote after the
/// start of an unquoted field is literal text. Errors once the file yields
/// more than [`MAX_TOTAL_FIELDS`] fields.
fn read_records(source: &str) -> WisResult<Vec<Vec<String>>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut fields = 0usize;
    let mut in_quotes = false;
    let mut chars = source.chars().peekable();

    let flush = |records: &mut Vec<Vec<String>>,
                 record: &mut Vec<String>,
                 field: &mut String,
                 fields: &mut usize| {
        record.push(std::mem::take(field));
        *fields += 1;

        if record.len() > 1 || !record[0].is_empty() {
            records.push(std::mem::take(record));
        } else {
            record.clear();
        }
    };

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }

            continue;
        }

        match c {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                record.push(std::mem::take(&mut field));
                fields += 1;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                flush(&mut records, &mut record, &mut field, &mut fields);
            }
            '\n' => flush(&mut records, &mut record, &mut field, &mut fields),
            _ => field.push(c),
        }
        if fields > MAX_TOTAL_FIELDS {
            return Err(WisError::InvalidInput {
                message: "password file has too many fields to be a credential export".to_string(),
            });
        }
    }

    if !field.is_empty() || !record.is_empty() {
        flush(&mut records, &mut record, &mut field, &mut fields);
    }

    Ok(records)
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(host: &str, username: &str, password: &str) -> ParsedCredential {
        ParsedCredential {
            host: host.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        }
    }

    #[test]
    fn parses_chrome_export() {
        let source = "name,url,username,password,note\n\
                      Example,https://www.example.com/login,alice,hunter2,\n\
                      Other,https://other.example/,bob,\"pa,ss\"\"word\",remember\n";
        let parsed = parse_password_csv(source).expect("parses");
        assert_eq!(parsed.skipped, 0);
        assert_eq!(
            parsed.credentials,
            vec![
                cred("example.com", "alice", "hunter2"),
                cred("other.example", "bob", "pa,ss\"word"),
            ]
        );
    }

    #[test]
    fn parses_firefox_export_with_bom_and_crlf() {
        let source = "\u{feff}\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\",\"timeCreated\",\"timeLastUsed\",\"timePasswordChanged\"\r\n\
                      \"https://example.com\",\"alice\",\"hunter2\",,\"https://example.com\",\"{guid}\",\"1\",\"2\",\"3\"\r\n";
        let parsed = parse_password_csv(source).expect("parses");
        assert_eq!(
            parsed.credentials,
            vec![cred("example.com", "alice", "hunter2")]
        );
    }

    #[test]
    fn rejects_files_without_credential_columns() {
        for source in ["", "a,b,c\n1,2,3\n", "name,site,login,secret\nx,y,z,w\n"] {
            let err = parse_password_csv(source).expect_err("not a password csv");
            assert!(matches!(err, WisError::InvalidInput { .. }));
        }
    }

    #[test]
    fn skips_bad_rows_and_counts_them() {
        let long_pass = "x".repeat(5000);
        let source = format!(
            "url,username,password\n\
             javascript:alert(1),a,b\n\
             https://ok.example/,alice,hunter2\n\
             https://empty.example/,bob,\n\
             https://long.example/,carol,{long_pass}\n\
             not a url at all with spaces,dan,pw\n"
        );
        let parsed = parse_password_csv(&source).expect("parses");
        assert_eq!(
            parsed.credentials,
            vec![cred("ok.example", "alice", "hunter2")]
        );
        assert_eq!(parsed.skipped, 4);
    }

    #[test]
    fn preserves_password_whitespace_and_quoted_newlines() {
        let source = "url,username,password\n\
                      https://a.example/,u,\" spaced \"\n\
                      https://b.example/,v,\"line1\nline2\"\n";
        let parsed = parse_password_csv(source).expect("parses");
        assert_eq!(parsed.credentials[0].password, " spaced ");
        assert_eq!(parsed.credentials[1].password, "line1\nline2");
    }

    #[test]
    fn normalizes_hosts_like_the_vault() {
        let source = "url,username,password\n\
                      https://WWW.Example.COM/path,a,pw\n\
                      example.org,b,pw\n\
                      https://example.net:8443/,c,pw\n";
        let parsed = parse_password_csv(source).expect("parses");
        let hosts: Vec<&str> = parsed.credentials.iter().map(|c| c.host.as_str()).collect();
        assert_eq!(hosts, vec!["example.com", "example.org", "example.net"]);
    }

    #[test]
    fn rejects_oversized_imports() {
        let mut source = String::from("url,username,password\n");
        for i in 0..=MAX_IMPORT_CREDENTIALS {
            source.push_str(&format!("https://e{i}.example/,u,pw\n"));
        }
        let err = parse_password_csv(&source).expect_err("over cap");
        assert!(matches!(err, WisError::InvalidInput { .. }));
    }

    #[test]
    fn rejects_field_bombs() {
        let source = format!("url,username,password\n{}", ",".repeat(300_000));
        let err = parse_password_csv(&source).expect_err("too many fields");
        assert!(matches!(err, WisError::InvalidInput { .. }));
    }

    #[test]
    fn round_trips_through_serializer() {
        let original = vec![
            cred("example.com", "alice", "hun,ter\"2"),
            cred("other.example", "", "with\nnewline"),
        ];
        let csv = to_password_csv(&original);
        let parsed = parse_password_csv(&csv).expect("round trip parses");
        assert_eq!(parsed.credentials, original);
        assert_eq!(parsed.skipped, 0);
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        let c = cred("example.com", "alice", "supersecret");
        let debug = format!("{c:?}");
        assert!(!debug.contains("supersecret"));
        assert!(debug.contains("<redacted>"));
    }
}
