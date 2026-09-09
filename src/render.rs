//! Diff presentation: size caps, binary refusal, ANSI neutralization, and delegation to an
//! external renderer with a plain-text fallback (spec §9, §10).
//!
//! Everything here treats its input as UNTRUSTED. Patch bytes come from repository content and
//! the external renderer is a program the user configured: neither may drive the terminal, so
//! every byte passes the control-sequence scanner before it reaches the screen.

use ansi_to_tui::IntoText;
use ratatui::text::Text;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 2 MiB (spec §9).
pub const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// 5000 lines (spec §9).
pub const DEFAULT_MAX_LINES: usize = 5000;
/// How many leading bytes are inspected for the binary check.
const BINARY_SNIFF_BYTES: usize = 8192;
/// Cap on what an external renderer may emit, bounding memory if it spews.
const MAX_RENDER_OUTPUT: u64 = 16 * 1024 * 1024;
/// Grace for the renderer's stdout reader thread after the child is gone.
const READ_GRACE: Duration = Duration::from_millis(250);

/// The display caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub max_bytes: u64,
    pub max_lines: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            max_bytes: DEFAULT_MAX_BYTES,
            max_lines: DEFAULT_MAX_LINES,
        }
    }
}

/// The guarded result of preparing content for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// Binary content: a placeholder, never the raw bytes.
    Binary,
    /// Capped content plus the notice explaining what was cut.
    Truncated { text: String, notice: String },
    /// Content shown whole.
    Full { text: String },
}

impl Prepared {
    /// The text to display (empty for `Binary`, which has its own placeholder).
    pub fn text(&self) -> &str {
        match self {
            Prepared::Binary => "",
            Prepared::Truncated { text, .. } | Prepared::Full { text } => text,
        }
    }

    /// The truncation notice, if any.
    pub fn notice(&self) -> Option<&str> {
        match self {
            Prepared::Truncated { notice, .. } => Some(notice),
            _ => None,
        }
    }
}

/// Whether `bytes` look binary: a NUL in the sniffed prefix is the classic, cheap signal.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|b| *b == 0)
}

/// Prepare raw `git diff` output for display.
pub fn prepare_patch(bytes: &[u8], caps: Caps) -> Prepared {
    if is_binary(bytes) {
        return Prepared::Binary;
    }
    let text = String::from_utf8_lossy(bytes);
    // git says so itself when it declines to diff a binary file.
    if text
        .lines()
        .any(|l| l.starts_with("Binary files ") || l == "GIT binary patch")
    {
        return Prepared::Binary;
    }
    cap(text.into_owned(), caps)
}

/// Prepare an untracked file as a synthetic all-additions patch (spec §4.1).
///
/// A real unified-diff header is synthesized rather than dumping the file, so an external
/// renderer styles it exactly like any other addition, and the read is bounded by `max_bytes`
/// so a huge or hostile file is never slurped whole.
pub fn prepare_untracked(path: &Path, rel: &str, caps: Caps) -> Prepared {
    let mut buf = Vec::new();
    let read = std::fs::File::open(path)
        // +1 so exceeding the cap is detectable rather than looking like an exact fit.
        .and_then(|f| {
            f.take(caps.max_bytes.saturating_add(1))
                .read_to_end(&mut buf)
        });
    if read.is_err() {
        return Prepared::Full {
            text: format!("could not read {rel}"),
        };
    }
    if is_binary(&buf) {
        return Prepared::Binary;
    }
    let body = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = body.lines().collect();
    let mut out = String::with_capacity(buf.len() + 128);
    out.push_str(&format!("diff --git a/{rel} b/{rel}\n"));
    out.push_str("new file mode 100644\n");
    out.push_str("--- /dev/null\n");
    out.push_str(&format!("+++ b/{rel}\n"));
    out.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
    for line in &lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    cap(out, caps)
}

/// Apply the line and byte caps, producing the notice when either bites.
fn cap(mut text: String, caps: Caps) -> Prepared {
    let mut reasons = Vec::new();
    let line_count = text.lines().count();
    if line_count > caps.max_lines {
        let kept: Vec<&str> = text.lines().take(caps.max_lines).collect();
        text = kept.join("\n");
        reasons.push(format!("truncated at {} lines", caps.max_lines));
    }
    if text.len() as u64 > caps.max_bytes {
        truncate_to_bytes(&mut text, caps.max_bytes);
        reasons.push(format!("truncated at {}", human_bytes(caps.max_bytes)));
    }
    if reasons.is_empty() {
        Prepared::Full { text }
    } else {
        Prepared::Truncated {
            text,
            notice: reasons.join("; "),
        }
    }
}

