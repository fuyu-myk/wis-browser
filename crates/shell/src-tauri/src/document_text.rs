//! Text from the PDF and word processor files attached to a canvas agent
//! request. The native parsers read untrusted files, so they run in a short
//! lived child of this binary that drops what it inherited and then may only
//! read the system's own files: no network, no Keychain or other services, no
//! writes. A crash or a hang costs only the child, and its memory goes back
//! when it exits.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use wis_core::{WisError, WisResult};

const CHILD_FLAG: &str = "--wis-read-document";
pub const MAX_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_CHARS: usize = 200_000;
const TIME_LIMIT: Duration = Duration::from_secs(30);
/// Readers running at once, across every canvas tab.
static READERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Pdf,
    Docx,
    Doc,
    Rtf,
    Odt,
}

impl DocKind {
    fn arg(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Doc => "doc",
            Self::Rtf => "rtf",
            Self::Odt => "odt",
        }
    }

    fn from_arg(arg: &str) -> Option<Self> {
        [Self::Pdf, Self::Docx, Self::Doc, Self::Rtf, Self::Odt]
            .into_iter()
            .find(|kind| kind.arg() == arg)
    }

    /// Whether `bytes` open the way this kind of file does.
    fn fits(self, bytes: &[u8]) -> bool {
        match self {
            Self::Pdf => bytes[..bytes.len().min(1024)]
                .windows(5)
                .any(|w| w == b"%PDF-"),
            Self::Docx | Self::Odt => bytes.starts_with(b"PK\x03\x04"),
            Self::Doc => bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
            Self::Rtf => bytes.starts_with(br"{\rtf"),
        }
    }
}

/// How much of a file was read: pages of a PDF, else UTF-16 units.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partial {
    pub read: usize,
    pub total: usize,
    pub pages: bool,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum Outcome {
    Read {
        text: String,
        partial: Option<Partial>,
    },
    Locked,
    Empty,
    Unreadable,
}

/// Lengths in UTF-16 units, as the page counts them.
fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Line ends made plain; object placeholders, NULs and other controls gone.
fn tidy(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\r' | '\u{2028}' | '\u{2029}' => Some('\n'),
            '\n' | '\t' => Some(c),
            '\u{fffc}' => None,
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// The start of `text` within `max` UTF-16 units.
fn cut(text: &str, max: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= max
        })
        .collect()
}

fn finish(text: String, partial: Option<Partial>) -> Outcome {
    if text.trim().is_empty() {
        Outcome::Empty
    } else {
        Outcome::Read { text, partial }
    }
}

/// Whole pages while they fit in `max`; a first page too long alone is cut.
fn gather(pages: impl Iterator<Item = String>, total: usize, max: usize) -> Outcome {
    let mut text = String::new();
    let mut used = 0;
    for (read, page) in pages.enumerate() {
        let page = tidy(&page);
        let page = page.trim();
        if page.is_empty() {
            continue;
        }

        let gap = if text.is_empty() { 0 } else { 2 };
        let size = units(page);
        if used + gap + size > max {
            if text.is_empty() {
                return finish(
                    cut(page, max),
                    Some(Partial {
                        read: read + 1,
                        total,
                        pages: true,
                    }),
                );
            }

            return finish(
                text,
                Some(Partial {
                    read,
                    total,
                    pages: true,
                }),
            );
        }
        if gap > 0 {
            text.push_str("\n\n");
        }

        text.push_str(page);
        used += gap + size;
    }

    finish(text, None)
}

/// A flowing document's text, cut to `max` when longer.
fn whole(text: &str, max: usize) -> Outcome {
    let text = tidy(text);
    let text = text.trim();
    let total = units(text);
    if total <= max {
        return finish(text.to_string(), None);
    }

    finish(
        cut(text, max),
        Some(Partial {
            read: max,
            total,
            pages: false,
        }),
    )
}

