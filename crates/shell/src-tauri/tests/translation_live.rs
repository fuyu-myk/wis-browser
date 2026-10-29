//! Live check of the Translation bridge against the real framework. It needs
//! the Japanese and English models and a main run loop, neither of which CI
//! has, so it runs only when asked:
//! `WIS_TRANSLATION_LIVE=1 cargo test -p wis-browser --test translation_live`

#[cfg(target_os = "macos")]
fn main() {
    if std::env::var_os("WIS_TRANSLATION_LIVE").is_none() {
        println!("translation_live: skipped (set WIS_TRANSLATION_LIVE=1 to run)");
        return;
    }
    live::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod live {
    use app_lib::translation::{translate, Run, Strategy, TranslateError, TranslateRequest};

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRunLoopRun();
    }

    fn run_text(text: &str, tag: Option<u32>) -> Run {
        Run {
            text: text.into(),
            tag,
        }
    }

    async fn check() -> Result<(), String> {
        let japanese = app_lib::translation::check("ja-JP")
            .await
            .map_err(|err| format!("check ja-JP: {err}"))?;
        println!("check ja-JP: {japanese:?}");
        if japanese.same_language || japanese.source_name != "Japanese" {
            return Err(format!("unexpected check for ja-JP: {japanese:?}"));
        }
        let british = app_lib::translation::check("en-GB")
            .await
            .map_err(|err| format!("check en-GB: {err}"))?;
        if !british.same_language {
            return Err(format!("en-GB should read as English: {british:?}"));
        }

        let runs = [
            run_text("詳しくは", None),
            run_text("こちらのリンク", Some(1)),
            run_text("から記事をお読みください。", None),
        ];
        let request = TranslateRequest {
            source: "ja",
            target: "en",
            strategy: Strategy::HighFidelity,
            runs: &runs,
            skip: &[],
        };
        let out = translate(&request)
            .await
            .map_err(|err| format!("high fidelity: {err}"))?;
        let text: String = out.iter().map(|run| run.text.as_str()).collect();
        println!("high fidelity: {text}");
        if !out
            .iter()
            .any(|run| run.tag == Some(1) && !run.text.trim().is_empty())
        {
            return Err(format!("the tagged link did not survive: {out:?}"));
        }

        let held = [
            run_text("画面下部の", None),
            run_text("•", Some(0)),
            run_text("を押して、最初の画面に戻ります。", None),
        ];
        let skipped = translate(&TranslateRequest {
            runs: &held,
            skip: &[0],
            ..request
        })
        .await
        .map_err(|err| format!("held object: {err}"))?;
        println!("held object: {skipped:?}");
        if !skipped
            .iter()
            .any(|run| run.tag == Some(0) || run.text.contains('•'))
        {
            return Err(format!("the held object did not come back: {skipped:?}"));
        }

        let unsupported = TranslateRequest {
            source: "xx",
            ..request
        };
        match translate(&unsupported).await {
            Err(TranslateError::Unsupported) => println!("unknown language: Unsupported"),
            other => return Err(format!("unknown language gave {other:?}")),
        }
        Ok(())
    }

    /// The checks run on a worker while the main thread serves the run loop,
    /// as AppKit's does in the browser.
    pub fn run() {
        std::thread::spawn(|| {
            // A panic would otherwise leave the main thread in the run loop.
            let outcome = std::panic::catch_unwind(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("a tokio runtime")
                    .block_on(check())
            });
            match outcome {
                Ok(Ok(())) => {
                    println!("translation_live: ok");
                    std::process::exit(0);
                }
                Ok(Err(err)) => {
                    eprintln!("translation_live: FAILED: {err}");
                    std::process::exit(1);
                }
                Err(_) => {
                    eprintln!("translation_live: FAILED: the check panicked");
                    std::process::exit(1);
                }
            }
        });
        // SAFETY: called once, on the main thread, which then serves the loop
        // until the worker exits the process.
        unsafe { CFRunLoopRun() };
    }
}
