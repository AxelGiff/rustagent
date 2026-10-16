//! Supported programming languages and tree-sitter syntax highlighting configuration.

use crate::platform;
use freya::code_editor::EditorLanguage;

/// A programming language supported by the editor. Each language knows its
/// tree-sitter grammar, highlights query, file extension and how to run it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SupportedLanguage {
    Python,
    Rust,
    JavaScript,
    TypeScript,
    Html,
    Css,
    C,
    Cpp,
    Java,
    Go,
}

impl SupportedLanguage {
    /// Detects supported language from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "rs" => Some(SupportedLanguage::Rust),
            "py" => Some(SupportedLanguage::Python),
            "js" | "mjs" | "cjs" => Some(SupportedLanguage::JavaScript),
            "ts" | "tsx" => Some(SupportedLanguage::TypeScript),
            "html" | "htm" => Some(SupportedLanguage::Html),
            "css" => Some(SupportedLanguage::Css),
            "c" | "h" => Some(SupportedLanguage::C),
            "cpp" | "cc" | "cxx" | "hpp" => Some(SupportedLanguage::Cpp),
            "java" => Some(SupportedLanguage::Java),
            "go" => Some(SupportedLanguage::Go),
            _ => None,
        }
    }

    /// The file extension (without the dot) used for the editor label and the
    /// temp file written before execution.
    pub fn extension(&self) -> &'static str {
        match self {
            SupportedLanguage::Python => "py",
            SupportedLanguage::Rust => "rs",
            SupportedLanguage::JavaScript => "js",
            SupportedLanguage::TypeScript => "ts",
            SupportedLanguage::Html => "html",
            SupportedLanguage::Css => "css",
            SupportedLanguage::C => "c",
            SupportedLanguage::Cpp => "cpp",
            SupportedLanguage::Java => "java",
            SupportedLanguage::Go => "go",
        }
    }

    /// Build the tree-sitter language + highlights query for the editor.
    pub fn editor_language(&self) -> EditorLanguage {
        match self {
            SupportedLanguage::Python => EditorLanguage::new(
                tree_sitter_python::LANGUAGE,
                tree_sitter_python::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Rust => EditorLanguage::new(
                tree_sitter_rust::LANGUAGE,
                tree_sitter_rust::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::JavaScript => EditorLanguage::new(
                tree_sitter_javascript::LANGUAGE,
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            ),
            SupportedLanguage::TypeScript => EditorLanguage::new(
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
                tree_sitter_typescript::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Html => EditorLanguage::new(
                tree_sitter_html::LANGUAGE,
                tree_sitter_html::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Css => {
                EditorLanguage::new(tree_sitter_css::LANGUAGE, tree_sitter_css::HIGHLIGHTS_QUERY)
            }
            SupportedLanguage::C => {
                EditorLanguage::new(tree_sitter_c::LANGUAGE, tree_sitter_c::HIGHLIGHT_QUERY)
            }
            SupportedLanguage::Cpp => {
                EditorLanguage::new(tree_sitter_cpp::LANGUAGE, tree_sitter_cpp::HIGHLIGHT_QUERY)
            }
            SupportedLanguage::Java => EditorLanguage::new(
                tree_sitter_java::LANGUAGE,
                tree_sitter_java::HIGHLIGHTS_QUERY,
            ),
            SupportedLanguage::Go => {
                EditorLanguage::new(tree_sitter_go::LANGUAGE, tree_sitter_go::HIGHLIGHTS_QUERY)
            }
        }
    }

    /// The shell command used to run a file of this language. The `{file}`
    /// placeholder is replaced with the path to the temp file.
    pub fn run_command(&self, file: &std::path::Path) -> String {
        let file = file.display().to_string();
        // Use the platform temp directory for compiled binaries so the
        // command works on Windows, macOS, and Linux alike.
        let temp_dir = std::env::temp_dir();
        let bin = |name: &str| temp_dir.join(name).display().to_string();
        match self {
            SupportedLanguage::Python => {
                format!("{} {}\n", platform::python_command(), file)
            }
            SupportedLanguage::Rust => {
                let out = bin("main_rs");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::rust_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::JavaScript => {
                format!("{} {}\n", platform::node_runner(), file)
            }
            SupportedLanguage::TypeScript => {
                format!("{} {}\n", platform::ts_runner(), file)
            }
            SupportedLanguage::Html => platform::open_command(&file),
            SupportedLanguage::Css => "echo 'CSS is a stylesheet, nothing to run.'\n".to_string(),
            SupportedLanguage::C => {
                let out = bin("main_c");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::c_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::Cpp => {
                let out = bin("main_cpp");
                format!(
                    "{} {} -o {} && {}\n",
                    platform::cpp_compiler(),
                    file,
                    out,
                    out
                )
            }
            SupportedLanguage::Java => {
                // Java requires the file name to match the public class name.
                format!(
                    "{} {} && {} Main\n",
                    platform::java_compiler(),
                    file,
                    platform::java_runtime()
                )
            }
            SupportedLanguage::Go => {
                format!("{} run {}\n", platform::go_runner(), file)
            }
        }
    }

    /// Detect the language from a user message. Uses token/word matching
    /// to avoid false positives (e.g. "users", "first", "ajs"). Defaults to Python.
    pub fn detect(message: &str) -> SupportedLanguage {
        let lower = message.to_lowercase();
        // Split by non-alphanumeric (except '+' and '#') to get clean tokens
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
            .filter(|s| !s.is_empty())
            .collect();

        let has_word = |target: &str| words.contains(&target);
        let has_any = |targets: &[&str]| targets.iter().any(|&t| has_word(t));

        if has_any(&["javascript", "js", "node", "nodejs"]) {
            SupportedLanguage::JavaScript
        } else if has_any(&["typescript", "ts", "tsx"]) {
            SupportedLanguage::TypeScript
        } else if has_any(&["html", "htm"]) {
            SupportedLanguage::Html
        } else if has_any(&["css", "scss", "sass"]) {
            SupportedLanguage::Css
        } else if has_any(&["c++", "cpp", "cplusplus"]) {
            SupportedLanguage::Cpp
        } else if has_any(&["golang", "go"]) {
            SupportedLanguage::Go
        } else if has_any(&["java"]) {
            SupportedLanguage::Java
        } else if has_any(&["rust", "rs"]) {
            SupportedLanguage::Rust
        } else if has_word("c") {
            SupportedLanguage::C
        } else {
            // Default to Python for anything else (including "python").
            SupportedLanguage::Python
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_defaults_to_python() {
        assert_eq!(
            SupportedLanguage::detect("hello there"),
            SupportedLanguage::Python
        );
        assert_eq!(SupportedLanguage::detect(""), SupportedLanguage::Python);
        assert_eq!(
            SupportedLanguage::detect("write a program"),
            SupportedLanguage::Python
        );
    }

    #[test]
    fn detect_python_explicitly() {
        assert_eq!(
            SupportedLanguage::detect("write python code"),
            SupportedLanguage::Python
        );
        assert_eq!(
            SupportedLanguage::detect("a python script"),
            SupportedLanguage::Python
        );
    }

    #[test]
    fn detect_javascript() {
        assert_eq!(
            SupportedLanguage::detect("write javascript"),
            SupportedLanguage::JavaScript
        );
        assert_eq!(
            SupportedLanguage::detect("a js function"),
            SupportedLanguage::JavaScript
        );
        assert_eq!(
            SupportedLanguage::detect("node script"),
            SupportedLanguage::JavaScript
        );
    }

    #[test]
    fn detect_typescript() {
        assert_eq!(
            SupportedLanguage::detect("write typescript"),
            SupportedLanguage::TypeScript
        );
        assert_eq!(
            SupportedLanguage::detect("ts code"),
            SupportedLanguage::TypeScript
        );
    }

    #[test]
    fn detect_html_and_css() {
        assert_eq!(
            SupportedLanguage::detect("make an html page"),
            SupportedLanguage::Html
        );
        assert_eq!(
            SupportedLanguage::detect("style with css"),
            SupportedLanguage::Css
        );
    }

    #[test]
    fn detect_c_family() {
        assert_eq!(
            SupportedLanguage::detect("write c++ code"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("cpp program"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("cplusplus"),
            SupportedLanguage::Cpp
        );
        assert_eq!(
            SupportedLanguage::detect("write c code"),
            SupportedLanguage::C
        );
    }

    #[test]
    fn detect_go_java_rust() {
        assert_eq!(
            SupportedLanguage::detect("golang program"),
            SupportedLanguage::Go
        );
        assert_eq!(SupportedLanguage::detect("go code"), SupportedLanguage::Go);
        assert_eq!(
            SupportedLanguage::detect("java class"),
            SupportedLanguage::Java
        );
        assert_eq!(
            SupportedLanguage::detect("rust code"),
            SupportedLanguage::Rust
        );
        assert_eq!(
            SupportedLanguage::detect("rs module"),
            SupportedLanguage::Rust
        );
    }

    #[test]
    fn detect_is_case_insensitive() {
        assert_eq!(
            SupportedLanguage::detect("WRITE JAVASCRIPT"),
            SupportedLanguage::JavaScript
        );
    }

    #[test]
    fn from_extension_matches_expected() {
        assert_eq!(
            SupportedLanguage::from_extension("rs"),
            Some(SupportedLanguage::Rust)
        );
        assert_eq!(
            SupportedLanguage::from_extension("PY"),
            Some(SupportedLanguage::Python)
        );
        assert_eq!(SupportedLanguage::from_extension("unknown"), None);
    }

    #[test]
    fn run_command_css_is_noop_message() {
        let cmd = SupportedLanguage::Css.run_command(std::path::Path::new("/tmp/main.css"));
        assert!(cmd.contains("nothing to run"));
    }

    #[test]
    fn run_command_go_uses_go_run() {
        let cmd = SupportedLanguage::Go.run_command(std::path::Path::new("/tmp/main.go"));
        assert!(cmd.contains(platform::go_runner()));
        assert!(cmd.contains("run"));
    }
}
