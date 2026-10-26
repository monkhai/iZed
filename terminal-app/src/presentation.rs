//! Platform-independent terminal landing UI, shared by iOS and the web preview.
//! Native services and application state stay with the caller.

use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
pub struct Palette {
    pub text: Hsla,
    pub muted: Hsla,
    pub surface: Hsla,
    pub border: Hsla,
    pub green: Hsla,
    pub element_background: Hsla,
    pub element_selected: Hsla,
}

pub fn button(id: &'static str, label: &'static str, palette: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(44.))
        .min_w(px(42.))
        .px(px(10.))
        .rounded(px(5.))
        .text_size(px(14.))
        .text_color(palette.text)
        .active(move |s| s.bg(palette.element_selected))
        .child(label)
}

pub fn session_header(
    name: impl Into<SharedString>,
    connected: bool,
    palette: Palette,
    on_menu: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_add: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .flex_shrink_0()
        .h(px(52.))
        .px(px(12.))
        .bg(palette.surface)
        .border_b_1()
        .border_color(palette.border)
        .child(
            div()
                .id("session-menu")
                .flex()
                .items_center()
                .gap(px(9.))
                .h_full()
                .px(px(4.))
                .child(div().size(px(7.)).rounded_full().bg(if connected {
                    palette.green
                } else {
                    palette.muted
                }))
                .child(name.into())
                .child(div().text_color(palette.muted).child("⌄"))
                .on_click(on_menu),
        )
        .child(
            button("add-connection", "+", palette)
                .text_size(px(24.))
                .on_click(on_add),
        )
}

pub fn machines_empty_state(
    palette: Palette,
    on_browse: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .px(px(22.))
        .py(px(26.))
        .gap(px(18.))
        .child(div().text_size(px(18.)).child("Machines"))
        .child(
            div()
                .text_size(px(12.))
                .text_color(palette.muted)
                .child("Choose a computer on your tailnet."),
        )
        .child(
            button("browse-machines", "Browse machines  →", palette)
                .bg(palette.element_background)
                .border_1()
                .border_color(palette.border)
                .on_click(on_browse),
        )
}

/// Terminal accessory keys. The caller owns key repeat and terminal input.
pub fn key_strip(
    palette: Palette,
    control: bool,
    on_key: impl Fn(&&'static str, &mut Window, &mut App) + 'static,
) -> Div {
    let on_key = std::rc::Rc::new(on_key);
    let mut strip = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(2.))
        .flex_shrink_0()
        .px(px(6.))
        .py(px(5.))
        .bg(palette.surface)
        .border_t_1()
        .border_color(palette.border);
    for (id, label, key) in [
        ("escape", "esc", "escape"),
        ("tab", "tab", "tab"),
        ("control", "ctrl", "control"),
        ("left", "←", "left"),
        ("down", "↓", "down"),
        ("up", "↑", "up"),
        ("right", "→", "right"),
    ] {
        let on_key = on_key.clone();
        strip = strip.child(
            button(id, label, palette)
                .flex_1()
                .min_w_0()
                .when(key == "control" && control, |el| {
                    el.bg(palette.element_selected).text_color(palette.green)
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    on_key(&key, window, cx)
                }),
        );
    }
    strip
}
