//! The browser binary started as a document reader (`document_text.rs`).
#![cfg(target_os = "macos")]

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn reader(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wis-browser"))
        .arg("--wis-read-document")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the reader");
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn outcome(args: &[&str], input: &[u8]) -> Value {
    let out = reader(args, input);
    assert!(out.status.success(), "reader failed: {:?}", out.status);
    serde_json::from_slice(&out.stdout).expect("JSON from the reader")
}

/// A PDF with one line of Helvetica text per page.
fn pdf(pages: &[&str]) -> Vec<u8> {
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + i * 2))
        .collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    for (i, text) in pages.iter().enumerate() {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + i * 2
        ));
        let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn a_pdf_is_read_page_by_page_up_to_the_limit() {
    let doc = pdf(&["First page", "Second page", "Third page"]);
    assert_eq!(
        outcome(&["pdf", "1000"], &doc),
        json!({ "status": "read", "text": "First page\n\nSecond page\n\nThird page", "partial": null })
    );
    assert_eq!(
        outcome(&["pdf", "25"], &doc),
        json!({
            "status": "read",
            "text": "First page\n\nSecond page",
            "partial": { "read": 2, "total": 3, "pages": true }
        })
    );
}

#[test]
fn a_pdf_without_text_is_empty_and_rich_text_comes_out_plain() {
    assert_eq!(
        outcome(&["pdf", "1000"], &pdf(&[""])),
        json!({ "status": "empty" })
    );
    let rtf = br"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0 Minutes\par Agreed.\par}";
    assert_eq!(
        outcome(&["rtf", "1000"], rtf),
        json!({ "status": "read", "text": "Minutes\nAgreed.", "partial": null })
    );
}

#[test]
fn a_reader_drops_what_it_inherits() {
    let mut ends = [0; 2];
    // SAFETY: a pipe whose ends are plain descriptors, closed below.
    assert_eq!(unsafe { libc::pipe(ends.as_mut_ptr()) }, 0);
    let [read_end, write_end] = ends;
    let mut child = Command::new(env!("CARGO_BIN_EXE_wis-browser"))
        .args(["--wis-read-document", "rtf", "100"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the reader");
    let mut byte = 0u8;
    // SAFETY: our own descriptors; the read blocks until every write end is
    // closed, which happens only once the reader has dropped its copy.
    let got = unsafe {
        libc::close(write_end);
        let got = libc::read(read_end, (&mut byte as *mut u8).cast(), 1);
        libc::close(read_end);
        got
    };
    assert_eq!(got, 0, "the inherited write end was closed");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br"{\rtf1 ok}")
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
}

#[test]
fn a_reader_started_wrongly_refuses() {
    assert_eq!(reader(&["exe", "10"], b"").status.code(), Some(2));
    assert_eq!(reader(&["pdf"], b"").status.code(), Some(2));
}
