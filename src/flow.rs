//! Core chat → editor → terminal flow logic.
//!
//! This module holds the pure, UI-independent logic that connects the chat,
//! editor, and terminal panels. Keeping it separate from the Freya components
//! makes the flow testable without launching a GUI.
//!
//! The functions here model the decisions made when a user sends a message
//! (detect language, decide whether to generate code, extract code from the
//! AI response) and when code is executed (build the temp file and run
//! command). They are intentionally free of any UI state so they can be
//! unit- and integration-tested in isolation.

use crate::SupportedLanguage;

/// Keywords that indicate the user wants code generated. This is intentionally
/// broad: it triggers on write/generate/create/make/code/implement/program/
/// script/function/class.
pub const ACTION_KEYWORDS: &[&str] = &[
    "write",
    "generate",
    "create",
    "make",
    "build",
    "code",
    "implement",
    "program",
    "script",
    "function",
    "class",
];

/// Whether the user wants to clear the editor.
pub fn wants_clear_editor(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("clear editor")
        || lower.contains("clear the editor")
        || lower.contains("empty editor")
        || lower.contains("empty the editor")
        || lower.contains("wipe editor")
        || lower.contains("wipe the editor")
        || lower.contains("reset editor")
        || lower.contains("reset the editor")
}

/// Whether the user wants code generated.
pub fn wants_code(message: &str) -> bool {
    let lower = message.to_lowercase();
    ACTION_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// What to do with an AI response.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorAction {
    /// Insert this code into the editor, switching to the given language.
    Insert {
        language: SupportedLanguage,
        code: String,
    },
    /// Show the raw response in the chat (no code to insert).
    ShowResponse,
}

/// Decide what to do with an AI response given whether the user asked for
/// code. If the user asked for code and the response contains a matching code
/// block, the code is extracted for insertion into the editor; otherwise the
/// raw response is shown in the chat.
pub fn decide_editor_action(
    response: &str,
    wants_code: bool,
    detected_language: SupportedLanguage,
) -> EditorAction {
    if wants_code {
        let code = extract_code_from_response(response, detected_language);
        if !code.is_empty() {
            return EditorAction::Insert {
                language: detected_language,
                code,
            };
        }
    }
    EditorAction::ShowResponse
}

/// The confirmation message shown in chat after inserting code into the editor.
pub fn insertion_confirmation(language: SupportedLanguage) -> String {
    format!(
        "I've inserted the generated **{}** code into the code editor for you. You can review it there and press **Execute Code** to run it in the terminal.",
        language_name(language)
    )
}

/// The temp file name used for a language's source file.
///
/// Java is special-cased: the Java compiler requires the public class name to
/// match the file name, and the conventional public class is `Main`, so the
/// file must be `Main.java` (capital M) for `javac Main.java && java Main` to
/// work. All other languages use the lowercase `main.<ext>` convention.
pub fn temp_source_file(language: SupportedLanguage) -> String {
    if language == SupportedLanguage::Java {
        return "Main.java".to_string();
    }
    format!("main.{}", language.extension())
}

/// Extract the identifier that immediately follows a keyword at the start of a
/// line, e.g. `fn foo(` → `foo`, `def bar():` → `bar`, `class Baz {` → `Baz`.
/// Returns `None` when no keyword is found or no identifier follows it.
fn identifier_after_keyword(line: &str, keywords: &[&str]) -> Option<String> {
    let trimmed = line.trim_start();
    for kw in keywords {
        if let Some(rest) = trimmed.strip_prefix(kw) {
            let rest = rest.trim_start();
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() {
                return Some(ident);
            }
        }
    }
    None
}

