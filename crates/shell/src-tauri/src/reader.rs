//! Reader mode: the in-page article transform shared by full-page reader
//! (a tab webview) and the Peek link preview.
//!
//! The script runs entirely inside the target webview and rewrites the
//! document into a clean article via `document.write`, so no untrusted markup
//! ever reaches the privileged chrome. Exiting reader mode reloads the live
//! page (`location.reload()`), which restores the original document.

/// In-page reader transform. Evaluated inside the target webview only.
pub fn reader_script() -> &'static str {
    READER_JS
}

const READER_JS: &str = r#"(function () {
  try {
    function textLen(el) { return ((el && (el.innerText || el.textContent)) || '').replace(/\s+/g, ' ').trim().length; }
    var best = null, bestScore = 0;
    var named = document.querySelectorAll('article, main, [role=main], [itemprop=articleBody], .post, .article, .entry-content, .post-content, #content, .content');
    Array.prototype.slice.call(named).forEach(function (c) { var s = textLen(c); if (s > bestScore) { bestScore = s; best = c; } });
    if (!best || bestScore < 200) {
      Array.prototype.slice.call(document.querySelectorAll('div, section')).forEach(function (c) {
        var ps = c.querySelectorAll('p'); if (ps.length < 2) return;
        var s = 0; for (var i = 0; i < ps.length; i++) s += textLen(ps[i]);
        if (s > bestScore) { bestScore = s; best = c; }
      });
    }
    if (!best) best = document.body;
    var tight = best.querySelector('#mw-content-text, [itemprop=articleBody]');
    if (tight && textLen(tight) > bestScore * 0.5) { best = tight; bestScore = textLen(tight); }
    var title = (document.title || '').trim();
    var clone = best.cloneNode(true);
    ['script', 'style', 'link', 'noscript', 'iframe', 'form', 'nav', 'aside', 'header', 'footer', 'button', 'svg', '[role=navigation]', '.mw-editsection', '.navbox', '.vector-page-toolbar', '.mw-indicators', '.mw-jump-link', '.reflist', '.mw-references-wrap', '.infobox', '.sidebar', '.metadata', '.ambox', '.noprint', '.mw-empty-elt', '.shortdescription', '.hatnote'].forEach(function (sel) {
      try { Array.prototype.slice.call(clone.querySelectorAll(sel)).forEach(function (n) { n.remove(); }); } catch (e) {}
    });
    Array.prototype.slice.call(clone.querySelectorAll('*')).forEach(function (n) {
      var keep = ['href', 'src', 'alt'];
      Array.prototype.slice.call(n.attributes || []).forEach(function (a) { if (keep.indexOf(a.name) < 0) n.removeAttribute(a.name); });
    });
    // Match the page's own light/dark rather than the webview's
    // prefers-color-scheme.
    function rgb(c) { var m = (c || '').match(/rgba?\(([^)]+)\)/); if (!m) return null; var p = m[1].split(',').map(parseFloat); return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 }; }
    function pageBg() { var els = [document.body, document.documentElement]; for (var i = 0; i < els.length; i++) { if (!els[i]) continue; var c = rgb(getComputedStyle(els[i]).backgroundColor); if (c && c.a > 0) return c; } return { r: 255, g: 255, b: 255, a: 1 }; }
    var pc = pageBg();
    var dark = (0.299 * pc.r + 0.587 * pc.g + 0.114 * pc.b) < 128;
    var bgCol = dark ? '#16161a' : '#fbfbfd', fgCol = dark ? '#e9e9ee' : '#1d1d22', linkCol = dark ? '#9aa4ff' : '#6b77f8';
    var css = '*{box-sizing:border-box}html,body{margin:0;padding:0;background:' + bgCol + ';color:' + fgCol + '}body{font:17px/1.65 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}main.kbreader{max-width:680px;margin:0 auto;padding:40px 24px 80px}main.kbreader h1.kbreader-title{font-size:28px;line-height:1.25;margin:0 0 24px}main.kbreader img{max-width:100%;height:auto;border-radius:8px}main.kbreader p{margin:0 0 18px}main.kbreader a{color:' + linkCol + '}';
    var safeTitle = title.replace(/</g, '&lt;');
    var head = '<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><style>' + css + '</style>';
    var body = '<main class=kbreader>' + (title ? ('<h1 class=kbreader-title>' + safeTitle + '</h1>') : '') + clone.innerHTML + '</main>';
    // Rebuild the document tree via the DOM rather than document.write.
    document.documentElement.innerHTML = '<head>' + head + '</head><body>' + body + '</body>';
    document.title = title;
  } catch (e) {}
})();"#;

#[cfg(test)]
mod tests {
    use super::reader_script;

    #[test]
    fn reader_script_is_the_shared_article_transform() {
        // The same transform backs full-page reader and the Peek preview;
        // both call `reader_script()`, so this locks its identity.
        let js = reader_script();
        assert!(
            js.contains("kbreader"),
            "reader transform emits the kbreader article shell"
        );
        // Rebuilds via the DOM.
        assert!(
            js.contains("documentElement.innerHTML"),
            "reader transform rewrites the document tree via the DOM"
        );
        assert!(
            !js.contains("document.write("),
            "reader transform must not call document.write()"
        );
    }
}