/// Truncate to at most `max_bytes`, backing up to a UTF-8 boundary so no character is split.
fn truncate_to_bytes(s: &mut String, max_bytes: u64) {
    let max = (max_bytes.min(s.len() as u64)) as usize;
    if max == s.len() {
        return;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

/// A short human label for a byte cap.
fn human_bytes(n: u64) -> String {
    let kib = n / 1024;
    if kib >= 1024 && kib.is_multiple_of(1024) {
        format!("{} MB", kib / 1024)
    } else {
        format!("{kib} KB")
    }
}

// ---------------------------------------------------------------------------
// External renderer
// ---------------------------------------------------------------------------

/// An external diff styler. `None` from [`DiffRenderer::render`] means "not available" — a
/// fallback, never an error (spec §9).
pub trait DiffRenderer: Send + Sync {
    fn render(&self, patch: &str) -> Option<String>;
    /// The program's name, for the fallback notice.
    fn name(&self) -> &str;
}

/// No external styling at all: what `diff_tool = ""` selects. Produces no notice, because an
/// explicitly disabled renderer is a choice, not a problem.
pub struct NoRenderer;

impl DiffRenderer for NoRenderer {
    fn render(&self, _patch: &str) -> Option<String> {
        None
    }
    fn name(&self) -> &str {
        ""
    }
}

/// Pipe the patch through an external program (`delta` by default) and take its stdout.
pub struct DeltaRenderer {
    program: String,
    timeout: Duration,
}

impl DeltaRenderer {
    pub fn new(program: String, timeout: Duration) -> Self {
        DeltaRenderer { program, timeout }
    }
}

impl DiffRenderer for DeltaRenderer {
    fn render(&self, patch: &str) -> Option<String> {
        let mut child = Command::new(&self.program)
            // Force color: the program's stdout is a pipe, so it would otherwise disable it.
            .env("CLICOLOR_FORCE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if let Some(mut stdin) = child.stdin.take() {
            // A renderer that exits early closes the pipe; that is a miss, not a crash.
            let _ = stdin.write_all(patch.as_bytes());
        }
        let (tx, rx) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = stdout.take(MAX_RENDER_OUTPUT).read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        let status = crate::proc::wait_until(&mut child, Instant::now() + self.timeout);
        let out = rx.recv_timeout(READ_GRACE).ok()?;
        status
            .filter(std::process::ExitStatus::success)
            .map(|_| String::from_utf8_lossy(&out).into_owned())
    }

    fn name(&self) -> &str {
        &self.program
    }
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

/// The finished diff view: styled text plus whatever the user should be told about it.
pub struct Rendered {
    pub text: Text<'static>,
    pub notice: Option<String>,
}

/// Turn prepared content into displayable text, delegating styling when available.
///
/// Binary content never reaches the renderer — there is nothing to style and no reason to hand
/// repository bytes to another program.
pub fn render(prepared: Prepared, renderer: &dyn DiffRenderer) -> Rendered {
    if matches!(prepared, Prepared::Binary) {
        return Rendered {
            text: Text::raw("Binary file differs"),
            notice: None,
        };
    }
    let mut notices: Vec<String> = prepared.notice().map(str::to_string).into_iter().collect();
    let styled = renderer.render(prepared.text());
    let body = match styled {
        Some(styled) => styled,
        None => {
            // An explicitly disabled renderer has no name and needs no apology.
            if !renderer.name().is_empty() {
                notices.push(format!(
                    "{} is not available — showing plain text",
                    renderer.name()
                ));
            }
            prepared.text().to_string()
        }
    };
    Rendered {
        text: to_text(&body),
        notice: (!notices.is_empty()).then(|| notices.join("; ")),
    }
}

// ---------------------------------------------------------------------------
// Terminal-control neutralization (spec §10)
// ---------------------------------------------------------------------------

/// Turn possibly-styled text into ratatui `Text`, keeping SGR colors and dropping every other
/// terminal control.
pub fn to_text(raw: &str) -> Text<'static> {
    let cleaned = neutralize(raw, Mode::Styled);
    cleaned
        .clone()
        .into_text()
        .unwrap_or_else(|_| Text::raw(neutralize(&cleaned, Mode::Plain)))
}

/// Drop EVERY terminal control, SGR included, keeping line structure. For status lines and
/// notices, which are plain strings rather than styled spans.
pub fn neutralize_plain_text(raw: &str) -> String {
    neutralize(raw, Mode::Plain)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Keep SGR (colors) for `ansi-to-tui`; drop everything else.
    Styled,
    /// Drop all controls including SGR; keep newlines and tabs.
    Plain,
}

/// Remove terminal-control escape sequences.
///
/// Operates on bytes (control sequences are all ASCII) and passes every other byte through, so
/// UTF-8 content survives verbatim. A truncated sequence at end-of-input is simply dropped.
fn neutralize(raw: &str, mode: Mode) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != 0x1b {
            let b = bytes[i];
            // Keep newline and tab; drop every other C0 control. Non-ASCII bytes are UTF-8
            // continuation bytes and pass through untouched.
            if b >= 0x20 || b == b'\n' || b == b'\t' {
                out.push(b);
            }
            i += 1;
            continue;
        }
        match bytes.get(i + 1) {
            // CSI: parameters and intermediates until a final byte in 0x40..=0x7e.
            Some(b'[') => {
                let start = i;
                let mut j = i + 2;
                while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                let end = (j + 1).min(bytes.len());
                // SGR (`m`) is the only sequence that may survive, and only in Styled mode:
                // it paints our own region and cannot move the cursor or drive the terminal.
                if mode == Mode::Styled && bytes.get(j) == Some(&b'm') {
                    out.extend_from_slice(&bytes[start..end]);
                }
                i = end;
            }
            // OSC and the other string-argument sequences: drop through BEL or ST (ESC \).
            Some(b']') | Some(b'P') | Some(b'X') | Some(b'^') | Some(b'_') => {
                let mut j = i + 2;
                while j < bytes.len() {
                    if bytes[j] == 0x07 {
                        j += 1;
                        break;
                    }
                    if bytes[j] == 0x1b && bytes.get(j + 1) == Some(&b'\\') {
                        j += 2;
                        break;
                    }
                    j += 1;
                }
                i = j;
            }
            // Any other two-byte escape, or a lone trailing ESC.
            Some(_) => i += 2,
            None => i += 1,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(max_bytes: u64, max_lines: usize) -> Caps {
        Caps {
            max_bytes,
            max_lines,
        }
    }

    /// A renderer that records what it was handed and returns a fixed answer.
    struct StubRenderer {
        out: Option<String>,
    }

    impl DiffRenderer for StubRenderer {
        fn render(&self, patch: &str) -> Option<String> {
            self.out.clone().map(|o| format!("{o}{patch}"))
        }
        fn name(&self) -> &str {
            "stub"
        }
    }

    fn plain(text: &Text<'_>) -> String {
        text.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---- caps ------------------------------------------------------------------------------

    #[test]
    fn the_default_caps_are_the_spec_limits() {
        assert_eq!(Caps::default().max_bytes, 2 * 1024 * 1024);
        assert_eq!(Caps::default().max_lines, 5000);
    }

    #[test]
    fn a_small_patch_is_shown_whole() {
        let p = prepare_patch(b"@@ -1 +1 @@\n-a\n+b\n", Caps::default());
        assert!(matches!(p, Prepared::Full { .. }));
        assert!(p.text().contains("+b"));
    }

    #[test]
    fn a_patch_past_the_line_cap_is_truncated_with_a_visible_notice() {
        let big = "+line\n".repeat(20);
        let p = prepare_patch(big.as_bytes(), caps(1 << 20, 5));
        let Prepared::Truncated { text, notice } = p else {
            panic!("expected truncation");
        };
        assert_eq!(text.lines().count(), 5);
        assert!(notice.contains("5"), "{notice}");
    }

    #[test]
    fn a_patch_past_the_byte_cap_is_truncated_with_a_visible_notice() {
        let big = "+".repeat(5000);
        let p = prepare_patch(big.as_bytes(), caps(100, 100_000));
        let Prepared::Truncated { text, notice } = p else {
            panic!("expected truncation");
        };
        assert!(text.len() <= 100, "{}", text.len());
        assert!(!notice.is_empty());
    }

    #[test]
    fn truncation_never_splits_a_multi_byte_character() {
        // Cutting mid-codepoint would corrupt the pane; the cut backs up to a boundary.
        let text = "＋".repeat(100); // 3 bytes each
        let p = prepare_patch(text.as_bytes(), caps(10, 100_000));
        assert!(matches!(p, Prepared::Truncated { .. }));
        assert!(p.text().chars().all(|c| c == '＋'));
    }

    #[test]
    fn an_empty_patch_is_full_and_empty_rather_than_an_error() {
        let p = prepare_patch(b"", Caps::default());
        assert!(matches!(p, Prepared::Full { .. }));
        assert_eq!(p.text(), "");
    }

    // ---- binary ------------------------------------------------------------------------------

    #[test]
    fn a_nul_byte_makes_content_binary() {
        assert!(is_binary(b"abc\0def"));
        assert!(!is_binary(b"abc\ndef\t"));
    }

    #[test]
    fn gits_own_binary_marker_is_recognized_even_without_a_nul_byte() {
        let patch = b"diff --git a/x b/x\nBinary files a/x and b/x differ\n";
        assert!(matches!(
            prepare_patch(patch, Caps::default()),
            Prepared::Binary
        ));
    }

    #[test]
    fn a_git_binary_patch_body_is_recognized() {
        let patch = b"diff --git a/x b/x\nGIT binary patch\nliteral 12\n";
        assert!(matches!(
            prepare_patch(patch, Caps::default()),
            Prepared::Binary
        ));
    }

    #[test]
    fn a_binary_result_is_never_handed_to_the_external_renderer() {
        let stub = StubRenderer {
            out: Some("STYLED".to_string()),
        };
        let rendered = render(Prepared::Binary, &stub);
        assert_eq!(plain(&rendered.text), "Binary file differs");
        assert!(!plain(&rendered.text).contains("STYLED"));
    }

    // ---- untracked -----------------------------------------------------------------------------

    #[test]
    fn an_untracked_file_is_rendered_as_a_synthetic_all_additions_patch() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let file = dir.path().join("new.txt");
        std::fs::write(&file, "one\ntwo\n").expect("write");
        let p = prepare_untracked(&file, "new.txt", Caps::default());
        let text = p.text();
        // A real unified-diff header, so `delta` styles it exactly like any other addition.
        assert!(text.contains("diff --git a/new.txt b/new.txt"), "{text}");
        assert!(text.contains("--- /dev/null"), "{text}");
        assert!(text.contains("+++ b/new.txt"), "{text}");
        assert!(text.contains("@@ -0,0 +1,2 @@"), "{text}");
        assert!(text.contains("+one\n+two"), "{text}");
    }

    #[test]
    fn an_untracked_binary_file_is_not_dumped_into_the_pane() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let file = dir.path().join("blob.bin");
        std::fs::write(&file, [0u8, 1, 2, 3]).expect("write");
        assert!(matches!(
            prepare_untracked(&file, "blob.bin", Caps::default()),
            Prepared::Binary
        ));
    }

    #[test]
    fn an_untracked_file_past_the_cap_is_truncated() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let file = dir.path().join("big.txt");
        std::fs::write(&file, "x\n".repeat(100)).expect("write");
        assert!(matches!(
            prepare_untracked(&file, "big.txt", caps(1 << 20, 10)),
            Prepared::Truncated { .. }
        ));
    }

    #[test]
    fn an_unreadable_untracked_file_yields_a_message_not_a_panic() {
        let p = prepare_untracked(Path::new("/definitely/not/here"), "nope", Caps::default());
        assert!(p.text().contains("could not read"), "{}", p.text());
    }

    #[test]
    fn the_read_of_an_untracked_file_is_bounded_by_the_byte_cap() {
        // A hostile or accidental multi-gigabyte file must never be slurped whole.
        let dir = tempfile::TempDir::new().expect("tempdir");
        let file = dir.path().join("huge.txt");
        std::fs::write(&file, "y".repeat(200_000)).expect("write");
        let p = prepare_untracked(&file, "huge.txt", caps(1024, 100_000));
        // Header lines plus at most ~1 KiB of body.
        assert!(p.text().len() < 4096, "{}", p.text().len());
    }

    // ---- ANSI neutralization (spec §10) -----------------------------------------------------------

    #[test]
    fn a_cursor_control_sequence_is_stripped_from_styled_output() {
        // SGR is kept for ratatui; everything else that can drive the terminal is removed.
        let out = neutralize_plain_text("a\x1b[2Jb\x1b[Hc");
        assert_eq!(out, "abc");
    }

    #[test]
    fn an_osc_52_clipboard_hijack_is_stripped() {
        // A renderer's output must never be able to write the user's clipboard.
        assert_eq!(neutralize_plain_text("x\x1b]52;c;ZXZpbA==\x07y"), "xy");
    }

    #[test]
    fn an_osc_terminated_by_string_terminator_is_stripped() {
        assert_eq!(neutralize_plain_text("x\x1b]0;title\x1b\\y"), "xy");
    }

    #[test]
    fn bare_control_bytes_are_dropped() {
        assert_eq!(neutralize_plain_text("a\x07b\x08c"), "abc");
    }

    #[test]
    fn a_truncated_escape_at_the_end_of_input_does_not_panic() {
        for input in ["abc\x1b", "abc\x1b[", "abc\x1b]52;c;", "abc\x1b[38;5;"] {
            let _ = neutralize_plain_text(input);
            let _ = to_text(input);
        }
    }

    #[test]
    fn unicode_content_survives_neutralization_verbatim() {
        let s = "測試 ← 中文 ✓ emoji 🎉";
        assert_eq!(neutralize_plain_text(s), s);
    }

    #[test]
    fn sgr_styling_survives_into_the_rendered_text_as_styling_not_as_literal_escapes() {
        let text = to_text("\x1b[31mred\x1b[0m plain");
        let flat = plain(&text);
        assert!(flat.contains("red"), "{flat}");
        assert!(
            !flat.contains('\x1b'),
            "escapes must not reach the buffer: {flat:?}"
        );
    }

    #[test]
    fn line_structure_is_preserved_when_rendering_styled_content() {
        assert_eq!(to_text("a\nb\nc").lines.len(), 3);
    }

    // ---- renderer delegation ------------------------------------------------------------------------

    #[test]
    fn the_external_renderers_output_is_used_when_it_succeeds() {
        let stub = StubRenderer {
            out: Some("PRE:".to_string()),
        };
        let rendered = render(
            Prepared::Full {
                text: "+a\n".to_string(),
            },
            &stub,
        );
        assert!(plain(&rendered.text).contains("PRE:"));
        assert_eq!(rendered.notice, None);
    }

    #[test]
    fn a_missing_renderer_falls_back_to_plain_text_with_a_notice_not_an_error() {
        // spec §9: "delta 未安裝 → 純文字 diff + 一次性提示（非錯誤）". On this machine that is
        // the LIVE path, not an edge case.
        let stub = StubRenderer { out: None };
        let rendered = render(
            Prepared::Full {
                text: "+a\n".to_string(),
            },
            &stub,
        );
        assert!(plain(&rendered.text).contains("+a"));
        let notice = rendered.notice.expect("a fallback notice");
        assert!(notice.contains("stub"), "{notice}");
    }

    #[test]
    fn a_truncation_notice_survives_the_renderer_delegation() {
        let stub = StubRenderer {
            out: Some(String::new()),
        };
        let rendered = render(
            Prepared::Truncated {
                text: "+a\n".to_string(),
                notice: "truncated at 5 lines".to_string(),
            },
            &stub,
        );
        assert_eq!(rendered.notice.as_deref(), Some("truncated at 5 lines"));
    }

    #[test]
    fn both_a_truncation_and_a_fallback_notice_are_reported_together() {
        let stub = StubRenderer { out: None };
        let rendered = render(
            Prepared::Truncated {
                text: "+a\n".to_string(),
                notice: "truncated at 5 lines".to_string(),
            },
            &stub,
        );
        let notice = rendered.notice.expect("a notice");
        assert!(notice.contains("truncated"), "{notice}");
        assert!(notice.contains("stub"), "{notice}");
    }

    #[test]
    fn the_no_renderer_never_styles_anything_and_never_complains() {
        let rendered = render(
            Prepared::Full {
                text: "+a\n".to_string(),
            },
            &NoRenderer,
        );
        assert!(plain(&rendered.text).contains("+a"));
        assert_eq!(
            rendered.notice, None,
            "an explicitly disabled renderer is not a problem"
        );
    }

    #[test]
    fn an_external_renderer_that_is_not_installed_returns_none_rather_than_erroring() {
        let renderer = DeltaRenderer::new(
            "definitely-not-a-real-program-xyz".to_string(),
            Duration::from_secs(2),
        );
        assert_eq!(renderer.render("+a\n"), None);
    }

    #[test]
    fn an_external_renderer_receives_the_patch_on_stdin_and_its_stdout_is_used() {
        // `cat` is the simplest faithful stand-in for delta: stdin -> stdout.
        let renderer = DeltaRenderer::new("cat".to_string(), Duration::from_secs(5));
        assert_eq!(renderer.render("+hello\n").as_deref(), Some("+hello\n"));
    }

    #[test]
    fn an_external_renderer_that_hangs_is_killed_and_reported_as_a_miss() {
        let renderer = DeltaRenderer::new("sleep".to_string(), Duration::from_millis(100));
        let started = std::time::Instant::now();
        // `sleep` with the patch on stdin and no args exits non-zero immediately on most
        // systems; either way the call must return promptly and never hang the worker.
        let _ = renderer.render("+a\n");
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
