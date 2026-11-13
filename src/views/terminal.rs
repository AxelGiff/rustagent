//! Integrated terminal panel component.

use crate::platform;
use crate::views::ui;
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
        // Format `/c/Users/...` (slashes initiaux, lettre de lecteur à l'index 1)
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b'/'
        {
            let drive = (bytes[1] as char).to_ascii_uppercase();
            return std::path::PathBuf::from(format!("{}:{}", drive, &trimmed[2..]));
        }
        // Format `c/Users/...` (sans slash initial, lettre de lecteur à l'index 0)
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b'/'
        {
            let drive = (bytes[0] as char).to_ascii_uppercase();
            return std::path::PathBuf::from(format!("{}:{}", drive, &trimmed[1..]));
        }
    }
    std::path::PathBuf::from(trimmed)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerminalViewTab {
    Shell(usize),
}

pub fn terminal_panel(
    handle: Writable<Option<TerminalHandle>>,
    current_dir: Writable<std::path::PathBuf>,
) -> impl IntoElement {
    let active_tab = use_state(|| TerminalViewTab::Shell(0));
    let secondary_terminals = use_state(Vec::<TerminalHandle>::new);

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
    let dimensions = use_state(|| (0.0, 0.0));
    let click_origin = use_state(|| None::<(usize, usize)>);

    let handle_for_side_effect = handle.clone();
    use_side_effect(move || {
        let focused = *Platform::get().is_app_focused.read() && focus().is_focused();
        if let Some(handle) = handle_for_side_effect.read().clone() {
            handle.focus_changed(focused);
        }
    });

    let active_tab_val = *active_tab.read();
    let is_t1_active = active_tab_val == TerminalViewTab::Shell(0);

    let handle_for_clear = handle.clone();
    let secondary_for_clear = secondary_terminals.clone();

    rect()
        .expanded()
        .content(Content::Flex)
        .background(c.background)
        .color(c.text_primary)
        .child(
            // En-tête d'onglets Terminal interactif (style Warp / VS Code)
            rect()
                .width(Size::fill())
                .height(Size::px(28.))
                .background(c.surface_primary)
                .padding(Gaps::new(2., 8., 2., 8.))
                .horizontal()
                .cross_align(Alignment::Center)
                .content(Content::Flex)
                .border(Border::new().fill(c.border).width(BorderWidth {
                    top: 0.,
                    right: 0.,
                    bottom: 1.,
                    left: 0.,
                }))
                .spacing(6.)
                .child(
                    // Onglet Terminal 1
                    {
                        let mut active_tab = active_tab;
                        ui::clickable(
                            rect()
                                .padding(Gaps::new(2., 8., 2., 8.))
                                .background(if is_t1_active { c.surface_secondary } else { c.surface_primary })
                                .corner_radius(3.)
                                .border(Border::new().fill(if is_t1_active { c.primary } else { Color::TRANSPARENT }).width(1.))
                                .on_press(move |_| {
                                    *active_tab.write() = TerminalViewTab::Shell(0);
                                })
                                .child(
                                    ui::row(5.)
                                        .child(label().text(">_").color(if is_t1_active { c.primary } else { c.text_secondary }).font_size(10.))
                                        .child(ui::mono("Terminal 1", 11., if is_t1_active { c.text_primary } else { c.text_secondary })),
                                ),
                        )
                    }
                )
                // Onglets supplémentaires créés dynamiquement via '+'
                .children(
                    secondary_terminals.read().iter().enumerate().map(|(idx, _)| {
                        let term_num = idx + 2;
                        let is_active = active_tab_val == TerminalViewTab::Shell(idx + 1);
                        let mut active_tab_sel = active_tab;
                        let mut secondary_terminals_del = secondary_terminals;
                        let mut active_tab_del = active_tab;

                        ui::clickable(
                            rect()
                                .padding(Gaps::new(2., 8., 2., 8.))
                                .background(if is_active { c.surface_secondary } else { c.surface_primary })
                                .corner_radius(3.)
                                .border(Border::new().fill(if is_active { c.primary } else { Color::TRANSPARENT }).width(1.))
                                .horizontal()
                                .cross_align(Alignment::Center)
                                .spacing(6.)
                                .on_press(move |_| {
                                    *active_tab_sel.write() = TerminalViewTab::Shell(idx + 1);
                                })
                                .child(
                                    ui::row(4.)
                                        .child(label().text(">_").color(if is_active { c.primary } else { c.text_secondary }).font_size(10.))
                                        .child(ui::mono(format!("Terminal {}", term_num), 11., if is_active { c.text_primary } else { c.text_secondary })),
                                )
                                .child(
                                    ui::clickable(
                                        rect()
                                            .padding(Gaps::new(1., 4., 1., 4.))
                                            .on_press(move |_| {
                                                let mut list = secondary_terminals_del.write();
                                                if idx < list.len() {
                                                    list.remove(idx);
                                                    *active_tab_del.write() = TerminalViewTab::Shell(0);
                                                }
                                            })
                                            .child(label().text("✕").color(c.text_placeholder).font_size(9.)),
                                    )
                                ),
                        )
                    })
                )
                .child(
                    // Bouton + pour ouvrir un nouveau shell
                    {
                        let mut secondary_terminals = secondary_terminals;
                        let mut active_tab = active_tab;
                        ui::clickable(
                            rect()
                                .padding(Gaps::new(2., 7., 2., 7.))
                                .background(c.surface_primary)
                                .corner_radius(3.)
                                .border(Border::new().fill(c.border).width(1.))
                                .on_press(move |_| {
                                    if let Some(new_handle) = spawn_terminal() {
                                        let mut list = secondary_terminals.write();
                                        list.push(new_handle);
                                        let new_idx = list.len();
                                        *active_tab.write() = TerminalViewTab::Shell(new_idx);
                                    }
                                })
                                .child(label().text("+").color(c.text_primary).font_size(12.)),
                        )
                    }
                )
                .child(ui::spacer())
                .child(
                    // Bouton Clear rapide (actif sur le terminal courant)
                    ui::clickable(
                        rect()
                            .padding(Gaps::new(2., 8., 2., 8.))
                            .background(c.surface_secondary)
                            .corner_radius(3.)
                            .border(Border::new().fill(c.border).width(1.))
                            .on_press(move |_| {
                                match active_tab_val {
                                    TerminalViewTab::Shell(0) => {
                                        if let Some(h) = handle_for_clear.read().clone() {
                                            let _ = h.write_key(&Key::Character("l".into()), Modifiers::CONTROL);
                                        }
                                    }
                                    TerminalViewTab::Shell(idx) => {
                                        let list = secondary_for_clear.read();
                                        if let Some(h) = list.get(idx - 1) {
                                            let _ = h.write_key(&Key::Character("l".into()), Modifiers::CONTROL);
                                        }
                                    }
                                }
                            })
                            .child(ui::mono("Clear", 10.5, c.text_secondary)),
                    )
                ),
        )
        .child(
            match active_tab_val {
                TerminalViewTab::Shell(0) => {
                    if let Some(h) = handle.read().clone() {
                        render_single_terminal(h, a11y_id, &c, dimensions, click_origin)
                    } else {
                        rect().padding(12.).child(ui::mono("Terminal 1 arrêté.", 12., c.text_secondary)).into_element()
                    }
                }
                TerminalViewTab::Shell(idx) => {
                    let list = secondary_terminals.read();
                    if let Some(h) = list.get(idx - 1).cloned() {
                        render_single_terminal(h, a11y_id, &c, dimensions, click_origin)
                    } else {
                        rect().padding(12.).child(ui::mono("Terminal non disponible.", 12., c.text_secondary)).into_element()
                    }
                }
            }
        )
}

/// Renders a single active terminal shell instance.
fn render_single_terminal(
    handle: TerminalHandle,
    a11y_id: AccessibilityId,
    c: &ColorsSheet,
    dimensions: State<(f32, f32)>,
    click_origin: State<Option<(usize, usize)>>,
) -> Element {
    let mut dims_measure = dimensions;
    let mut click_down = click_origin;
    let mut click_up = click_origin;

    rect()
        .width(Size::fill())
        .height(Size::flex(1.))
        .child(
            Terminal::new(handle.clone())
                .on_measured(move |(char_width, line_height)| {
                    dims_measure.set((char_width, line_height));
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
                        click_down.set(Some((row as usize, col as usize)));
                        let button = match e.button {
                            Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                            Some(MouseButton::Right) => TerminalMouseButton::Right,
                            _ => TerminalMouseButton::Left,
                        };
                        let selection_type = match EventsCombos::pressed(e.element_location) {
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
                        let origin = click_up();
                        click_up.set(None);
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
        .width(Size::fill())
        .height(Size::flex(1.))
        .background(c.surface_secondary)
        .padding(6.)
        .into_element()
}