/// Extract the text of an HTML `<title>...</title>` tag, if present.
fn html_title(code: &str) -> Option<String> {
    let lower = code.to_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = lower[start..].find("</title>")? + start;
    let title = code[start..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Derive a meaningful base file name (without extension) from the script
/// content, based on the first function/class/struct name found. Returns
/// `None` when no meaningful name can be derived (e.g. prose, or a language
/// like CSS that has no named declarations).
pub fn derive_base_name(code: &str, language: SupportedLanguage) -> Option<String> {
    // HTML has no function/class declarations; use the `<title>` tag instead.
    if language == SupportedLanguage::Html {
        return html_title(code);
    }

    let keywords: &[&str] = match language {
        SupportedLanguage::Python => &["def ", "class "],
        SupportedLanguage::Rust => &["fn ", "struct ", "enum ", "trait "],
        SupportedLanguage::JavaScript => &["function ", "class ", "const ", "let "],
        SupportedLanguage::TypeScript => &["function ", "class ", "interface ", "const ", "let "],
        SupportedLanguage::Html => &[],
        SupportedLanguage::Css => &[],
        SupportedLanguage::C => &["int ", "void ", "char ", "float ", "double "],
        SupportedLanguage::Cpp => &["int ", "void ", "class ", "char ", "float ", "double "],
        SupportedLanguage::Java => &["public class ", "class "],
        SupportedLanguage::Go => &["func "],
    };

    for line in code.lines() {
        if let Some(name) = identifier_after_keyword(line, keywords) {
            return Some(name);
        }
    }
    None
}

/// The file name shown in the editor header and used for the temp file written
/// before execution.
///
/// This derives a meaningful name from the script content (e.g. a Fibonacci
/// script becomes `fibonacci.py`) so the editor title reflects what was
/// written. When no meaningful name can be derived it falls back to the
/// conventional `main.<ext>` name (see [`temp_source_file`]).
///
/// Java is always `Main.java`: the Java compiler requires the public class
/// name to match the file name, and the conventional public class is `Main`.
pub fn derive_file_name(code: &str, language: SupportedLanguage) -> String {
    if language == SupportedLanguage::Java {
        return "Main.java".to_string();
    }
    match derive_base_name(code, language) {
        Some(base) => format!("{}.{}", base, language.extension()),
        None => temp_source_file(language),
    }
}

/// Human-readable name for a language, used in chat messages.
pub fn language_name(lang: SupportedLanguage) -> &'static str {
    match lang {
        SupportedLanguage::Python => "Python",
        SupportedLanguage::Rust => "Rust",
        SupportedLanguage::JavaScript => "JavaScript",
        SupportedLanguage::TypeScript => "TypeScript",
        SupportedLanguage::Html => "HTML",
        SupportedLanguage::Css => "CSS",
        SupportedLanguage::C => "C",
        SupportedLanguage::Cpp => "C++",
        SupportedLanguage::Java => "Java",
        SupportedLanguage::Go => "Go",
    }
}

/// Extract code from the AI response for the given language. Handles markdown
/// code fences (```lang ... ```) and inline backticks.
pub fn extract_code_from_response(response: &str, language: SupportedLanguage) -> String {
    // The language tags we accept in markdown code fences for this language.
    let lang_tags: &[&str] = match language {
        SupportedLanguage::Python => &["python", "py"],
        SupportedLanguage::Rust => &["rust", "rs"],
        SupportedLanguage::JavaScript => &["javascript", "js", "node"],
        SupportedLanguage::TypeScript => &["typescript", "ts", "tsx"],
        SupportedLanguage::Html => &["html", "htm"],
        SupportedLanguage::Css => &["css"],
        SupportedLanguage::C => &["c"],
        SupportedLanguage::Cpp => &["cpp", "c++", "cxx"],
        SupportedLanguage::Java => &["java"],
        SupportedLanguage::Go => &["go", "golang"],
    };

    let mut result = String::new();
    let mut in_code_block = false;
    let mut current_code = String::new();

    for line in response.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code_block {
                // Opening fence. Check if the tag matches our language.
                let tag = trimmed.trim_start_matches("```").trim().to_lowercase();
                let matches = tag.is_empty() || lang_tags.iter().any(|t| tag.starts_with(t));
                if matches {
                    in_code_block = true;
                    current_code.clear();
                }
            } else {
                // Closing fence.
                in_code_block = false;
                if !current_code.trim().is_empty() {
                    result.push_str(&current_code);
                    result.push('\n');
                }
            }
        } else if in_code_block {
            current_code.push_str(line);
            current_code.push('\n');
        }
    }

    // If no matching code block found, try inline backticks.
    if result.is_empty() {
        for (i, part) in response.split('`').enumerate() {
            if i % 2 == 1 && !part.trim().is_empty() {
                // Only take it if it looks like code for this language.
                if looks_like_code(part, language) {
                    result.push_str(part);
                    result.push('\n');
                }
            }
        }
    }

    // Clean up the result.
    if !result.is_empty() {
        result = result.trim_end().to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

/// Heuristic to decide whether a snippet looks like code for the given language.
pub fn looks_like_code(snippet: &str, language: SupportedLanguage) -> bool {
    match language {
        SupportedLanguage::Python => {
            snippet.contains("def ")
                || snippet.contains("import ")
                || snippet.contains("print(")
                || snippet.contains("class ")
                || snippet.contains("if __name__")
        }
        SupportedLanguage::Rust => {
            snippet.contains("fn ")
                || snippet.contains("let ")
                || snippet.contains("use ")
                || snippet.contains("fn main")
        }
        SupportedLanguage::JavaScript => {
            snippet.contains("function ")
                || snippet.contains("const ")
                || snippet.contains("let ")
                || snippet.contains("console.log")
                || snippet.contains("=>")
        }
        SupportedLanguage::TypeScript => {
            snippet.contains("function ")
                || snippet.contains("const ")
                || snippet.contains("let ")
                || snippet.contains("interface ")
                || snippet.contains(": string")
        }
        SupportedLanguage::Html => {
            snippet.contains("<html")
                || snippet.contains("<body")
                || snippet.contains("<div")
                || snippet.contains("<!DOCTYPE")
        }
        SupportedLanguage::Css => {
            snippet.contains("{")
                && (snippet.contains("color")
                    || snippet.contains("margin")
                    || snippet.contains("padding")
                    || snippet.contains("display"))
        }
        SupportedLanguage::C => {
            snippet.contains("#include")
                || snippet.contains("int main")
                || snippet.contains("printf")
        }
        SupportedLanguage::Cpp => {
            snippet.contains("#include")
                || snippet.contains("int main")
                || snippet.contains("std::")
                || snippet.contains("cout")
        }
        SupportedLanguage::Java => {
            snippet.contains("public class")
                || snippet.contains("public static void main")
                || snippet.contains("System.out")
        }
        SupportedLanguage::Go => {
            snippet.contains("package main")
                || snippet.contains("func main")
                || snippet.contains("fmt.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // wants_clear_editor()
    // ------------------------------------------------------------------

    #[test]
    fn clear_editor_phrases_are_detected() {
        for phrase in [
            "clear editor",
            "clear the editor",
            "empty editor",
            "empty the editor",
            "wipe editor",
            "wipe the editor",
            "reset editor",
            "reset the editor",
        ] {
            assert!(wants_clear_editor(phrase), "expected true for: {phrase}");
        }
    }

    #[test]
    fn clear_editor_is_case_insensitive() {
        assert!(wants_clear_editor("CLEAR EDITOR"));
        assert!(wants_clear_editor("Please Clear The Editor"));
    }

    #[test]
    fn non_clear_messages_are_not_detected() {
        assert!(!wants_clear_editor("write a function"));
        assert!(!wants_clear_editor("hello"));
        assert!(!wants_clear_editor(""));
    }

    // ------------------------------------------------------------------
    // wants_code()
    // ------------------------------------------------------------------

    #[test]
    fn action_keywords_trigger_code_generation() {
        for keyword in ACTION_KEYWORDS {
            assert!(
                wants_code(&format!("please {keyword} a program")),
                "expected true for keyword: {keyword}"
            );
        }
    }

    #[test]
    fn wants_code_is_case_insensitive() {
        assert!(wants_code("WRITE a function"));
        assert!(wants_code("Please Generate code"));
    }

    #[test]
    fn plain_questions_do_not_trigger_code() {
        assert!(!wants_code("what is the weather?"));
        assert!(!wants_code("hello there"));
        assert!(!wants_code(""));
    }

    // ------------------------------------------------------------------
    // decide_editor_action() — the chat → editor handoff
    // ------------------------------------------------------------------

    #[test]
    fn inserts_code_when_requested_and_present() {
        let response = "Here:\n```python\ndef foo():\n    return 1\n```";
        let action = decide_editor_action(response, true, SupportedLanguage::Python);
        match action {
            EditorAction::Insert { language, code } => {
                assert_eq!(language, SupportedLanguage::Python);
                assert!(code.contains("def foo():"));
            }
            EditorAction::ShowResponse => panic!("expected Insert"),
        }
    }

    #[test]
    fn shows_response_when_no_code_requested() {
        let response = "Just some explanation, no code.";
        let action = decide_editor_action(response, false, SupportedLanguage::Python);
        assert_eq!(action, EditorAction::ShowResponse);
    }

    #[test]
    fn shows_response_when_code_requested_but_absent() {
        let response = "I can't generate that right now.";
        let action = decide_editor_action(response, true, SupportedLanguage::Python);
        assert_eq!(action, EditorAction::ShowResponse);
    }

    #[test]
    fn inserts_code_with_detected_language() {
        let response = "```rust\nfn main() {}\n```";
        let action = decide_editor_action(response, true, SupportedLanguage::Rust);
        match action {
            EditorAction::Insert { language, code } => {
                assert_eq!(language, SupportedLanguage::Rust);
                assert!(code.contains("fn main()"));
            }
            EditorAction::ShowResponse => panic!("expected Insert"),
        }
    }

    // ------------------------------------------------------------------
    // insertion_confirmation()
    // ------------------------------------------------------------------

    #[test]
    fn confirmation_mentions_language_and_execute() {
        let msg = insertion_confirmation(SupportedLanguage::Python);
        assert!(msg.contains("Python"));
        assert!(msg.contains("Execute Code"));
        assert!(msg.contains("code editor"));
    }

    // ------------------------------------------------------------------
    // temp_source_file() — the editor → terminal handoff
    // ------------------------------------------------------------------

    #[test]
    fn temp_file_uses_language_extension() {
        assert_eq!(temp_source_file(SupportedLanguage::Python), "main.py");
        assert_eq!(temp_source_file(SupportedLanguage::Rust), "main.rs");
        assert_eq!(temp_source_file(SupportedLanguage::JavaScript), "main.js");
        assert_eq!(temp_source_file(SupportedLanguage::Go), "main.go");
    }

    /// Java is special-cased: the file must be `Main.java` so the public class
    /// name matches the file name (required by `javac`).
    #[test]
    fn temp_file_java_uses_capital_main() {
        assert_eq!(temp_source_file(SupportedLanguage::Java), "Main.java");
    }

    /// The editor → terminal handoff for Java must be self-consistent: the
    /// temp file written from the editor (`Main.java`) must match the class
    /// name the run command compiles and executes (`Main`). If these diverge,
    /// `javac` fails because the public class name would not match the file
    /// name, so the terminal could never run the editor's code.
    #[test]
    fn java_editor_to_terminal_handoff_is_consistent() {
        let file = temp_source_file(SupportedLanguage::Java);
        assert_eq!(file, "Main.java");

        let command = SupportedLanguage::Java.run_command(std::path::Path::new(&file));
        // The command must compile the exact file we wrote from the editor.
        assert!(command.contains("Main.java"), "command: {command}");
        // And it must run the class whose name matches that file (Main).
        assert!(command.contains("java Main"), "command: {command}");
    }

    // ------------------------------------------------------------------
    // derive_base_name() / derive_file_name() — the editor title
    // ------------------------------------------------------------------

    /// A Python script with a named function derives a meaningful base name.
    #[test]
    fn derive_base_name_python_function() {
        let code = "def fibonacci(n):\n    return n\n";
        assert_eq!(
            derive_base_name(code, SupportedLanguage::Python),
            Some("fibonacci".to_string())
        );
    }

    /// A Python class name is derived too.
    #[test]
    fn derive_base_name_python_class() {
        let code = "class Greeter:\n    pass\n";
        assert_eq!(
            derive_base_name(code, SupportedLanguage::Python),
            Some("Greeter".to_string())
        );
    }

    /// Rust `fn` names are derived.
    #[test]
    fn derive_base_name_rust_fn() {
        let code = "fn main() {}\n";
        assert_eq!(
            derive_base_name(code, SupportedLanguage::Rust),
            Some("main".to_string())
        );
    }

    /// JavaScript function and const names are derived.
    #[test]
    fn derive_base_name_javascript() {
        assert_eq!(
            derive_base_name("function greet() {}\n", SupportedLanguage::JavaScript),
            Some("greet".to_string())
        );
        assert_eq!(
            derive_base_name(
                "const add = (a, b) => a + b;\n",
                SupportedLanguage::JavaScript
            ),
            Some("add".to_string())
        );
    }

    /// Go `func` names are derived.
    #[test]
    fn derive_base_name_go() {
        assert_eq!(
            derive_base_name("func main() {}\n", SupportedLanguage::Go),
            Some("main".to_string())
        );
    }

    /// C/C++ `int main` derives `main`.
    #[test]
    fn derive_base_name_c_main() {
        assert_eq!(
            derive_base_name("int main() { return 0; }\n", SupportedLanguage::C),
            Some("main".to_string())
        );
        assert_eq!(
            derive_base_name("int main() { return 0; }\n", SupportedLanguage::Cpp),
            Some("main".to_string())
        );
    }

    /// HTML derives its name from the `<title>` tag.
    #[test]
    fn derive_base_name_html_title() {
        let code = "<html><head><title>My Page</title></head></html>\n";
        assert_eq!(
            derive_base_name(code, SupportedLanguage::Html),
            Some("My Page".to_string())
        );
    }

    /// CSS has no named declarations, so no base name is derived.
    #[test]
    fn derive_base_name_css_none() {
        assert_eq!(
            derive_base_name("body { color: red; }\n", SupportedLanguage::Css),
            None
        );
    }

    /// Prose or empty content yields no base name.
    #[test]
    fn derive_base_name_none_for_prose() {
        assert_eq!(
            derive_base_name("just some text\n", SupportedLanguage::Python),
            None
        );
        assert_eq!(derive_base_name("\n", SupportedLanguage::Python), None);
    }

    /// `derive_file_name` combines the base name with the language extension.
    #[test]
    fn derive_file_name_uses_base_name_and_extension() {
        assert_eq!(
            derive_file_name(
                "def fibonacci(n):\n    return n\n",
                SupportedLanguage::Python
            ),
            "fibonacci.py"
        );
        assert_eq!(
            derive_file_name("fn main() {}\n", SupportedLanguage::Rust),
            "main.rs"
        );
    }

    /// `derive_file_name` falls back to the conventional `main.<ext>` name
    /// when nothing meaningful can be derived.
    #[test]
    fn derive_file_name_falls_back_to_main() {
        assert_eq!(
            derive_file_name("just some text\n", SupportedLanguage::Python),
            "main.py"
        );
        assert_eq!(
            derive_file_name("body { color: red; }\n", SupportedLanguage::Css),
            "main.css"
        );
    }

    /// Java is always `Main.java` regardless of content, because the public
    /// class name must match the file name.
    #[test]
    fn derive_file_name_java_is_always_main() {
        assert_eq!(
            derive_file_name("public class Foo {}\n", SupportedLanguage::Java),
            "Main.java"
        );
        assert_eq!(derive_file_name("\n", SupportedLanguage::Java), "Main.java");
    }

    /// The derived file name is consistent with the temp file written before
    /// execution: when a meaningful name is derived, the temp file uses it;
    /// otherwise it falls back to the same `main.<ext>` as `temp_source_file`.
    #[test]
    fn derive_file_name_is_consistent_with_temp_file() {
        // Meaningful name → temp file uses the derived name.
        let code = "def fibonacci(n):\n    return n\n";
        let derived = derive_file_name(code, SupportedLanguage::Python);
        assert_eq!(derived, "fibonacci.py");

        // No meaningful name → identical to the conventional temp file.
        let fallback = derive_file_name("\n", SupportedLanguage::Python);
        assert_eq!(fallback, temp_source_file(SupportedLanguage::Python));
        assert_eq!(fallback, "main.py");
    }

    // ------------------------------------------------------------------
    // Full chat → editor → terminal flow (integration-style)
    // ------------------------------------------------------------------

    /// Simulate the full "user asks for code" flow: detect language, decide
    /// whether to generate, extract code, and build the run command.
    #[test]
    fn full_flow_python_chat_to_terminal() {
        let user_message = "write python code to print hello";
        let detected = SupportedLanguage::detect(user_message);
        assert_eq!(detected, SupportedLanguage::Python);
        assert!(wants_code(user_message));

        let ai_response = "```python\ndef hello():\n    print('hello')\n```";
        let action = decide_editor_action(ai_response, wants_code(user_message), detected);

        let (language, code) = match action {
            EditorAction::Insert { language, code } => (language, code),
            EditorAction::ShowResponse => panic!("expected code insertion"),
        };
        assert_eq!(language, SupportedLanguage::Python);
        assert!(code.contains("def hello():"));

        // Editor → terminal: build the temp file and run command.
        let file = temp_source_file(language);
        assert_eq!(file, "main.py");
        let command = language.run_command(std::path::Path::new(&file));
        assert!(command.contains(crate::platform::python_command()));
        assert!(command.contains("main.py"));
    }

    /// Simulate the full flow for a Rust request.
    #[test]
    fn full_flow_rust_chat_to_terminal() {
        let user_message = "generate a rust program";
        let detected = SupportedLanguage::detect(user_message);
        assert_eq!(detected, SupportedLanguage::Rust);
        assert!(wants_code(user_message));

        let ai_response = "```rust\nfn main() {\n    println!(\"hi\");\n}\n```";
        let action = decide_editor_action(ai_response, wants_code(user_message), detected);

        let (language, code) = match action {
            EditorAction::Insert { language, code } => (language, code),
            EditorAction::ShowResponse => panic!("expected code insertion"),
        };
        assert_eq!(language, SupportedLanguage::Rust);
        assert!(code.contains("fn main()"));

        let file = temp_source_file(language);
        assert_eq!(file, "main.rs");
        let command = language.run_command(std::path::Path::new(&file));
        assert!(command.contains(crate::platform::rust_compiler()));
        assert!(command.contains("main.rs"));
    }

    /// Simulate a "clear editor" request, which short-circuits before the AI.
    #[test]
    fn full_flow_clear_editor_short_circuits() {
        let user_message = "please clear the editor";
        assert!(wants_clear_editor(user_message));
        // A clear request should not trigger code generation.
        assert!(!wants_code(user_message));
    }

    /// Simulate a plain question that should not touch the editor.
    #[test]
    fn full_flow_plain_question_does_not_touch_editor() {
        let user_message = "what is the capital of France?";
        assert!(!wants_code(user_message));
        assert!(!wants_clear_editor(user_message));

        let ai_response = "The capital of France is Paris.";
        let action = decide_editor_action(
            ai_response,
            wants_code(user_message),
            SupportedLanguage::Python,
        );
        assert_eq!(action, EditorAction::ShowResponse);
    }

    // ------------------------------------------------------------------
    // Edge cases: extract_code_from_response()
    // ------------------------------------------------------------------

    /// A response with no code fences and no inline code yields empty output.
    #[test]
    fn extract_no_code_returns_empty() {
        assert_eq!(
            extract_code_from_response("Just some prose, no code here.", SupportedLanguage::Python),
            ""
        );
        assert_eq!(
            extract_code_from_response("", SupportedLanguage::Python),
            ""
        );
    }

    /// A code fence with no language tag is still accepted (matches any).
    #[test]
    fn extract_untagged_fence_is_accepted() {
        let code =
            extract_code_from_response("```\nprint('hello')\n```", SupportedLanguage::Python);
        assert!(code.contains("print('hello')"));
    }

    /// A code fence tagged with a *different* language is not extracted.
    #[test]
    fn extract_wrong_language_fence_is_ignored() {
        let code =
            extract_code_from_response("```rust\nfn main() {}\n```", SupportedLanguage::Python);
        assert_eq!(code, "");
    }

    /// When multiple matching code blocks are present, all are concatenated.
    #[test]
    fn extract_multiple_blocks_are_concatenated() {
        let code = extract_code_from_response(
            "First:\n```python\ndef a():\n    pass\n```\nSecond:\n```python\ndef b():\n    pass\n```",
            SupportedLanguage::Python,
        );
        assert!(code.contains("def a():"));
        assert!(code.contains("def b():"));
    }

    /// Code containing special characters (quotes, `$`, backticks inside
    /// prose) is preserved verbatim.
    #[test]
    fn extract_preserves_special_characters() {
        let code = extract_code_from_response(
            "```python\nprint(\"$HOME\")\nprint('it\'s')\n```",
            SupportedLanguage::Python,
        );
        assert!(code.contains("$HOME"));
        assert!(code.contains("it's"));
    }

    /// A very long code response is fully preserved (no truncation).
    #[test]
    fn extract_handles_very_long_response() {
        let mut body = String::new();
        for i in 0..2000 {
            body.push_str(&format!("line_{i}\n"));
        }
        let response = format!("```python\n{body}```");
        let code = extract_code_from_response(&response, SupportedLanguage::Python);
        assert!(code.contains("line_0"));
        assert!(code.contains("line_1999"));
        assert!(code.lines().count() >= 2000);
    }

    /// Inline backticks are only extracted when they look like code.
    #[test]
    fn extract_inline_backticks_only_when_code_like() {
        // `def foo():` looks like Python code → extracted.
        let code = extract_code_from_response(
            "Use `def foo():` to define a function.",
            SupportedLanguage::Python,
        );
        assert!(code.contains("def foo():"));

        // `hello` is not code-like → not extracted.
        let not_code =
            extract_code_from_response("Say `hello` to the user.", SupportedLanguage::Python);
        assert_eq!(not_code, "");
    }

    /// A fence that never closes still yields its code via the inline-backtick
    /// fallback (the content is preserved rather than silently dropped).
    #[test]
    fn extract_unterminated_fence_still_yields_code() {
        let code = extract_code_from_response(
            "```python\ndef foo():\n    pass",
            SupportedLanguage::Python,
        );
        assert!(code.contains("def foo():"));
        assert!(code.contains("pass"));
    }

    // ------------------------------------------------------------------
    // Edge cases: decide_editor_action()
    // ------------------------------------------------------------------

    /// An empty response never inserts code.
    #[test]
    fn decide_empty_response_shows_response() {
        assert_eq!(
            decide_editor_action("", true, SupportedLanguage::Python),
            EditorAction::ShowResponse
        );
    }

    /// Code present but the user did not ask for code → show the response.
    #[test]
    fn decide_code_present_but_not_requested_shows_response() {
        let response = "```python\ndef foo():\n    pass\n```";
        assert_eq!(
            decide_editor_action(response, false, SupportedLanguage::Python),
            EditorAction::ShowResponse
        );
    }

    /// A response with only prose but the user asked for code → show response.
    #[test]
    fn decide_prose_when_code_requested_shows_response() {
        let response = "I'm sorry, I can't write that.";
        assert_eq!(
            decide_editor_action(response, true, SupportedLanguage::Python),
            EditorAction::ShowResponse
        );
    }

    // ------------------------------------------------------------------
    // Edge cases: looks_like_code()
    // ------------------------------------------------------------------

    /// Non-code snippets are rejected for each language.
    #[test]
    fn looks_like_code_rejects_plain_text() {
        assert!(!looks_like_code("hello world", SupportedLanguage::Python));
        assert!(!looks_like_code("just some words", SupportedLanguage::Rust));
        assert!(!looks_like_code(
            "a random phrase",
            SupportedLanguage::JavaScript
        ));
        assert!(!looks_like_code("nothing here", SupportedLanguage::Go));
        assert!(!looks_like_code("plain text", SupportedLanguage::Java));
    }

    /// Code-like snippets are accepted for each language.
    #[test]
    fn looks_like_code_accepts_code() {
        assert!(looks_like_code(
            "def foo(): pass",
            SupportedLanguage::Python
        ));
        assert!(looks_like_code("fn main() {}", SupportedLanguage::Rust));
        assert!(looks_like_code(
            "const x = 1;",
            SupportedLanguage::JavaScript
        ));
        assert!(looks_like_code("package main", SupportedLanguage::Go));
        assert!(looks_like_code("public class Foo", SupportedLanguage::Java));
        assert!(looks_like_code("<html>", SupportedLanguage::Html));
        assert!(looks_like_code(
            "body { color: red; }",
            SupportedLanguage::Css
        ));
        assert!(looks_like_code("#include <stdio.h>", SupportedLanguage::C));
        assert!(looks_like_code("std::cout", SupportedLanguage::Cpp));
        assert!(looks_like_code(
            "interface Foo",
            SupportedLanguage::TypeScript
        ));
    }

    // ------------------------------------------------------------------
    // Edge cases: full flow across all 10 languages
    // ------------------------------------------------------------------

    /// For every supported language, a matching request + response flows all
    /// the way from chat to a run command without panicking.
    #[test]
    fn full_flow_all_languages_produce_run_command() {
        let cases: &[(&str, &str, SupportedLanguage)] = &[
            (
                "write python",
                "```python\nprint('hi')\n```",
                SupportedLanguage::Python,
            ),
            (
                "write rust",
                "```rust\nfn main() {}\n```",
                SupportedLanguage::Rust,
            ),
            (
                "write javascript",
                "```javascript\nconsole.log('hi');\n```",
                SupportedLanguage::JavaScript,
            ),
            (
                "write typescript",
                "```typescript\nconst x: number = 1;\n```",
                SupportedLanguage::TypeScript,
            ),
            (
                "write html",
                "```html\n<html></html>\n```",
                SupportedLanguage::Html,
            ),
            (
                "write css",
                "```css\nbody { color: red; }\n```",
                SupportedLanguage::Css,
            ),
            (
                "write c code",
                "```c\nint main() { return 0; }\n```",
                SupportedLanguage::C,
            ),
            (
                "write c++",
                "```cpp\nint main() { return 0; }\n```",
                SupportedLanguage::Cpp,
            ),
            (
                "write java",
                "```java\npublic class Main {}\n```",
                SupportedLanguage::Java,
            ),
            (
                "write go code",
                "```go\npackage main\nfunc main() {}\n```",
                SupportedLanguage::Go,
            ),
        ];

        for (msg, response, expected_lang) in cases {
            let detected = SupportedLanguage::detect(msg);
            assert_eq!(detected, *expected_lang, "detect failed for: {msg}");
            assert!(wants_code(msg), "wants_code failed for: {msg}");

            let action = decide_editor_action(response, true, detected);
            let (lang, code) = match action {
                EditorAction::Insert { language, code } => (language, code),
                EditorAction::ShowResponse => panic!("no code inserted for: {msg}"),
            };
            assert_eq!(lang, *expected_lang);
            assert!(!code.trim().is_empty(), "empty code for: {msg}");

            // Editor → terminal handoff must produce a non-empty command.
            let file = temp_source_file(lang);
            let command = lang.run_command(std::path::Path::new(&file));
            assert!(!command.trim().is_empty(), "empty command for: {msg}");
        }
    }

    // ------------------------------------------------------------------
    // Streaming flow tests (Task 19)
    // ------------------------------------------------------------------

    /// Incremental token accumulation and post-stream code block extraction.
    #[test]
    fn streaming_flow_incremental_assembly_and_code_extraction() {
        let chunks = vec![
            "Here is the Python function:\n\n```python\n",
            "def add(a, b):\n",
            "    return a + b\n",
            "```\n",
            "Hope this helps!",
        ];

        let mut accumulated_chat = String::new();
        for chunk in &chunks {
            accumulated_chat.push_str(chunk);
        }

        assert_eq!(
            accumulated_chat,
            "Here is the Python function:\n\n```python\ndef add(a, b):\n    return a + b\n```\nHope this helps!"
        );

        // After stream completion, extraction runs:
        let action = decide_editor_action(&accumulated_chat, true, SupportedLanguage::Python);
        match action {
            EditorAction::Insert { language, code } => {
                assert_eq!(language, SupportedLanguage::Python);
                assert_eq!(code, "def add(a, b):\n    return a + b");
            }
            EditorAction::ShowResponse => panic!("Expected Insert action after streaming finished"),
        }
    }

    /// Mid-stream error handling preserves partial response and appends error notice.
    #[test]
    fn streaming_flow_handles_mid_stream_error_preserves_partial() {
        let chunks = vec!["Here is partial text... ", "and some more."];
        let mut accumulated = String::new();
        for c in chunks {
            accumulated.push_str(c);
        }
        let error_notice = "\n\n⚠️ A network error occurred while contacting the Albert API.";
        accumulated.push_str(error_notice);

        assert!(accumulated.starts_with("Here is partial text... and some more."));
        assert!(accumulated.contains("network error"));

        // If code was requested but stream failed mid-way without complete code block, show partial response in chat
        let action = decide_editor_action(&accumulated, true, SupportedLanguage::Python);
        assert_eq!(action, EditorAction::ShowResponse);
    }

    // ------------------------------------------------------------------
    // Multi-turn conversation context tests (Task 20)
    // ------------------------------------------------------------------

    /// Code extraction is strictly scoped to the latest response, ignoring prior turns in history.
    #[test]
    fn multi_turn_code_extraction_scoped_to_latest_response() {
        let _turn1_response = "```python\ndef old_function():\n    return 1\n```";
        let turn2_response = "```python\ndef new_function():\n    return 2\n```";

        let action = decide_editor_action(turn2_response, true, SupportedLanguage::Python);
        match action {
            EditorAction::Insert { language, code } => {
                assert_eq!(language, SupportedLanguage::Python);
                assert_eq!(code, "def new_function():\n    return 2");
            }
            EditorAction::ShowResponse => panic!("Expected Insert action for latest response"),
        }
    }

    /// Clearing chat resets history payload for subsequent requests.
    #[test]
    fn clear_chat_resets_history_payload() {
        let mut messages = vec![
            crate::api::Message {
                role: crate::api::Role::User,
                content: "First turn".to_string(),
            },
            crate::api::Message {
                role: crate::api::Role::AI,
                content: "First AI response".to_string(),
            },
        ];

        let history_before = crate::api::build_chat_history(&messages, 20);
        assert_eq!(history_before.len(), 2);

        // Simulate Clear Chat action
        messages.clear();
        messages.push(crate::api::Message {
            role: crate::api::Role::AI,
            content: "Chat cleared. How can I help you?".to_string(),
        });

        let history_after = crate::api::build_chat_history(&messages, 20);
        assert_eq!(history_after.len(), 0);
    }
}

