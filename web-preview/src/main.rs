#![cfg_attr(target_family = "wasm", no_main)]

#[path = "../../terminal-app/src/presentation.rs"]
mod presentation;
mod text_input;

use gpui::{prelude::*, *};
use presentation::Palette;
use std::{borrow::Cow, time::Duration};
use text_input::TextInput;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Scene {
    Landing,
    SignIn,
    Authorize,
    Machines,
    Empty,
    RefreshError,
    Connect,
    Verify,
    ChangedIdentity,
    Connecting,
    Terminal,
    SessionMenu,
    Keyboard,
    ConnectionError,
    Disconnected,
    SignOut,
}
impl Scene {
    const ALL: [Self; 16] = [
        Self::Landing,
        Self::SignIn,
        Self::Authorize,
        Self::Machines,
        Self::Empty,
        Self::RefreshError,
        Self::Connect,
        Self::Verify,
        Self::ChangedIdentity,
        Self::Connecting,
        Self::Terminal,
        Self::SessionMenu,
        Self::Keyboard,
        Self::ConnectionError,
        Self::Disconnected,
        Self::SignOut,
    ];
    fn id(self) -> &'static str {
        match self {
            Self::Landing => "landing",
            Self::SignIn => "sign-in",
            Self::Authorize => "authorize",
            Self::Machines => "machines",
            Self::Empty => "empty",
            Self::RefreshError => "refresh-error",
            Self::Connect => "connect",
            Self::Verify => "verify",
            Self::ChangedIdentity => "changed-identity",
            Self::Connecting => "connecting",
            Self::Terminal => "terminal",
            Self::SessionMenu => "session-menu",
            Self::Keyboard => "keyboard",
            Self::ConnectionError => "connection-error",
            Self::Disconnected => "disconnected",
            Self::SignOut => "sign-out",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Landing => "Landing",
            Self::SignIn => "Tailscale sign-in",
            Self::Authorize => "Authorize device",
            Self::Machines => "Machines",
            Self::Empty => "Empty tailnet",
            Self::RefreshError => "Refresh error",
            Self::Connect => "Connection details",
            Self::Verify => "Verify host identity",
            Self::ChangedIdentity => "Host identity changed",
            Self::Connecting => "Connecting",
            Self::Terminal => "Terminal session",
            Self::SessionMenu => "Session controls",
            Self::Keyboard => "Keyboard & key strip",
            Self::ConnectionError => "SSH access error",
            Self::Disconnected => "Disconnected",
            Self::SignOut => "Sign out confirmation",
        }
    }
    fn source(self) -> &'static str {
        match self {
            Self::Landing => "Shared production GPUI",
            Self::Terminal
            | Self::SessionMenu
            | Self::Keyboard
            | Self::Connecting
            | Self::Disconnected
            | Self::ConnectionError => "Shared GPUI chrome · simulated terminal",
            _ => "UIKit / system UI represented in GPUI",
        }
    }
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Terminal
                | Self::SessionMenu
                | Self::Keyboard
                | Self::Connecting
                | Self::ConnectionError
                | Self::Disconnected
                | Self::Verify
                | Self::ChangedIdentity
        )
    }
}
#[derive(Clone)]
struct Machine {
    name: &'static str,
    address: &'static str,
    os: &'static str,
    online: bool,
}
fn machines() -> Vec<Machine> {
    vec![
        Machine {
            name: "Studio Mac",
            address: "studio-mac.demo.ts.net",
            os: "macOS",
            online: true,
        },
        Machine {
            name: "Build Server",
            address: "build-server.demo.ts.net",
            os: "Linux",
            online: true,
        },
        Machine {
            name: "MacBook Pro",
            address: "macbook.demo.ts.net",
            os: "macOS",
            online: false,
        },
    ]
}

