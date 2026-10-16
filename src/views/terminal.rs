//! Integrated terminal panel component.

use crate::platform;
use freya::clipboard::Clipboard;
use freya::prelude::*;
use freya::terminal::*;
use futures_util::FutureExt;

/// Spawn a fresh terminal and return its handle (or None on failure).
/// The shell and environment are chosen per-platform via the `platform` module.
pub fn spawn_terminal() -> Option<TerminalHandle> {
    let mut cmd = CommandBuilder::new(platform::terminal_shell());
    // Pass shell arguments that make the shell report its current working
    // directory via OSC 7, so the file-tree sidebar can follow the terminal.
    for arg in platform::terminal_shell_args() {
        cmd.arg(arg);
    }
    for (key, value) in platform::terminal_env() {
        cmd.env(key, value);
    }
    TerminalHandle::new(TerminalId::new(), cmd, None).ok()
}

/// Normalise un chemin reçu via OSC 7 (notamment depuis Git Bash / MSYS2 sous Windows).
pub fn normalize_osc7_path(raw: std::path::PathBuf) -> std::path::PathBuf {
    let s = raw.to_string_lossy();
    let trimmed = s.trim().trim_matches('\r');
    if cfg!(windows) {
        // Convertir un chemin MSYS2 de type `/c/Users/...` ou `c/Users/...` en `C:/Users/...`
        let bytes = trimmed.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            let drive = (bytes[1] as char).to_ascii_uppercase();
            return std::path::PathBuf::from(format!("{}:{}", drive, &trimmed[2..]));
        }
        if bytes.len() >= 2
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            let drive = (bytes[0] as char).to_ascii_uppercase();
            return std::path::PathBuf::from(format!("{}:{}", drive, &trimmed[1..]));
        }
    }
    std::path::PathBuf::from(trimmed)
}