/// Reads `bytes` as a `kind` document in a child process.
pub async fn read(kind: DocKind, bytes: Vec<u8>, max: usize) -> WisResult<Outcome> {
    if bytes.len() > MAX_BYTES {
        return Err(WisError::InvalidInput {
            message: "the file is too large to read".into(),
        });
    }
    if !kind.fits(&bytes) {
        return Ok(Outcome::Unreadable);
    }
    if !cfg!(target_os = "macos") {
        return Err(WisError::InvalidInput {
            message: "reading PDF and Word files needs macOS for now".into(),
        });
    }

    let max = max.min(MAX_CHARS);
    let Ok(_reader) = READERS.acquire().await else {
        return Ok(Outcome::Unreadable);
    };
    match tokio::time::timeout(TIME_LIMIT, run_child(kind, &bytes, max)).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(err)) => {
            log::warn!("document reader: {err}");
            Ok(Outcome::Unreadable)
        }
        Err(_) => Ok(Outcome::Unreadable),
    }
}

async fn run_child(kind: DocKind, bytes: &[u8], max: usize) -> std::io::Result<Outcome> {
    use std::process::Stdio;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args([CHILD_FLAG, kind.arg(), &max.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err(std::io::Error::other("no pipes to the reader"));
    };
    stdin.write_all(bytes).await?;
    drop(stdin);

    let mut out = Vec::new();
    let most = max as u64 * 3 + 4096;
    stdout.take(most).read_to_end(&mut out).await?;
    if !child.wait().await?.success() {
        return Ok(Outcome::Unreadable);
    }

    let outcome = serde_json::from_slice::<Outcome>(&out).map_err(std::io::Error::other)?;
    Ok(match outcome {
        Outcome::Read { ref text, .. } if units(text) > max => Outcome::Unreadable,
        outcome => outcome,
    })
}

/// Runs this process as a document reader when it was started as one, and
/// never returns then.
pub fn serve_child() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(CHILD_FLAG)) {
        return;
    }

    let kind = args.next().and_then(|arg| DocKind::from_arg(arg.to_str()?));
    let max = args
        .next()
        .and_then(|arg| arg.to_str()?.parse::<usize>().ok());
    let code = match (kind, max) {
        (Some(kind), Some(max)) => child(kind, max.min(MAX_CHARS)),
        _ => 2,
    };

    std::process::exit(code);
}

