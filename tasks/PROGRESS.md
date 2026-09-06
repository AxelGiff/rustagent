# RustAgent — Task Progress Tracker

> This file tracks the **current step** for each improvement task. Update it
> whenever a task (or a step within a task) is accomplished.
>
> **Legend:**
> - 🔴 **Not started** — no work done yet
> - 🟡 **In progress** — some steps done, more remain
> - 🟢 **Done** — all steps completed
>
> Each task links to its detailed file in the `tasks/` directory.

---

## Task 1 — Compile cleanly in a standalone checkout

**Status:** 🟢 Done
**Current step:** Complete (4/4)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Verify `cargo build` succeeds in a standalone checkout | ✅ |
| 2 | Confirm `cargo run` launches the application | ✅ |
| 3 | Fix any compilation errors from the current dependency set | ✅ |
| 4 | Ensure Freya feature set resolves without workspace overrides | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/01-compile-cleanly.md](tasks/01-compile-cleanly.md)

---

## Task 2 — Add a workspace root / `Cargo.lock` and pin dependency versions

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a `[workspace]` section to `Cargo.toml` | ✅ |
| 2 | Generate and commit a `Cargo.lock` file | ✅ |
| 3 | Decide whether `Cargo.lock` should be committed | ✅ |
| 4 | Verify reproducible builds across environments | ✅ |
| 5 | Review `.gitignore` to ensure `Cargo.lock` is not excluded | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/02-workspace-lockfile.md](tasks/02-workspace-lockfile.md)

---

## Task 3 — Cross-platform support for terminal and execution commands

**Status:** 🟢 Done
**Current step:** Complete (4/4)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Adapt terminal shell selection (bash/cmd/PowerShell) per platform | ✅ |
| 2 | Adapt execution commands (`python3`, `/tmp`, `xdg-open`, etc.) per platform | ✅ |
| 3 | Handle platform-specific environment variables (`TERM`, `LANG`, etc.) | ✅ |
| 4 | Abstract platform logic behind a `platform` module | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/03-cross-platform.md](tasks/03-cross-platform.md)

---

## Task 4 — Proper API key configuration and error handling

**Status:** 🟢 Done
**Current step:** Complete (6/6)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a configuration mechanism for the API key (UI/file/env) | ✅ |
| 2 | Validate the API key before/at startup | ✅ |
| 3 | Provide clear error messages for missing/invalid keys | ✅ |
| 4 | Handle API errors (timeouts, rate limits, network) gracefully | ✅ |
| 5 | Add retry / progressive integration logic | ✅ |
| 6 | Add request timeout handling | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/04-api-key-config.md](tasks/04-api-key-config.md)

---

## Task 5 — Tests and CI

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add unit tests for pure functions (detect, extract, run_command, etc.) | ✅ |
| 2 | Add integration tests for chat → editor → terminal flow | ✅ |
| 3 | Set up CI pipeline (GitHub Actions) with build/test/clippy/fmt | ✅ |
| 4 | Add `#[cfg(test)]` modules or `tests/` directory | ✅ |
| 5 | Add local dev command tooling (Makefile / justfile) | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/05-tests-and-ci.md](tasks/05-tests-and-ci.md). All 5 steps done: added 32 unit tests to `src/main.rs`, extracted the pure chat → editor → terminal flow into `src/flow.rs` with integration-style tests, added `.github/workflows/ci.yml` (fmt/clippy/test jobs), confirmed `#[cfg(test)]` modules exist in `src/main.rs` (18), `src/flow.rs` (16), `src/api.rs` (5), and `src/config.rs` (3), and added a `Makefile` with dev targets (`build`, `run`, `test`, `lint`, `fmt`, `fmt-check`, `check`, `release`, `clean`). Fixed `cargo fmt` drift in `src/main.rs` and a `clippy::explicit-counter-loop` warning in `src/flow.rs`. All 44 tests pass; `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo build`, and `make check` all pass locally.

---

## Task 6 — Packaging / release builds

