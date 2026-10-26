//! First Android terminal milestone: shared production presentation, local UI only.
//! No shell, SSH, Tailscale, account or credential service is initialized here.

#[path = "../../terminal-app/src/presentation.rs"]
mod presentation;

use gpui::{prelude::*, *};
use std::borrow::Cow;

#[derive(Default)]
struct PreviewState {
    preview: bool,
    menu: bool,
    control: bool,
    inputs: usize,
    last_key: String,
}

impl PreviewState {
    fn back(&mut self) {
        if self.menu {
            self.menu = false;
        } else {
            self.preview = false;
            self.control = false;
        }
    }

    fn key(&mut self, key: &str) {
        match key {
            "escape" => self.back(),
            "control" => self.control = !self.control,
            _ => {
                self.inputs += 1;
                self.last_key = format!("{}{}", if self.control { "ctrl+" } else { "" }, key);
                self.control = false;
            }
        }
    }
}

struct AndroidTerminal {
    state: PreviewState,
    focus: FocusHandle,
}

// Vercel Dark colors from app/themes/vercel-theme.json, also used by the iOS terminal.
fn palette() -> presentation::Palette {
    presentation::Palette {
        text: rgb(0xededed).into(),
        muted: rgb(0xa0a0a0).into(),
        surface: rgb(0x000000).into(),
        border: rgb(0x2e2e2e).into(),
        green: rgb(0x00ac3a).into(),
        element_background: rgb(0x1f1f1f).into(),
        element_selected: rgb(0x1a1a1a).into(),
    }
}

impl Render for AndroidTerminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.focus(&self.focus, cx);
        let p = palette();
        let insets = ized_platform::android::jni::platform()
            .and_then(|platform| platform.primary_window())
            .map(|window| window.safe_area_insets_logical());
        let mut content = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .bg(rgb(0x000000))
            .text_color(p.text)
            .text_size(px(13.))
            .font_family("JetBrains Mono")
            .pt(px(insets.as_ref().map_or(0., |i| i.top)))
            .pb(px(insets.as_ref().map_or(0., |i| i.bottom)))
            .pl(px(insets.as_ref().map_or(0., |i| i.left)))
            .pr(px(insets.as_ref().map_or(0., |i| i.right)))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.state.back();
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(presentation::session_header(
                if self.state.preview {
                    "Local preview"
                } else {
                    "iZed Terminal"
                },
                false,
                p,
                cx.listener(|this, _, _, cx| {
                    this.state.menu = !this.state.menu;
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.state.preview = false;
                    this.state.menu = false;
                    this.state.control = false;
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .px(px(18.))
                    .py(px(10.))
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Android UI preview · no remote connection"),
            );

        if self.state.menu {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .p(px(12.))
                    .bg(p.element_background)
                    .child(div().child("No SSH sessions connected"))
                    .child(
                        presentation::button("reset-preview", "Reset preview", p).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.state = PreviewState::default();
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        presentation::button("close-menu", "Close menu", p).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.state.menu = false;
                                cx.notify();
                            },
                        )),
                    ),
            );
        }

        if self.state.preview {
            content =
                content.child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .p(px(10.))
                        .bg(rgb(0x111111))
                        .child(div().text_size(px(14.)).child("Local input preview"))
                        .child(div().text_color(p.muted).child(
                            "Accessory keys update this local fixture. No shell is running.",
                        ))
                        .child(div().child(format!("Input events: {}", self.state.inputs)))
                        .child(div().child(format!(
                            "Last key: {}",
                            if self.state.last_key.is_empty() {
                                "none".to_string()
                            } else {
                                self.state.last_key.clone()
                            }
                        )))
                        .child(
                            presentation::button("back-machines", "Back to machines", p).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.state.preview = false;
                                    this.state.control = false;
                                    cx.notify();
                                }),
                            ),
                        ),
                );
            content = content.child(presentation::key_strip(
                p,
                self.state.control,
                cx.listener(|this, key: &&'static str, _, cx| {
                    this.state.key(key);
                    cx.notify();
                }),
            ));
        } else {
            content = content.child(presentation::machines_empty_state(p,
                cx.listener(|this, _, _, cx| {
                    this.state.preview = true;
                    this.state.menu = false;
                    cx.notify();
                })))
                .child(div().px(px(22.)).pb(px(18.)).text_size(px(11.)).text_color(p.muted)
                    .child("Browse opens a local preview. Machine discovery and SSH are not available yet."));
        }
        content
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../fonts/JetBrainsMono-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../fonts/JetBrainsMono-Medium.ttf")),
        ])
        .expect("load terminal fonts");
    cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|cx| AndroidTerminal {
            state: PreviewState::default(),
            focus: cx.focus_handle(),
        })
    })
    .expect("open Android terminal UI");
    log::info!("Android terminal UI is live (local preview only)");
    cx.activate(true);
}