fn child(kind: DocKind, max: usize) -> i32 {
    use std::io::Read;

    native::close_inherited();
    native::limit_cpu(TIME_LIMIT.as_secs() + 10);
    let mut bytes = Vec::new();
    let read = std::io::stdin()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > MAX_BYTES {
        return 2;
    }
    if !native::confine() {
        return 3;
    }

    let outcome = native::extract(kind, &bytes, max);
    match serde_json::to_writer(std::io::stdout().lock(), &outcome) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{c_char, c_int, c_void, CStr};

    use objc2::encode::{Encode, Encoding};
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    use super::{gather, whole, DocKind, Outcome};

    #[link(name = "PDFKit", kind = "framework")]
    extern "C" {}

    #[link(name = "AppKit", kind = "framework")]
    extern "C" {
        static NSDocumentTypeDocumentOption: *const AnyObject;
        static NSOfficeOpenXMLTextDocumentType: *const AnyObject;
        static NSDocFormatTextDocumentType: *const AnyObject;
        static NSRTFTextDocumentType: *const AnyObject;
        static NSOpenDocumentTextDocumentType: *const AnyObject;
    }

    extern "C" {
        fn sandbox_init(profile: *const c_char, flags: u64, errorbuf: *mut *mut c_char) -> c_int;
        fn sandbox_free_error(errorbuf: *mut c_char);
    }

    /// Reads of the system's own files only: no network, no mach services
    /// (Keychain, LaunchServices, the font server), no writes, nothing of the
    /// user's.
    const PROFILE: &CStr = c"(version 1)(deny default)(allow file-read* \
        (subpath \"/System\") (subpath \"/usr/lib\") (subpath \"/usr/share\") \
        (subpath \"/Library/Fonts\"))";

    #[repr(C)]
    struct NSRange {
        location: usize,
        length: usize,
    }

    // SAFETY: matches Foundation's `{_NSRange=QQ}`.
    unsafe impl Encode for NSRange {
        const ENCODING: Encoding =
            Encoding::Struct("_NSRange", &[usize::ENCODING, usize::ENCODING]);
    }

    /// Closes every descriptor the parent let through; only stdio stays.
    pub fn close_inherited() {
        let Ok(entries) = std::fs::read_dir("/dev/fd") else {
            return;
        };
        let open: Vec<c_int> = entries
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
            .filter(|fd| *fd > 2)
            .collect();
        for fd in open {
            // SAFETY: closing a descriptor number; nothing here holds it.
            unsafe { libc::close(fd) };
        }
    }

    /// Ends the reader on its own if it outlives the parent's timeout, as
    /// when Wis quits mid-read.
    pub fn limit_cpu(seconds: u64) {
        let limit = libc::rlimit {
            rlim_cur: seconds,
            rlim_max: seconds,
        };
        // SAFETY: a plain struct for the duration of the call.
        unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) };
    }

    /// Readies PDFKit, which looks outside the profile the first time it
    /// runs, then confines this process for good.
    pub fn confine() -> bool {
        let mut error: *mut c_char = std::ptr::null_mut();
        // SAFETY: an empty document only loads PDFKit; the profile is a
        // literal and its error buffer is freed with the matching call.
        unsafe {
            autoreleasepool(|_| {
                let empty: *mut AnyObject = msg_send![class!(NSData), data];
                let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
                let doc: *mut AnyObject = msg_send![doc, initWithData: empty];
                if !doc.is_null() {
                    let _: () = msg_send![doc, release];
                }
            });
            let code = sandbox_init(PROFILE.as_ptr(), 0, &mut error);
            if !error.is_null() {
                sandbox_free_error(error);
            }
            code == 0
        }
    }

    pub fn extract(kind: DocKind, bytes: &[u8], max: usize) -> Outcome {
        // SAFETY: every send matches its method (see the tests); objects
        // made here are released here or autoreleased by the pool.
        autoreleasepool(|_| unsafe {
            let data: *mut AnyObject = msg_send![
                class!(NSData),
                dataWithBytes: bytes.as_ptr() as *const c_void,
                length: bytes.len()
            ];
            if data.is_null() {
                return Outcome::Unreadable;
            }

            match kind {
                DocKind::Pdf => pdf(data, max),
                DocKind::Docx => flowing(data, NSOfficeOpenXMLTextDocumentType, max),
                DocKind::Doc => flowing(data, NSDocFormatTextDocumentType, max),
                DocKind::Rtf => flowing(data, NSRTFTextDocumentType, max),
                DocKind::Odt => flowing(data, NSOpenDocumentTextDocumentType, max),
            }
        })
    }

    /// Copied as UTF-16, so a NUL or a lone surrogate inside cannot end or
    /// drop the text.
    unsafe fn string(ns: *mut AnyObject) -> String {
        if ns.is_null() {
            return String::new();
        }

        let length: usize = msg_send![ns, length];
        let mut units = vec![0u16; length];
        let range = NSRange {
            location: 0,
            length,
        };

        let _: () = msg_send![ns, getCharacters: units.as_mut_ptr(), range: range];
        String::from_utf16_lossy(&units)
    }

    unsafe fn pdf(data: *mut AnyObject, max: usize) -> Outcome {
        let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
        let doc: *mut AnyObject = msg_send![doc, initWithData: data];
        if doc.is_null() {
            return Outcome::Unreadable;
        }

        let locked: bool = msg_send![doc, isLocked];
        let outcome = if locked {
            Outcome::Locked
        } else {
            let count: usize = msg_send![doc, pageCount];
            let pages = (0..count).map(|at| {
                autoreleasepool(|_| {
                    let page: *mut AnyObject = msg_send![doc, pageAtIndex: at];
                    if page.is_null() {
                        return String::new();
                    }

                    string(msg_send![page, string])
                })
            });
            gather(pages, count, max)
        };

        let _: () = msg_send![doc, release];
        outcome
    }

    unsafe fn flowing(data: *mut AnyObject, kind: *const AnyObject, max: usize) -> Outcome {
        let options: *mut AnyObject = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: kind,
            forKey: NSDocumentTypeDocumentOption
        ];
        let text: *mut AnyObject = msg_send![class!(NSAttributedString), alloc];
        let text: *mut AnyObject = msg_send![
            text,
            initWithData: data,
            options: options,
            documentAttributes: std::ptr::null_mut::<*mut AnyObject>(),
            error: std::ptr::null_mut::<*mut AnyObject>()
        ];
        if text.is_null() {
            return Outcome::Unreadable;
        }

        let outcome = whole(&string(msg_send![text, string]), max);
        let _: () = msg_send![text, release];
        outcome
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::c_void;

        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, sel};

        use super::*;

        type Obj = *mut AnyObject;

        #[test]
        fn every_send_matches_its_method() {
            class!(NSData)
                .metaclass()
                .verify_sel::<(*const c_void, usize), Obj>(sel!(dataWithBytes:length:))
                .unwrap();
            let pdf = class!(PDFDocument);
            pdf.verify_sel::<(Obj,), Obj>(sel!(initWithData:)).unwrap();
            pdf.verify_sel::<(), Bool>(sel!(isLocked)).unwrap();
            pdf.verify_sel::<(), usize>(sel!(pageCount)).unwrap();
            pdf.verify_sel::<(usize,), Obj>(sel!(pageAtIndex:)).unwrap();
            class!(PDFPage).verify_sel::<(), Obj>(sel!(string)).unwrap();
            class!(NSDictionary)
                .metaclass()
                .verify_sel::<(Obj, Obj), Obj>(sel!(dictionaryWithObject:forKey:))
                .unwrap();
            class!(NSAttributedString)
                .verify_sel::<(Obj, Obj, *mut Obj, *mut Obj), Obj>(sel!(
                    initWithData:options:documentAttributes:error:
                ))
                .unwrap();
            class!(NSAttributedString)
                .verify_sel::<(), Obj>(sel!(string))
                .unwrap();
            class!(NSData)
                .metaclass()
                .verify_sel::<(), Obj>(sel!(data))
                .unwrap();
            let string = class!(NSString);
            string.verify_sel::<(), usize>(sel!(length)).unwrap();
            string
                .verify_sel::<(*mut u16, NSRange), ()>(sel!(getCharacters:range:))
                .unwrap();
        }

        #[test]
        fn a_nul_or_lone_surrogate_keeps_the_rest_of_the_text() {
            let units: [u16; 6] = [0x61, 0x00, 0x62, 0xD800, 0x63, 0x64];
            let ns: *mut AnyObject = unsafe {
                msg_send![class!(NSString), stringWithCharacters: units.as_ptr(), length: units.len()]
            };
            assert_eq!(unsafe { string(ns) }, "a\0b\u{FFFD}cd");
        }

        #[test]
        fn rich_text_comes_out_plain() {
            let rtf = br"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0 Quarterly plan\par Ship it.\par}";
            assert_eq!(
                extract(DocKind::Rtf, rtf, 1000),
                Outcome::Read {
                    text: "Quarterly plan\nShip it.".into(),
                    partial: None
                }
            );
            assert_eq!(
                extract(DocKind::Pdf, b"%PDF-1.4 nonsense", 1000),
                Outcome::Unreadable
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{DocKind, Outcome};

    pub fn close_inherited() {}

    pub fn limit_cpu(_seconds: u64) {}

    pub fn confine() -> bool {
        false
    }

    pub fn extract(_kind: DocKind, _bytes: &[u8], _max: usize) -> Outcome {
        Outcome::Unreadable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, partial: Option<Partial>) -> Outcome {
        Outcome::Read {
            text: text.into(),
            partial,
        }
    }

    #[test]
    fn a_file_must_open_like_its_kind() {
        assert!(DocKind::Pdf.fits(b"%PDF-1.7\n"));
        assert!(DocKind::Pdf.fits(b"\xef\xbb\xbf junk %PDF-1.4"));
        assert!(!DocKind::Pdf.fits(b"PK\x03\x04"));
        assert!(DocKind::Docx.fits(b"PK\x03\x04rest"));
        assert!(DocKind::Odt.fits(b"PK\x03\x04rest"));
        assert!(DocKind::Doc.fits(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]));
        assert!(DocKind::Rtf.fits(br"{\rtf1\ansi"));
        assert!(!DocKind::Rtf.fits(b"plain text"));
        assert!(!DocKind::Doc.fits(b""));
    }

    #[test]
    fn kinds_round_trip_through_the_child_argument() {
        for kind in [
            DocKind::Pdf,
            DocKind::Docx,
            DocKind::Doc,
            DocKind::Rtf,
            DocKind::Odt,
        ] {
            assert_eq!(DocKind::from_arg(kind.arg()), Some(kind));
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.arg()));
        }
        assert_eq!(DocKind::from_arg("exe"), None);
    }

    #[test]
    fn text_loses_placeholders_and_controls() {
        assert_eq!(tidy("a\r\nb\rc\u{2028}d"), "a\nb\nc\nd");
        assert_eq!(tidy("x\u{fffc}y\0z\u{7}\tw"), "xyz\tw");
    }

    #[test]
    fn cuts_count_utf16_units_and_never_split_a_character() {
        assert_eq!(cut("abcdef", 4), "abcd");
        assert_eq!(cut("ab\u{1F600}cd", 3), "ab");
        assert_eq!(cut("ab\u{1F600}cd", 4), "ab\u{1F600}");
        assert_eq!(cut("abc", 10), "abc");
    }

    #[test]
    fn pages_are_read_whole_while_they_fit() {
        let pages = || ["one".to_string(), " ".into(), "two".into(), "three".into()].into_iter();
        assert_eq!(gather(pages(), 4, 100), read("one\n\ntwo\n\nthree", None));
        assert_eq!(
            gather(pages(), 4, 9),
            read(
                "one\n\ntwo",
                Some(Partial {
                    read: 3,
                    total: 4,
                    pages: true
                })
            )
        );
        assert_eq!(
            gather(["abcdefgh".to_string()].into_iter(), 1, 3),
            read(
                "abc",
                Some(Partial {
                    read: 1,
                    total: 1,
                    pages: true
                })
            )
        );
        assert_eq!(
            gather([" ".to_string(), "abcdefgh".into()].into_iter(), 2, 3),
            read(
                "abc",
                Some(Partial {
                    read: 2,
                    total: 2,
                    pages: true
                })
            )
        );
        assert_eq!(gather([" ".to_string()].into_iter(), 1, 9), Outcome::Empty);
    }

    #[test]
    fn a_flowing_document_is_cut_to_the_limit() {
        assert_eq!(whole("  short \n", 100), read("short", None));
        assert_eq!(
            whole("abcdefghij", 4),
            read(
                "abcd",
                Some(Partial {
                    read: 4,
                    total: 10,
                    pages: false
                })
            )
        );
        assert_eq!(whole("\u{fffc}\n", 4), Outcome::Empty);
    }

    #[test]
    fn outcomes_reach_the_page_tagged() {
        let json = serde_json::to_value(read(
            "t",
            Some(Partial {
                read: 1,
                total: 2,
                pages: true,
            }),
        ))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "status": "read", "text": "t", "partial": { "read": 1, "total": 2, "pages": true } })
        );
        assert_eq!(
            serde_json::to_value(Outcome::Locked).unwrap(),
            serde_json::json!({ "status": "locked" })
        );
    }

    #[tokio::test]
    async fn files_that_are_not_their_kind_or_too_large_never_reach_a_reader() {
        assert_eq!(
            read_file(DocKind::Pdf, b"not a pdf".to_vec())
                .await
                .unwrap(),
            Outcome::Unreadable
        );
        assert!(read_file(DocKind::Pdf, vec![0; MAX_BYTES + 1])
            .await
            .is_err());
    }

    async fn read_file(kind: DocKind, bytes: Vec<u8>) -> WisResult<Outcome> {
        super::read(kind, bytes, 100).await
    }
}