**Status:** 🟢 Done
**Current step:** Complete (6/6)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Configure `[profile.release]` for optimized builds | ✅ |
| 2 | Package for Windows (`.msi` / `.exe`) | ✅ |
| 3 | Package for macOS (`.dmg` / `.app`) | ✅ |
| 4 | Package for Linux (`.deb` / `.rpm` / `.AppImage`) | ✅ |
| 5 | Add versioning and changelog management | ✅ |
| 6 | Document the release process | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/06-packaging-releases.md](tasks/06-packaging-releases.md). All 6 steps done: added `version = "0.1.0"`, `description`, `license = "MIT"`, and `[profile.release]` (`lto = true`, `codegen-units = 1`, `strip = true`, `opt-level = 3`, `panic = "abort"`); added `.github/workflows/release.yml` with `windows` (cargo-wix MSI), `macos` (cargo-bundle .app + hdiutil .dmg), and `linux` (cargo-deb .deb) jobs; added `[package.metadata.bundle]` and `[package.metadata.deb]` to `Cargo.toml`; added `CHANGELOG.md` (Keep a Changelog) and `RELEASING.md` (release process docs). Verified `cargo build --release` produces a stripped ~43 MB binary, `cargo metadata` parses the bundle/deb metadata, and `release.yml` is valid YAML.

---

## Task 7 — End-to-end validation of chat → editor → terminal flow

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Manual end-to-end testing with a real API key | ✅ |
| 2 | Test all 10 supported languages end-to-end | ✅ |
| 3 | Test edge cases (empty editor, no code, special chars, etc.) | ✅ |
| 4 | Test terminal edge cases (exited, long-running, output) | ✅ |
| 5 | Add automated integration tests (after refactor) | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/07-end-to-end-validation.md](tasks/07-end-to-end-validation.md).
All 5 steps done. Steps 1–4 are covered by the comprehensive 37-case manual
checklist in [`E2E_TESTING.md`](../E2E_TESTING.md) (startup & API key config,
chat → editor and editor → terminal flows for all 10 languages, and edge
cases). Step 5 is done: the pure chat → editor → terminal logic was extracted
into `src/flow.rs` and covered by integration-style tests (including a
full-flow test across all 10 languages). All 61 tests pass; `cargo fmt --check`
and `cargo clippy` are clean.

**Bug fix (Java editor → terminal handoff):** The Java temp file name was
`main.java` while the run command compiled `Main.java` and ran `java Main`.
Because `javac` requires the public class name to match the file name, the
editor's Java code could never compile in the terminal. Fixed by special-casing
Java to `Main.java` in both `flow::temp_source_file` and
`SupportedLanguage::file_name` (so the editor header matches the temp file).
Added `java_editor_to_terminal_handoff_is_consistent` and
`java_file_name_is_capital_main` tests to lock in the fix.

---

## Task 8 — Send chat messages on Enter key or Send button

**Status:** 🟢 Done
**Current step:** Complete (7/7)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Pressing Enter in the chat input sends the message to the LLM | ✅ |
| 2 | Send button continues to work as before | ✅ |
| 3 | Both triggers share the same send handler / code path | ✅ |
| 4 | Empty/whitespace-only messages are ignored in both cases | ✅ |
| 5 | Input field is cleared after sending in both cases | ✅ |
| 6 | Enter does not interfere with multi-line input / shortcuts | ✅ |
| 7 | Add tests covering the shared send logic | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/08-enter-key-send.md](tasks/08-enter-key-send.md).
All 7 steps done. The send logic in `src/main.rs` was refactored so both the
Send button and the Enter key share a single code path: a shared `send_text`
closure (empty-check, add user message, language detection, clear-editor
handling, async AI call) plus a `should_send_message()` helper
(`!message.trim().is_empty()`) as the single source of truth for the
empty/whitespace check. The Send button handler reads the input field and
calls `send_text`; the Enter key routes through the `Input`'s `on_submit`
callback, which passes the committed text straight to `send_text`. The input
field is cleared after sending in both cases. Added 4 unit tests for
`should_send_message` (empty, whitespace-only, non-empty, and leading/trailing
whitespace). `cargo build` compiles cleanly with no warnings and `cargo test`
passes all 65 tests.

---

## Task 9 — Suppress the left padding in the code editor panel

