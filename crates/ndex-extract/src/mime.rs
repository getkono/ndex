//! MIME-type detection and text/binary classification (PRD §4.4, §4.8).

use ndex_core::path::NdexPath;

/// Number of leading bytes inspected by the text/binary heuristic (PRD §4.8).
pub const TEXT_SNIFF_BYTES: usize = 8192;

/// Detect a file's MIME type: magic bytes (`infer`) → extension (`mime_guess`) →
/// known-filename table → text/binary heuristic (PRD §4.4, §4.8).
///
/// When magic bytes and extension disagree, magic bytes win (PRD §4.4).
pub fn detect(path: &NdexPath, bytes: &[u8]) -> String {
    if let Some(kind) = infer::get(bytes) {
        return kind.mime_type().to_string();
    }
    if let Some(name) = file_name(path)
        && let Some(guess) = mime_guess::from_path(name).first()
    {
        return guess.essence_str().to_string();
    }
    if let Some(known) = known_filename(path) {
        return known.to_string();
    }
    if is_text(bytes) {
        "text/plain".to_string()
    } else {
        "application/octet-stream".to_string()
    }
}

/// Git-style text/binary heuristic: a NUL byte within the first [`TEXT_SNIFF_BYTES`] ⇒ binary
/// (PRD §4.8).
pub fn is_text(bytes: &[u8]) -> bool {
    let n = bytes.len().min(TEXT_SNIFF_BYTES);
    !bytes[..n].contains(&0)
}

/// Map common extensionless / fixed filenames to MIME types (PRD §4.8 known-file table).
pub fn known_filename(path: &NdexPath) -> Option<&'static str> {
    Some(match file_name(path)? {
        "Makefile" | "makefile" | "GNUmakefile" => "text/x-makefile",
        "Dockerfile" => "text/x-dockerfile",
        "CMakeLists.txt" => "text/x-cmake",
        "Vagrantfile" | "Rakefile" | "Gemfile" | "Procfile" | "Jenkinsfile" => "text/plain",
        _ => return None,
    })
}

/// Map a filename extension to a code language name for the tree-sitter router (PRD §4.4).
///
/// Pragmatic v0.1 defaults: `.h` → C (not C++), `.m` → Objective-C.
pub fn extension_language(path: &NdexPath) -> Option<&'static str> {
    let name = file_name(path)?;
    let ext = name.rsplit_once('.')?.1;
    Some(match ext {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "go" => "go",
        "sh" | "bash" => "bash",
        _ => return None,
    })
}

/// The final path component, if the path is valid UTF-8 and non-empty.
fn file_name(path: &NdexPath) -> Option<&str> {
    let s = std::str::from_utf8(path.as_bytes()).ok()?;
    s.rsplit('/').next().filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NdexPath {
        NdexPath::new(s.as_bytes().to_vec())
    }

    #[test]
    fn text_vs_binary() {
        assert!(is_text(b"hello world\n"));
        assert!(!is_text(b"PK\x03\x04\x00binary"));
        assert!(is_text(b"")); // empty is text
    }

    #[test]
    fn detects_png_by_magic() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(detect(&p("whatever.txt"), png), "image/png");
    }

    #[test]
    fn detects_text_by_extension() {
        assert_eq!(detect(&p("notes.txt"), b"just words"), "text/plain");
    }

    #[test]
    fn binary_without_hints_is_octet_stream() {
        assert_eq!(
            detect(&p("blob"), b"\x00\x01\x02\x03"),
            "application/octet-stream"
        );
    }

    #[test]
    fn known_filenames_and_extensions() {
        assert_eq!(known_filename(&p("/src/Makefile")), Some("text/x-makefile"));
        assert_eq!(known_filename(&p("/src/main.rs")), None);
        assert_eq!(extension_language(&p("/src/main.rs")), Some("rust"));
        assert_eq!(extension_language(&p("/x/y.unknownext")), None);
    }
}