pub fn terminal_panel(
    handle: Writable<Option<TerminalHandle>>,
    current_dir: Writable<std::path::PathBuf>,
) -> impl IntoElement {
    let handle_for_future = handle.clone();
    let current_dir_for_future = current_dir.clone();
    use_future(move || {
        let mut handle_for_future = handle_for_future.clone();
        let mut current_dir_for_future = current_dir_for_future.clone();
        async move {
            let terminal_handle = handle_for_future.read().clone();
            let Some(terminal_handle) = terminal_handle else {
                return;
            };
            loop {
                futures_util::select! {
                    _ = terminal_handle.closed().fuse() => {
                        let _ = handle_for_future.write().take();
                        break;
                    }
                    _ = terminal_handle.clipboard_changed().fuse() => {
                        if let Some(text) = terminal_handle.clipboard_content() {
                            let _ = Clipboard::set(text);
                        }
                    }
                    _ = terminal_handle.output_received().fuse() => {
                        // The shell reports its working directory via OSC 7 on
                        // every prompt. Whenever new output arrives, check the
                        // reported directory and update the sidebar if it
                        // changed (e.g. after the user runs `cd`).
                        // IMPORTANT: Drop the read/peek borrow BEFORE calling `.write()`
                        // to avoid a RefCell double-borrow panic in Freya State!
                        if let Some(raw_cwd) = terminal_handle.cwd() {
                            let cwd = normalize_osc7_path(raw_cwd);
                            if cwd.is_dir() {
                                let should_update = {
                                    let current = current_dir_for_future.peek();
                                    *current != cwd
                                };
                                if should_update {
                                    *current_dir_for_future.write() = cwd;
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let a11y_id = use_a11y();
    let c = use_theme().read().colors.clone();
    let focus = use_focus(a11y_id);
    let mut dimensions = use_state(|| (0.0, 0.0));
    let mut click_origin = use_state(|| None::<(usize, usize)>);

    let handle_for_side_effect = handle.clone();
    use_side_effect(move || {
        let focused = *Platform::get().is_app_focused.read() && focus().is_focused();
        if let Some(handle) = handle_for_side_effect.read().clone() {
            handle.focus_changed(focused);
        }
    });

    rect()
        .expanded()
        .center()
        .background(c.background)
        .color(c.text_primary)
        .child(if let Some(handle) = handle.read().clone() {
            rect()
                .child(
                    Terminal::new(handle.clone())
                        .on_measured(move |(char_width, line_height)| {
                            dimensions.set((char_width, line_height));
                        })
                        .on_mouse_down({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                a11y_id.request_focus();
                                let (char_width, line_height) = dimensions();
                                let cw = char_width.max(1.0) as f64;
                                let lh = line_height.max(1.0) as f64;
                                let col = (e.element_location.x / cw) as f32;
                                let row = (e.element_location.y / lh) as f32;
                                click_origin.set(Some((row as usize, col as usize)));
                                let button = match e.button {
                                    Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                                    Some(MouseButton::Right) => TerminalMouseButton::Right,
                                    _ => TerminalMouseButton::Left,
                                };
                                let selection_type = match EventsCombos::pressed(e.element_location)
                                {
                                    PressEventType::Double => SelectionType::Semantic,
                                    PressEventType::Triple => SelectionType::Lines,
                                    _ => SelectionType::Simple,
                                };
                                handle.mouse_down(row, col, button, selection_type);
                            }
                        })
                        .on_mouse_move({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                let (char_width, line_height) = dimensions();
                                let cw = char_width.max(1.0) as f64;
                                let lh = line_height.max(1.0) as f64;
                                let col = (e.element_location.x / cw) as f32;
                                let row = (e.element_location.y / lh) as f32;
                                handle.mouse_move(row, col);
                            }
                        })
                        .on_mouse_up({
                            let handle = handle.clone();
                            move |e: Event<MouseEventData>| {
                                let (char_width, line_height) = dimensions();
                                let cw = char_width.max(1.0) as f64;
                                let lh = line_height.max(1.0) as f64;
                                let col = (e.element_location.x / cw) as f32;
                                let row = (e.element_location.y / lh) as f32;
                                let button = match e.button {
                                    Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                                    Some(MouseButton::Right) => TerminalMouseButton::Right,
                                    _ => TerminalMouseButton::Left,
                                };
                                handle.mouse_up(row, col, button);
                                let origin = click_origin();
                                click_origin.set(None);
                                if button == TerminalMouseButton::Left
                                    && origin == Some((row as usize, col as usize))
                                    && let Some(url) = handle.hyperlink_at(row, col)
                                {
                                    let _ = open::that(url);
                                }
                            }
                        })
                        .on_global_pointer_press({
                            let handle = handle.clone();
                            move |_: Event<PointerEventData>| {
                                handle.release();
                            }
                        })
                        .on_wheel({
                            let handle = handle.clone();
                            move |e: Event<WheelEventData>| {
                                let (char_width, line_height) = dimensions();
                                let cw = char_width.max(1.0) as f64;
                                let lh = line_height.max(1.0) as f64;
                                let (mouse_x, mouse_y) = e.element_location.to_tuple();
                                let col = (mouse_x / cw) as f32;
                                let row = (mouse_y / lh) as f32;
                                handle.wheel(e.delta_y, row, col);
                            }
                        })
                        .a11y_id(a11y_id)
                        .a11y_role(AccessibilityRole::Terminal)
                        .a11y_auto_focus(true)
                        .on_key_up({
                            let handle = handle.clone();
                            move |e: Event<KeyboardEventData>| {
                                if e.key == Key::Named(NamedKey::Shift) {
                                    handle.shift_pressed(false);
                                }
                            }
                        })
                        .on_key_down(move |e: Event<KeyboardEventData>| {
                            let ctrl_shift =
                                e.modifiers.contains(Modifiers::CONTROL | Modifiers::SHIFT);

                            match &e.key {
                                Key::Character(ch)
                                    if ctrl_shift && ch.eq_ignore_ascii_case("c") =>
                                {
                                    if let Some(text) = handle.get_selected_text() {
                                        let _ = Clipboard::set(text);
                                    }
                                }
                                Key::Character(ch)
                                    if ctrl_shift && ch.eq_ignore_ascii_case("v") =>
                                {
                                    if let Ok(text) = Clipboard::get() {
                                        let _ = handle.paste(&text);
                                    }
                                }
                                _ => {
                                    let _ = handle.write_key(&e.key, e.modifiers);
                                }
                            }
                        }),
                )
                .expanded()
                .background(c.surface_secondary)
                .padding(6.)
                .into_element()
        } else {
            "Terminal exited".into_element()
        })
}