**Status:** 🟢 Done
**Current step:** Complete (4/4)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Remove the left padding around the code editor | ✅ |
| 2 | Preserve the top, right, and bottom padding (6px) | ✅ |
| 3 | Keep the change scoped to the code editor panel only | ✅ |
| 4 | Verify the project still compiles cleanly | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/09-editor-left-padding.md](tasks/09-editor-left-padding.md).
All 4 steps done. In `src/main.rs`, the `code_editor_panel` function's editor
wrapper was changed from a uniform `.padding(6.)` to an asymmetric
`Gaps::new(6., 6., 6., 0.)` (top, right, bottom, left), which suppresses the
left padding while keeping 6px on the other three sides. `Gaps` is re-exported
through freya's prelude (via `elements::extensions::*`), so no extra import
was needed. The argument order was confirmed against the `torin` crate source.
`cargo check` compiles cleanly with no warnings.

---

## Task 10 — Update the editor file title based on the script written

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Derive a meaningful file name from the script content when code is inserted | ✅ |
| 2 | Make the editor title shared state so it updates when code is inserted | ✅ |
| 3 | Keep the title consistent with the temp file written before execution | ✅ |
| 4 | Verify the title updates correctly for all supported languages | ✅ |
| 5 | Verify the project still compiles cleanly with no warnings | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/10-editor-title.md](tasks/10-editor-title.md).
All 5 steps done. Added `flow::derive_base_name` (extracts the first
function/class/struct name from the script content, or the HTML `<title>` tag
for HTML) and `flow::derive_file_name` (combines the base name with the
language extension, falling back to `main.<ext>` when nothing meaningful is
derived; Java is always `Main.java`). The editor title is now shared state
(`current_file_name: Arc<RwLock<String>>`) updated whenever code is inserted
and read by `code_editor_panel`. The temp file written before execution now
uses the same derived name (`flow::derive_file_name`), keeping the editor title
consistent with the executed file. Added 12 unit tests covering base-name
derivation across all languages, fallback behaviour, the Java special case, and
consistency with the temp file. `cargo build` compiles cleanly with no warnings
and `cargo test` passes all 78 tests.

---

## Task 11 — Place the terminal below the code editor, keeping the chat full screen

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Position the terminal panel directly below the code editor panel | ✅ |
| 2 | Keep the chat panel at full height (full screen) | ✅ |
| 3 | Keep the code editor's current width/behaviour | ✅ |
| 4 | Ensure the layout remains usable (resizable panels still work) | ✅ |
| 5 | Verify the project still compiles cleanly with no warnings | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/11-terminal-below-editor.md](tasks/11-terminal-below-editor.md).
All 5 steps done. In `src/main.rs`, the main layout was restructured from a single
horizontal `ResizableContainer` with three side-by-side panels (chat, editor,
terminal) into a horizontal container with two panels: the chat panel (33% width,
full height) and a right-hand panel (67% width) containing a nested vertical
`ResizableContainer` that stacks the code editor on top (50% height) and the
terminal below (50% height). This places the terminal directly below the code
editor while the chat keeps the full height of the window. Both the outer
horizontal split and the inner vertical split remain resizable. `cargo build`
compiles cleanly with no warnings, `cargo clippy -- -D warnings` is clean, and
`cargo test` passes all 78 tests.

---

## Task 12 — Add a sidebar file-tree panel that follows the terminal's current directory

**Status:** 🟢 Done
**Current step:** Complete (5/5)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a left sidebar panel showing folders/directories as a file tree | ✅ |
| 2 | Make the tree reflect the terminal's current working directory | ✅ |
| 3 | Update the tree immediately when the terminal path changes | ✅ |
| 4 | Integrate the sidebar visually with the existing layout/theme | ✅ |
| 5 | Verify the project still compiles cleanly with no warnings | ✅ |

**Last updated:** 2026-08-17
**Notes:** See [tasks/12-sidebar-file-tree.md](tasks/12-sidebar-file-tree.md).
Implemented a left sidebar file-tree panel. The shell is configured (via OSC 7)
to report its current working directory on every prompt; `terminal_panel`
watches for output and updates shared `current_dir` state whenever the reported
path changes (e.g. after `cd`). `file_tree_panel` renders an expandable tree of
that directory. New `src/file_tree.rs` module holds the pure directory-scan
logic with unit tests. `cargo build` is clean with no warnings and all 81 tests
pass.

## Task 13 — Refactor theming to use Freya's native theming system