struct TerminalPreview {
    palette: Palette,
    scene: Scene,
    scenes_open: bool,
    selected: usize,
    signed_in: bool,
    saved: Vec<usize>,
    session: bool,
    menu: bool,
    keyboard: bool,
    control: bool,
    advanced: bool,
    error: String,
    notice: String,
    refreshing: bool,
    generation: u64,
    search: Entity<TextInput>,
    username: Entity<TextInput>,
    command: Entity<TextInput>,
    device_name: Entity<TextInput>,
    shell: Entity<TextInput>,
    transcript: Vec<String>,
    terminal_scroll: ScrollHandle,
    _search_observer: Subscription,
}
fn palette() -> Palette {
    let theme: serde_json::Value =
        serde_json::from_str(include_str!("../../app/themes/vercel-theme.json")).expect("theme");
    let style = &theme["themes"][0]["style"];
    let color = |key: &str| {
        Hsla::from(rgb(u32::from_str_radix(
            style[key]
                .as_str()
                .expect("theme color")
                .trim_start_matches('#'),
            16,
        )
        .expect("hex")))
    };
    Palette {
        text: color("text"),
        muted: color("text.muted"),
        surface: color("tab_bar.background"),
        border: color("border"),
        green: color("terminal.ansi.green"),
        element_background: color("element.background"),
        element_selected: color("element.selected"),
    }
}
impl TerminalPreview {
    fn new(cx: &mut Context<Self>) -> Self {
        let input = |cx: &mut Context<Self>, value: &str, placeholder: &str| {
            cx.new(|cx| TextInput::new(value, placeholder, cx))
        };
        let search = input(cx, "", "Find a machine");
        let observer = cx.observe(&search, |_, _, cx| cx.notify());
        let requested = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default();
        let scene = Scene::ALL
            .into_iter()
            .find(|s| {
                requested
                    .trim_start_matches('?')
                    .split('&')
                    .any(|p| p == format!("scene={}", s.id()))
            })
            .unwrap_or(Scene::Landing);
        let mut this = Self {
            palette: palette(),
            scene,
            scenes_open: false,
            selected: 0,
            signed_in: false,
            saved: vec![],
            session: false,
            menu: false,
            keyboard: false,
            control: false,
            advanced: false,
            error: String::new(),
            notice: String::new(),
            refreshing: false,
            generation: 0,
            username: input(cx, "", "Your account on this machine"),
            command: input(cx, "", "e.g. tmux new-session -A -s phone"),
            device_name: input(cx, "iZed iPhone", "Name this iPhone in Tailscale"),
            shell: input(cx, "", "Type help, ls, pwd, whoami…"),
            search,
            transcript: vec![],
            terminal_scroll: ScrollHandle::new(),
            _search_observer: observer,
        };
        this.jump(scene, cx);
        this
    }
    fn go(&mut self, scene: Scene, cx: &mut Context<Self>) {
        self.generation += 1;
        self.scene = scene;
        self.scenes_open = false;
        self.menu = scene == Scene::SessionMenu;
        self.keyboard = scene == Scene::Keyboard;
        if self.keyboard {
            self.terminal_scroll.scroll_to_bottom();
        }
        self.error.clear();
        self.notice.clear();
        self.refreshing = false;
        if let Some(w) = web_sys::window() {
            if let Ok(h) = w.history() {
                let _ = h.replace_state_with_url(
                    &wasm_bindgen::JsValue::NULL,
                    "",
                    Some(&format!("?scene={}", scene.id())),
                );
            }
        }
        cx.notify();
    }
    fn jump(&mut self, scene: Scene, cx: &mut Context<Self>) {
        self.go(scene, cx);
        self.signed_in = !matches!(scene, Scene::Landing | Scene::SignIn | Scene::Authorize);
        self.session = scene.terminal();
        self.saved = if self.signed_in { vec![0] } else { vec![] };
        self.search.update(cx, |field, cx| field.set("", cx));
        if scene.terminal() {
            self.seed_terminal();
        }
    }
    fn seed_terminal(&mut self) {
        self.transcript = vec![
            "Last login: Sat Oct 24 09:41:00".into(),
            format!("Connected to {}", machines()[self.selected].name),
            "".into(),
            format!(
                "demo@{} ~ % pwd",
                machines()[self.selected]
                    .name
                    .to_lowercase()
                    .replace(' ', "-")
            ),
            "/Users/demo".into(),
            format!(
                "demo@{} ~ % ls",
                machines()[self.selected]
                    .name
                    .to_lowercase()
                    .replace(' ', "-")
            ),
            "Applications  Desktop  Developer".into(),
            "Documents     Downloads".into(),
            "".into(),
            "Interactive demo shell — type help".into(),
        ];
    }
    fn browse(&mut self, cx: &mut Context<Self>) {
        self.go(
            if self.signed_in {
                Scene::Machines
            } else {
                Scene::SignIn
            },
            cx,
        );
    }
    fn open_machine(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = index;
        if !machines()[index].online {
            self.go(Scene::ConnectionError, cx);
            self.error = "The machine is offline in Tailscale.".into();
        } else if self.saved.contains(&index) {
            self.begin_connect(cx);
        } else {
            self.username.update(cx, |f, cx| f.set("", cx));
            self.advanced = false;
            self.go(Scene::Connect, cx);
        }
    }
    fn connect(&mut self, cx: &mut Context<Self>) {
        let user = self.username.read(cx).text();
        if user.trim().is_empty() || user.contains('@') || user.chars().any(char::is_whitespace) {
            self.error = "Enter your SSH username.".into();
            cx.notify();
            return;
        }
        self.go(Scene::Verify, cx);
    }
    fn begin_connect(&mut self, cx: &mut Context<Self>) {
        self.go(Scene::Connecting, cx);
        let generation = self.generation;
        let timer = cx.background_executor().timer(Duration::from_millis(1100));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.session = true;
                    if !this.saved.contains(&this.selected) {
                        this.saved.push(this.selected);
                    }
                    this.seed_terminal();
                    this.go(Scene::Terminal, cx);
                }
            });
        })
        .detach();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.notice.clear();
        self.generation += 1;
        let generation = self.generation;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(700));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if generation == this.generation {
                    this.refreshing = false;
                    this.scene = Scene::Machines;
                    this.notice = "Your Tailscale machines · synced just now".into();
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn execute(&mut self, cx: &mut Context<Self>) {
        let command = self.shell.read(cx).text();
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        self.transcript.push(format!(
            "demo@{} ~ % {command}",
            machines()[self.selected]
                .name
                .to_lowercase()
                .replace(' ', "-")
        ));
        match command {
            "help" => self
                .transcript
                .push("Demo commands: help, ls, pwd, whoami, date, clear, exit".into()),
            "ls" => self.transcript.extend([
                "Applications  Desktop  Developer".into(),
                "Documents     Downloads".into(),
            ]),
            "pwd" => self.transcript.push("/Users/demo".into()),
            "whoami" => self.transcript.push("demo".into()),
            "date" => self
                .transcript
                .push("Sat Oct 24 09:41:00 2026 (demo clock)".into()),
            "clear" => self.transcript.clear(),
            "exit" => {
                self.go(Scene::Disconnected, cx);
                self.session = false;
            }
            _ if command.starts_with("echo ") => self.transcript.push(command[5..].to_owned()),
            _ => self
                .transcript
                .push(format!("demo: {command}: command not available; try help")),
        }
        if self.transcript.len() > 80 {
            self.transcript.drain(..40);
        }
        self.shell.update(cx, |f, cx| f.set("", cx));
        self.terminal_scroll.scroll_to_bottom();
        cx.notify();
    }
    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        presentation::button(id, label, self.palette)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| { cx.stop_propagation(); f(this, window, cx); }))
    }
    fn primary(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        self.action(id, label, cx, f)
            .w_full()
            .bg(self.palette.element_background)
            .border_1()
            .border_color(self.palette.border)
    }
    fn note(&self, text: impl Into<SharedString>) -> Div {
        div()
            .text_size(px(11.))
            .text_color(self.palette.muted)
            .child(text.into())
    }
    fn section(&self, title: &'static str) -> Div {
        div()
            .text_size(px(10.))
            .text_color(self.palette.muted)
            .pt(px(14.))
            .pb(px(6.))
            .child(title)
    }
    fn nav(&self, title: &'static str, cx: &mut Context<Self>) -> Div {
        div()
            .h(px(52.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px(px(10.))
            .border_b_1()
            .border_color(self.palette.border)
            .child(self.action("back", "‹ Back", cx, |this, _, cx| {
                let dest = match this.scene {
                    Scene::Connect => Scene::Machines,
                    Scene::Authorize => Scene::SignIn,
                    _ if this.session => Scene::Terminal,
                    _ => Scene::Landing,
                };
                this.go(dest, cx);
            }))
            .child(div().text_size(px(16.)).child(title))
            .child(self.action("refresh", "R", cx, |this, _, cx| this.refresh(cx)))
    }
    fn machine_row(&self, index: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        let m = machines()[index].clone();
        let saved = self.saved.contains(&index);
        div()
            .id(("machine", index))
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(76.))
            .flex_shrink_0()
            .px(px(18.))
            .border_b_1()
            .border_color(self.palette.border)
            .cursor_pointer()
            .active(|s| s.bg(self.palette.element_selected))
            .child(
                div()
                    .w(px(26.))
                    .text_color(self.palette.muted)
                    .child(if saved { ">_" } else { "[]" }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(m.name)
                    .child(self.note(if saved {
                        format!("demo@{}", m.address)
                    } else {
                        format!(
                            "{} · {} in Tailscale",
                            m.os,
                            if m.online { "Online" } else { "Offline" }
                        )
                    })),
            )
            .child(
                div()
                    .text_color(if m.online {
                        self.palette.green
                    } else {
                        self.palette.muted
                    })
                    .child(if m.online { "●" } else { "›" }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_machine(index, cx)))
    }
    fn machine_picker(&self, cx: &mut Context<Self>) -> Div {
        let scene = self.scene;
        let query = self.search.read(cx).text().to_lowercase();
        let visible = machines()
            .into_iter()
            .enumerate()
            .filter(|(_, m)| {
                format!("{} {} {}", m.name, m.address, m.os)
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let signed = scene != Scene::SignIn && scene != Scene::Authorize;
        let mut content=div().flex().flex_col().flex_1().min_h_0().child(self.nav("Machines",cx))
            .child(div().id("picker-scroll").flex_1().min_h_0().overflow_y_scroll().flex().flex_col()
                .child(div().p(px(18.)).flex().flex_col().gap(px(12.)).child(self.search.clone())
                    .child(self.note(if self.refreshing{"Refreshing Tailscale machines…"}else if scene==Scene::Empty{"No computers are available in this tailnet yet."}else if scene==Scene::RefreshError{"Could not refresh Tailscale machines. Try again."}else if signed{"Your Tailscale machines · synced just now\nConnected through iZed Terminal"}else{"Connect to Tailscale to see your machines."})))
                .when(signed&&scene!=Scene::Empty,|mut el|{
                    let recent=visible.iter().filter(|i|self.saved.contains(i)).copied().collect::<Vec<_>>();
                    let peers=visible.iter().filter(|i|!self.saved.contains(i)).copied().collect::<Vec<_>>();
                    if !recent.is_empty(){el=el.child(self.section("  RECENT CONNECTIONS"));for i in recent{el=el.child(self.machine_row(i,cx));}}
                    if !peers.is_empty(){el=el.child(self.section("  ON YOUR TAILNET"));for i in peers{el=el.child(self.machine_row(i,cx));}}
                    if visible.is_empty(){el=el.child(div().p(px(22.)).child(self.note("No machines match your search.")));}el
                })
                .child(div().p(px(18.)).flex().flex_col().gap(px(8.)).child(self.device_name.clone())
                    .child(self.action("tailscale-action",if signed{"Save device name"}else{"Connect to Tailscale"},cx,move|this,_,cx|{if signed{this.notice="Device name saved for this demo.".into();cx.notify();}else{this.go(Scene::Authorize,cx);}}))
                    .child(self.note(if signed{"This name identifies iZed Terminal in your tailnet."}else{"Sign in once. iZed Terminal appears as its own device in your tailnet."}))
                    .when(signed,|el|el.child(self.action("sign-out","Sign out of Tailscale",cx,|this,_,cx|this.go(Scene::SignOut,cx)).text_color(rgb(0xf32e40))))
                    .when(!self.notice.is_empty(),|el|el.child(self.note(self.notice.clone())))));
        if scene == Scene::SignOut {
            content = content.child(self.dialog(
                "Sign out of Tailscale?",
                "iZed Terminal will need to sign in again to reach your tailnet.",
                "Sign Out",
                cx,
                |this, _, cx| {
                    this.signed_in = false;
                    this.session = false;
                    this.go(Scene::SignIn, cx);
                },
                |this, _, cx| this.go(Scene::Machines, cx),
            ));
        }
        content
    }
    fn connection_editor(&self, cx: &mut Context<Self>) -> Div {
        let m = machines()[self.selected].clone();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(self.nav("Connect", cx))
            .child(
                div()
                    .id("connection-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(div().text_size(px(18.)).child(m.name))
                    .child(self.note(format!("{} · {}", m.os, m.address)))
                    .child(self.section("SSH USERNAME"))
                    .child(self.username.clone())
                    .child(self.note("We’ll remember this for next time."))
                    .child(
                        self.action(
                            "advanced",
                            "Launch command (optional)",
                            cx,
                            |this, _, cx| {
                                this.advanced = !this.advanced;
                                cx.notify();
                            },
                        )
                        .text_color(self.palette.muted),
                    )
                    .when(self.advanced, |el| el.child(self.command.clone()))
                    .when(!self.error.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0xf32e40))
                                .child(self.error.clone()),
                        )
                    })
                    .child(
                        self.primary("open-terminal", "Open terminal  →", cx, |this, _, cx| {
                            this.connect(cx)
                        })
                        .h(px(50.)),
                    )
                    .child(
                        self.action(
                            "copy-public-key",
                            "Copy this phone’s public SSH key",
                            cx,
                            |this, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    "DEMO PUBLIC SSH KEY — not a real credential".into(),
                                ));
                                this.notice = "Demo public-key placeholder copied.".into();
                                cx.notify();
                            },
                        )
                        .text_size(px(10.))
                        .text_color(self.palette.muted),
                    )
                    .when(!self.notice.is_empty(), |el| {
                        el.child(self.note(self.notice.clone()))
                    }),
            )
    }
    fn dialog(
        &self,
        title: &'static str,
        message: impl Into<SharedString>,
        confirm: &'static str,
        cx: &mut Context<Self>,
        yes: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        no: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Div {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000aa))
            .child(
                div()
                    .w(px(300.))
                    .max_w_full()
                    .rounded(px(14.))
                    .bg(rgb(0x242424))
                    .border_1()
                    .border_color(self.palette.border)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(div().text_size(px(16.)).child(title))
                            .child(div().text_size(px(11.)).child(message.into())),
                    )
                    .child(
                        self.action("dialog-confirm", confirm, cx, yes)
                            .border_t_1()
                            .border_color(self.palette.border),
                    )
                    .child(
                        self.action("dialog-cancel", "Cancel", cx, no)
                            .border_t_1()
                            .border_color(self.palette.border),
                    ),
            )
    }
    fn authorization(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .p(px(22.))
            .gap(px(20.))
            .child(div().text_size(px(18.)).child("Authorize iZed Terminal"))
            .child(self.note("External sign-in · interactive demo"))
            .child(
                div().child(
                    "Tailscale opens a browser to sign in and add this iPhone to your tailnet.",
                ),
            )
            .child(self.device_name.clone())
            .child(self.primary(
                "authorize-demo",
                "Authorize demo device",
                cx,
                |this, _, cx| {
                    this.signed_in = true;
                    this.go(Scene::Machines, cx);
                },
            ))
            .child(
                self.action("cancel-authorization", "Cancel", cx, |this, _, cx| {
                    this.go(Scene::SignIn, cx)
                }),
            )
            .child(
                self.note(
                    "No account is contacted. Machines and authorization are local fixtures.",
                ),
            )
    }
    fn terminal_body(&self, cx: &mut Context<Self>) -> Div {
        let p = self.palette;
        let active = matches!(
            self.scene,
            Scene::Terminal | Scene::SessionMenu | Scene::Keyboard
        );
        let m = machines()[self.selected].clone();
        let mut content=div().flex_1().min_h_0().flex().flex_col().relative()
            .child(presentation::session_header(m.name,active,p,cx.listener(|this,_,_,cx|{this.menu=!this.menu;cx.notify();}),cx.listener(|this,_,_,cx|this.browse(cx))))
            .child(div().id("terminal-transcript").track_scroll(&self.terminal_scroll).flex_1().min_h_0().overflow_y_scroll().px(px(10.)).py(px(8.)).text_size(px(12.)).line_height(px(19.)).flex().flex_col()
                .children(self.transcript.iter().map(|s|div().flex_shrink_0().child(if s.is_empty(){" ".to_owned()}else{s.clone()})))
                .when(active,|el|el.child(div().flex_shrink_0().mt(px(8.)).child(self.note(format!("demo@{} ~ %", m.name.to_lowercase().replace(' ', "-")))).child(self.shell.clone())
                    .child(self.action("run-demo-command","Run",cx,|this,_,cx|this.execute(cx)).text_size(px(11.))))))
            .when(active && !self.notice.is_empty(), |el|el.child(self.note(self.notice.clone()).px(px(10.))))
            .when(!active,|el|el.child(div().px(px(14.)).py(px(10.)).border_t_1().border_color(p.border).flex().flex_col().gap(px(8.))
                .child(self.note(match self.scene {Scene::Connecting=>"Connecting…",Scene::Verify|Scene::ChangedIdentity=>"Waiting for host verification…",Scene::Disconnected=>"Connection closed.",_=>"Permission denied (publickey). Authorize this phone’s key on the host."}))
                .when(self.scene==Scene::ConnectionError||self.scene==Scene::Disconnected,|el|el.child(self.action("reconnect","Reconnect",cx,|this,_,cx|this.begin_connect(cx))).child(self.action("copy-access-key","Copy SSH key",cx,|this,_,cx|{cx.write_to_clipboard(ClipboardItem::new_string("DEMO PUBLIC SSH KEY — not a real credential".into()));this.notice="Demo public-key placeholder copied.".into();cx.notify();})))
                .when(!self.notice.is_empty(),|el|el.child(self.note(self.notice.clone())))))
            .child(presentation::key_strip(p,self.control,cx.listener(|this,key,_,cx|{match *key {"control"=>this.control=!this.control,"escape"=>{this.menu=false;},"tab"=>{this.notice="Tab sent to demo terminal".into();},"up"=>{if let Some(line)=this.transcript.iter().rev().find(|s|s.contains(" ~ % ")){let command=line.split(" ~ % ").nth(1).unwrap_or("").to_owned();this.shell.update(cx,|f,cx|f.set(&command,cx));}},_=>this.notice=format!("{key} key sent to demo terminal")};cx.notify();})))
            .when(self.keyboard,|el|el.child(self.keyboard_view(cx)));
        if self.menu {
            let mut menu = div()
                .absolute()
                .top(px(52.))
                .left(px(10.))
                .right(px(10.))
                .bg(rgb(0x111111))
                .border_1()
                .border_color(p.border)
                .rounded(px(6.))
                .shadow_md()
                .p(px(5.))
                .flex()
                .flex_col()
                .gap(px(3.));
            for index in self.saved.iter().copied() {
                let host = machines()[index].clone();
                menu = menu.child(
                    div()
                        .id(("saved-session", index))
                        .h(px(56.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .cursor_pointer()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(host.name)
                                .child(self.note(format!("demo@{}", host.address))),
                        )
                        .child(div().text_color(p.green).child("●"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected = index;
                            this.menu = false;
                            this.go(Scene::Terminal, cx);
                        })),
                );
            }
            menu = menu
                .child(
                    div()
                        .flex()
                        .border_t_1()
                        .border_color(p.border)
                        .child(
                            self.action("paste", "Paste", cx, |this, _, cx| {
                                this.shell
                                    .update(cx, |f, cx| f.set("echo pasted demo text", cx));
                                this.menu = false;
                                cx.notify();
                            })
                            .flex_1(),
                        )
                        .child(
                            self.action("copy", "Copy", cx, |this, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    this.transcript.join("\n"),
                                ));
                                this.notice = "Demo terminal output copied.".into();
                                this.menu = false;
                                cx.notify();
                            })
                            .flex_1(),
                        )
                        .child(
                            self.action("keyboard", "Keyboard", cx, |this, _, cx| {
                                this.keyboard = !this.keyboard;
                                if this.keyboard {
                                    this.terminal_scroll.scroll_to_bottom();
                                }
                                this.menu = false;
                                cx.notify();
                            })
                            .flex_1(),
                        ),
                )
                .child(
                    self.action("disconnect", "Disconnect", cx, |this, _, cx| {
                        this.session = false;
                        this.go(Scene::Disconnected, cx);
                    })
                    .text_color(p.muted),
                );
            content = content.child(menu);
        }
        if matches!(self.scene, Scene::Verify | Scene::ChangedIdentity) {
            content=content.child(self.dialog(if self.scene==Scene::Verify{"Verify host identity"}else{"Host identity changed"},format!("demo@{}\n\nSHA256:DEMO-FINGERPRINT-NOT-A-REAL-HOST\n\nCompare this fingerprint with your host before trusting it.", m.address),if self.scene==Scene::Verify{"Trust and connect"}else{"Trust new identity"},cx,|this,_,cx|this.begin_connect(cx),|this,_,cx|{this.session=false;this.go(Scene::Machines,cx);}));
        }
        content
    }
    fn keyboard_view(&self, cx: &mut Context<Self>) -> Div {
        let mut keyboard = div()
            .flex_shrink_0()
            .bg(rgb(0x202020))
            .p(px(5.))
            .flex()
            .flex_col()
            .gap(px(5.));
        for (row_index, row) in ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
            .into_iter()
            .enumerate()
        {
            let mut line = div().flex().justify_center().gap(px(3.));
            for (col, ch) in row.chars().enumerate() {
                line = line.child(
                    div()
                        .id(("soft-key", row_index * 12 + col))
                        .flex_1()
                        .max_w(px(35.))
                        .h(px(35.))
                        .bg(rgb(0x444444))
                        .rounded(px(4.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ch.to_string())
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut text = this.shell.read(cx).text();
                            text.push(ch);
                            this.shell.update(cx, |f, cx| f.set(&text, cx));
                            cx.notify();
                        })),
                );
            }
            keyboard = keyboard.child(line);
        }
        keyboard.child(
            div()
                .flex()
                .gap(px(5.))
                .child(self.action("hide-keyboard", "Hide", cx, |this, _, cx| {
                    this.keyboard = false;
                    cx.notify();
                }))
                .child(
                    self.action("space", "space", cx, |this, _, cx| {
                        let mut text = this.shell.read(cx).text();
                        text.push(' ');
                        this.shell.update(cx, |f, cx| f.set(&text, cx));
                    })
                    .flex_1()
                    .bg(rgb(0x444444)),
                )
                .child(self.action("return", "return", cx, |this, _, cx| this.execute(cx))),
        )
    }
    fn scene_list(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(3.));
        for scene in Scene::ALL {
            list = list.child(
                div()
                    .id(("scene", scene as usize))
                    .px(px(12.))
                    .py(px(10.))
                    .rounded(px(5.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if scene == self.scene {
                        self.palette.element_background
                    } else {
                        rgb(0x111111).into()
                    })
                    .child(scene.title())
                    .on_click(cx.listener(move |this, _, _, cx| this.jump(scene, cx))),
            );
        }
        list
    }
}
impl Render for TerminalPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let wide = window.viewport_size().width > px(900.);
        let scene = self.scene;
        let body = if scene.terminal() {
            self.terminal_body(cx)
        } else {
            match scene {
                Scene::Landing => div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(presentation::session_header(
                        "iZed Terminal",
                        false,
                        p,
                        cx.listener(|this, _, _, cx| this.jump(Scene::SessionMenu, cx)),
                        cx.listener(|this, _, _, cx| this.browse(cx)),
                    ))
                    .child(presentation::machines_empty_state(
                        p,
                        cx.listener(|this, _, _, cx| this.browse(cx)),
                    )),
                Scene::Connect => self.connection_editor(cx),
                Scene::Authorize => self.authorization(cx),
                _ => self.machine_picker(cx),
            }
        };
        let phone = div()
            .w(px(390.))
            .max_w_full()
            .h_full()
            .max_h(px(844.))
            .min_h_0()
            .flex()
            .flex_col()
            .relative()
            .bg(rgb(0x0a0a0a))
            .font_family("JetBrains Mono")
            .text_size(px(13.))
            .text_color(p.text)
            .child(
                div()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px(px(22.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.))
                    .child("9:41")
                    .child("● 100%"),
            )
            .child(body)
            .child(
                div()
                    .h(px(22.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().w(px(110.)).h(px(4.)).rounded_full().bg(p.text)),
            );
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x171717))
            .text_color(p.text)
            .font_family("JetBrains Mono")
            .text_size(px(12.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "enter"
                    && matches!(
                        this.scene,
                        Scene::Terminal | Scene::SessionMenu | Scene::Keyboard
                    )
                {
                    this.execute(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child("iZed Terminal · app demo")
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child("Local fixtures · no real login or SSH"),
                            ),
                    )
                    .child(self.action(
                        "scene-browser",
                        if self.scenes_open {
                            "Close scenes"
                        } else {
                            "Scenes"
                        },
                        cx,
                        |this, _, cx| {
                            this.scenes_open = !this.scenes_open;
                            cx.notify();
                        },
                    )),
            );
        if self.scenes_open && !wide {
            return root
                .child(
                    div()
                        .id("scene-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .p(px(12.))
                        .child(self.scene_list(cx)),
                )
                .into_any_element();
        }
        let mut area = div().flex_1().min_h_0().flex();
        if wide {
            area = area.child(
                div()
                    .id("sidebar")
                    .w(px(235.))
                    .h_full()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .border_r_1()
                    .border_color(p.border)
                    .child(self.scene_list(cx)),
            );
        }
        root = root.child(
            area.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .flex_shrink_0()
                            .px(px(14.))
                            .py(px(9.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(scene.title())
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(scene.source()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(phone),
                    ),
            ),
        );
        root.into_any_element()
    }
}
fn run() {
    gpui_platform::application().run(|cx| {
        text_input::bind_keys(cx);
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(
                include_bytes!("../../app/fonts/JetBrainsMono-Regular.ttf").as_slice(),
            )])
            .expect("font");
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(TerminalPreview::new)
        })
        .expect("window");
        cx.activate(true);
    });
}
#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    gpui_platform::web_init();
    run();
}
#[cfg(not(target_family = "wasm"))]
fn main() {
    run();
}