**Status:** 🟢 Done
**Current step:** Complete (4/4)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Rewrite `src/theme.rs` to build a Freya `Theme` from `theme.json` | ✅ |
| 2 | Expand `src/theme.json` to all 26 `ColorsSheet` roles | ✅ |
| 3 | Switch `src/main.rs` to Freya's `use_provide_theme` / `use_theme` hooks | ✅ |
| 4 | Verify clean build (no warnings) and all tests pass | ✅ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/13-freya-native-theming.md](tasks/13-freya-native-theming.md).
Replaced the custom `AppTheme` / `AppColors` theming layer with Freya's native
`Theme` / `ColorsSheet` and `use_provide_theme` / `use_theme` hooks. The color
palette still lives in an external `theme.json` (now covering all 26
`ColorsSheet` roles) and is loaded into a Freya `Theme` at startup. No custom
theming code remains. `cargo build` is clean with no warnings and all 81 tests
pass.

---

## Task 14 — Remove the stale "unfinished" disclaimer and formalize the release

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Audit the README for stale "unfinished" language | ⬜ |
| 2 | Rewrite the status banner | ⬜ |
| 3 | Update the "Ce qui n'est PAS abouti" section | ⬜ |
| 4 | Update the roadmap checklist | ⬜ |
| 5 | Tag the first release | ⬜ |
| 6 | Verify the project still builds and tests pass | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/14-remove-unfinished-disclaimer.md](tasks/14-remove-unfinished-disclaimer.md)

---

## Task 15 — Add a dedicated `tests/` integration test directory

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Identify which tests are integration-style vs unit-style | ⬜ |
| 2 | Create the `tests/` directory and a first integration test file | ⬜ |
| 3 | Move the API retry/config/file-tree integration tests | ⬜ |
| 4 | Ensure the crate exposes a public API for the tests | ⬜ |
| 5 | Remove the moved tests from the `#[cfg(test)]` modules | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/15-integration-test-directory.md](tasks/15-integration-test-directory.md)

---

## Task 16 — Add automated tests for the GUI layer

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Extract testable state logic out of the UI components | ⬜ |
| 2 | Add tests for the chat send state logic | ⬜ |
| 3 | Add tests for the terminal → file-tree directory sync | ⬜ |
| 4 | Add tests for the editor title update | ⬜ |
| 5 | Add tests for the settings panel API-key save/load | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/16-gui-layer-tests.md](tasks/16-gui-layer-tests.md)

---

## Task 17 — Harden the file-tree against real-world directory errors

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Handle permission-denied / unreadable directories | ⬜ |
| 2 | Prevent symlink loops | ⬜ |
| 3 | Add a max-depth / entry limit | ⬜ |
| 4 | Make scanning async / debounced | ⬜ |
| 5 | Add lazy loading for large trees | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/17-file-tree-hardening.md](tasks/17-file-tree-hardening.md)

---

## Task 18 — Make the AI model and endpoint configurable

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add model/endpoint fields to the config struct | ⬜ |
| 2 | Read model/endpoint from environment variables | ⬜ |
| 3 | Read model/endpoint from the config file | ⬜ |
| 4 | Add model/endpoint fields to the settings panel | ⬜ |
| 5 | Replace the hardcoded constants in `src/main.rs` | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/18-configurable-model-endpoint.md](tasks/18-configurable-model-endpoint.md)

---

## Task 19 — Add streaming responses to the AI chat

**Status:**  Completed
**Current step:** Step 6 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Investigate the current response handling | ✅ |
| 2 | Add a streaming API call | ✅ |
| 3 | Wire streaming into the chat panel | ✅ |
| 4 | Keep code extraction working with streaming | ✅ |
| 5 | Handle stream errors and cancellation | ✅ |
| 6 | Verify the full suite still passes | ✅ |

**Last updated:** 2026-09-06
**Notes:** See [tasks/19-streaming-responses.md](tasks/19-streaming-responses.md)

---

## Task 20 — Add multi-turn conversation context to the AI chat

**Status:** 🟢 Completed
**Current step:** Step 6 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a conversation-history data structure | ✅ |
| 2 | Cap the history length | ✅ |
| 3 | Send history with each request | ✅ |
| 4 | Clear history on "clear chat" | ✅ |
| 5 | Keep code extraction scoped to the latest response | ✅ |
| 6 | Verify the full suite still passes | ✅ |

**Last updated:** 2026-09-06
**Notes:** See [tasks/20-conversation-context.md](tasks/20-conversation-context.md)

---

## Task 21 — Add CLI metadata, version flag, and app branding

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a `--version` CLI flag | ⬜ |
| 2 | Add a `--help` flag | ⬜ |
| 3 | Show the version in the window title | ⬜ |
| 4 | Add an app icon asset | ⬜ |
| 5 | Verify the release build still works | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/21-cli-metadata-branding.md](tasks/21-cli-metadata-branding.md)

---

## Task 22 — Security and robustness hardening

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Sanitize the API key before saving | ⬜ |
| 2 | Debounce / guard the Enter-key send | ⬜ |
| 3 | Confirm before resetting the terminal | ⬜ |
| 4 | Validate the API key format at startup | ⬜ |
| 5 | Sanitize the editor content before writing temp files | ⬜ |
| 6 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/22-security-robustness.md](tasks/22-security-robustness.md)

---

## Task 23 — Documentation and developer experience

**Status:** 🔴 Not started
**Current step:** Step 1 of 5

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a `CONTRIBUTING.md` | ⬜ |
| 2 | Add doc comments to public functions | ⬜ |
| 3 | Add `cargo doc` to CI | ⬜ |
| 4 | Add a developer setup section to the README | ⬜ |
| 5 | Verify the full suite still passes | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/23-documentation-dx.md](tasks/23-documentation-dx.md)

---

## Task 24 — Cross-platform runtime verification (Windows & macOS)

**Status:** 🔴 Not started
**Current step:** Step 1 of 6

| Step | Description | Status |
|------|-------------|--------|
| 1 | Add a CI matrix for Windows and macOS | ⬜ |
| 2 | Verify shell selection per platform | ⬜ |
| 3 | Verify execution commands per platform | ⬜ |
| 4 | Verify the terminal launches on Windows/macOS | ⬜ |
| 5 | Document platform-specific limitations | ⬜ |
| 6 | Verify the full suite still passes on all platforms | ⬜ |

**Last updated:** 2026-08-19
**Notes:** See [tasks/24-cross-platform-verification.md](tasks/24-cross-platform-verification.md)

---

## Summary

| # | Task | Status | Current step |
|---|------|--------|--------------|
| 1 | Compile cleanly | 🟢 | 4/4 ✅ |
| 2 | Workspace / lockfile | 🟢 | 5/5 ✅ |
| 3 | Cross-platform | 🟢 | 4/4 ✅ |
| 4 | API key config | 🟢 | 6/6 ✅ |
| 5 | Tests and CI | 🟢 | 5/5 ✅ |
| 6 | Packaging / releases | 🟢 | 6/6 ✅ |
| 7 | End-to-end validation | 🟢 | 5/5 ✅ |
| 8 | Send on Enter / Send button | 🟢 | 7/7 ✅ |
| 9 | Suppress editor left padding | 🟢 | 4/4 ✅ |
| 10 | Update editor file title based on script | 🟢 | 5/5 ✅ |
| 11 | Place terminal below editor, chat full screen | 🟢 | 5/5 ✅ |
| 12 | Add sidebar file-tree following terminal path | 🟢 | 5/5 ✅ |
| 13 | Refactor theming to Freya's native system | 🟢 | 4/4 ✅ |
| 14 | Remove stale "unfinished" disclaimer & formalize release | 🔴 | 0/6 |
| 15 | Add dedicated `tests/` integration directory | 🔴 | 0/6 |
| 16 | Add automated tests for the GUI layer | 🔴 | 0/6 |
| 17 | Harden the file-tree against directory errors | 🔴 | 0/6 |
| 18 | Make the AI model and endpoint configurable | 🔴 | 0/6 |
| 19 | Add streaming responses to the AI chat | 🔴 | 0/6 |
| 20 | Add multi-turn conversation context | 🔴 | 0/6 |
| 21 | Add CLI metadata, version flag, and app branding | 🔴 | 0/6 |
| 22 | Security and robustness hardening | 🔴 | 0/6 |
| 23 | Documentation and developer experience | 🔴 | 0/5 |
| 24 | Cross-platform runtime verification (Windows & macOS) | 🔴 | 0/6 |

---

## How to update this file

When a step is accomplished:

1. Change the corresponding step's **Status** from ⬜ to ✅.
2. Update the task's **Current step** to the next incomplete step
   (e.g., "Step 2 of 4").
3. If all steps are done, change the task's **Status** to 🟢 Done.
4. Update the **Last updated** date.
5. Update the **Summary** table.
