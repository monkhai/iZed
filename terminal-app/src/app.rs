#[path = "presentation.rs"]
mod presentation;
#[cfg(feature = "native-catalog")]
#[path = "native_catalog.rs"]
mod native_catalog;

use crate::{
    ssh::{self, Host},
    tailnet::Roster,
};
#[cfg(target_os = "ios")]
use crate::ios_keys::private_key;
#[cfg(target_os = "android")]
use crate::android_keys::private_key;
use gpui::{font, prelude::*, *};
use settings::{Settings, SettingsStore};
use std::{
    borrow::Cow,
    ffi::{c_char, CStr, CString},
    path::PathBuf,
    sync::LazyLock,
    time::{Duration, Instant},
};
use terminal::{terminal_settings::TerminalSettings, Terminal, TerminalBounds, TerminalBuilder};
use theme::ActiveTheme;
use zeroize::Zeroizing;

unsafe extern "C" {
    fn terminal_machines(json: *const c_char, present: bool);
    fn terminal_show_trust(address: *const c_char, fingerprint: *const c_char, changed: bool);
    fn terminal_start_tailscale();
    fn terminal_refresh_tailscale();
    fn terminal_accessibility(value: *const c_char, x: f64, y: f64, width: f64, height: f64);
}

pub(crate) enum NativeEvent {
    Add(Host),
    Trust(bool),
    Refresh,
    #[cfg(target_os = "android")]
    Text(String),
    TailscaleUpdate(String, ssh::TailscaleProxy),
    TailscaleError(String),
}
static NATIVE_EVENTS: LazyLock<(
    smol::channel::Sender<NativeEvent>,
    smol::channel::Receiver<NativeEvent>,
)> = LazyLock::new(smol::channel::unbounded);

pub(crate) fn send_native_event(event: NativeEvent) {
    let _ = NATIVE_EVENTS.0.try_send(event);
}

#[no_mangle]
extern "C" fn terminal_refresh_machines() {
    send_native_event(NativeEvent::Refresh);
}

#[no_mangle]
unsafe extern "C" fn terminal_add_host(
    name: *const c_char,
    address: *const c_char,
    command: *const c_char,
) {
    let string = |p| {
        if p == std::ptr::null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().trim().to_owned()
        }
    };
    let address = string(address);
    let name = string(name);
    send_native_event(NativeEvent::Add(Host {
        name: if name.is_empty() {
            address.clone()
        } else {
            name
        },
        address,
        command: string(command),
        fingerprint: None,
    }));
}

#[no_mangle]
pub(crate) extern "C" fn terminal_trust_host(trust: bool) {
    send_native_event(NativeEvent::Trust(trust));
}

#[no_mangle]
unsafe extern "C" fn terminal_update_tailnet(
    status_json: *const c_char,
    proxy_host: *const c_char,
    proxy_port: u16,
    proxy_credential: *const c_char,
    error: *const c_char,
) {
    let string = |pointer: *const c_char| {
        if pointer.is_null() {
            String::new()
        } else {
            CStr::from_ptr(pointer).to_string_lossy().into_owned()
        }
    };
    let error = string(error);
    if !error.is_empty() {
        send_native_event(NativeEvent::TailscaleError(error));
        return;
    }
    send_native_event(NativeEvent::TailscaleUpdate(
        string(status_json),
        ssh::TailscaleProxy {
            host: string(proxy_host),
            port: proxy_port,
            password: Zeroizing::new(string(proxy_credential)),
        },
    ));
}

fn hosts_path() -> PathBuf {
    let file = if cfg!(debug_assertions)
        && (std::env::var("IZED_SMOKE_TEST").as_deref() == Ok("1")
            || std::env::var("IZED_PICKER_TEST").as_deref() == Ok("1"))
    {
        "terminal-test-hosts.json"
    } else {
        "terminal-hosts.json"
    };
    crate::tailnet::data_dir().join(file)
}

struct Session {
    host: usize,
    terminal: Entity<Terminal>,
    connected: bool,
    cancel: smol::channel::Sender<()>,
    _subscriptions: Vec<Subscription>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.try_send(()).ok();
    }
}

#[derive(Default)]
struct MenuMotion {
    progress: f32,
    velocity: f32,
    last_frame: Option<Instant>,
}

impl MenuMotion {
    fn update(&mut self, open: bool, window: &mut Window) -> f32 {
        let target = if open { 1. } else { 0. };
        #[cfg(target_os = "ios")]
        unsafe {
            unsafe extern "C" { fn UIAccessibilityIsReduceMotionEnabled() -> bool; }
            if UIAccessibilityIsReduceMotionEnabled() {
                self.progress = target;
                self.velocity = 0.;
                self.last_frame = None;
                return target;
            }
        }
        let now = Instant::now();
        let dt = self.last_frame.map_or(1. / 60., |last| now.duration_since(last).as_secs_f32().min(0.05));
        // Analytic critically damped spring: retain position and velocity when
        // the user reverses direction, without overshooting the header bounds.
        let displacement = self.progress - target;
        let term = self.velocity + 24. * displacement;
        let decay = (-24. * dt).exp();
        self.progress = target + (displacement + term * dt) * decay;
        self.velocity = (self.velocity - 24. * term * dt) * decay;
        if (self.progress - target).abs() < 0.001 && self.velocity.abs() < 0.01 {
            self.progress = target;
            self.velocity = 0.;
            self.last_frame = None;
        } else {
            self.last_frame = Some(now);
            window.request_animation_frame();
        }
        self.progress.clamp(0., 1.)
    }
}

struct PhoneTerminal {
    #[cfg(feature = "native-catalog")]
    preview: bool,
    hosts: Vec<Host>,
    sessions: Vec<Session>,
    active: Option<usize>,
    focus: FocusHandle,
    menu: bool,
    menu_motion: MenuMotion,
    tools_menu: bool,
    tools_notice: Option<&'static str>,
    control: bool,
    busy: bool,
    message: String,
    public_key: String,
    pending: Option<(usize, String)>,
    terminal_bounds: Bounds<Pixels>,
    selecting: bool,
    repeat_generation: u64,
    roster: Roster,
    refreshing: bool,
    roster_message: String,
    tailnet_ready: bool,
    tailscale_proxy: Option<ssh::TailscaleProxy>,
}

impl PhoneTerminal {
    fn new(cx: &mut Context<Self>) -> Self {
        let public_key = private_key().and_then(|key| {
            Ok(russh::keys::PrivateKey::from_openssh(key.as_str())?
                .public_key()
                .to_openssh()?)
        });
        let (public_key, message) = match public_key {
            Ok(key) => (key, String::new()),
            Err(error) => (String::new(), error.to_string()),
        };
        // Public half only: useful when pairing this device with a host.
        std::fs::create_dir_all(hosts_path().parent().unwrap()).ok();
        std::fs::write(
            hosts_path().with_file_name("device-public-key.pub"),
            &public_key,
        )
        .ok();
        // UIKit alerts return outside GPUI's entity leases. Drain them on our executor.
        let events = NATIVE_EVENTS.1.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |this, cx| match event {
                        #[cfg(target_os = "android")]
                        NativeEvent::Text(text) => {
                            for character in text.chars() {
                                let key = match character { '\n' | '\r' => "enter".into(), '\u{8}' => "backspace".into(), '\u{7f}' => "delete".into(), _ => character.to_string() };
                                this.send_key(Keystroke { key, key_char: Some(character.to_string()), modifiers: Modifiers::default() }, cx);
                            }
                        }
                        NativeEvent::Refresh => this.refresh_roster(cx),
                        NativeEvent::TailscaleUpdate(status, proxy) => {
                            match Roster::from_status(&status) {
                                Ok(roster) => {
                                    this.roster = roster;
                                    this.tailnet_ready = true;
                                    this.tailscale_proxy = Some(proxy);
                                    this.refreshing = false;
                                    this.roster_message.clear();
                                    if let Err(error) = this.roster.save() {
                                        this.roster_message = error.to_string();
                                    }
                                }
                                Err(error) => {
                                    this.refreshing = false;
                                    this.roster_message =
                                        format!("Could not read Tailscale machines: {error}");
                                }
                            }
                            this.picker(false);
                            cx.notify();
                        }
                        NativeEvent::TailscaleError(error) => {
                            this.refreshing = false;
                            this.tailnet_ready = false;
                            this.tailscale_proxy = None;
                            this.roster_message = error;
                            this.picker(false);
                            cx.notify();
                        }
                        NativeEvent::Add(host) => {
                            if let Err(error) = host.target() {
                                this.message = error.to_string();
                                cx.notify();
                                return;
                            }
                            if let Some(index) = this.hosts.iter().position(|h| {
                                h.address == host.address && h.command == host.command
                            }) {
                                this.connect(index, cx);
                            } else {
                                this.hosts.push(host);
                                this.save();
                                this.connect(this.hosts.len() - 1, cx);
                            }
                        }
                        NativeEvent::Trust(trust) => {
                            if let Some((index, fingerprint)) = this.pending.take() {
                                if trust {
                                    this.hosts[index].fingerprint = Some(fingerprint);
                                    this.save();
                                    this.open_session(index, cx);
                                } else {
                                    this.busy = false;
                                    this.message = "Connection cancelled".into();
                                    cx.notify();
                                }
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        unsafe { terminal_start_tailscale() };
        Self {
            #[cfg(feature = "native-catalog")]
            preview: false,
            hosts: std::fs::read(hosts_path())
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default(),
            sessions: Vec::new(),
            active: None,
            focus: cx.focus_handle(),
            menu: false,
            menu_motion: MenuMotion::default(),
            tools_menu: false,
            tools_notice: None,
            control: false,
            busy: false,
            message,
            public_key,
            pending: None,
            terminal_bounds: Bounds::default(),
            selecting: false,
            repeat_generation: 0,
            roster: Roster::load(),
            refreshing: false,
            roster_message: "Sign in to Tailscale to see your machines.".into(),
            tailnet_ready: false,
            tailscale_proxy: None,
        }
    }

    fn save(&mut self) {
        #[cfg(feature = "native-catalog")]
        if self.preview { return; }
        let result = (|| -> anyhow::Result<()> {
            let path = hosts_path();
            std::fs::create_dir_all(path.parent().unwrap())?;
            let temp = path.with_extension("tmp");
            std::fs::write(&temp, serde_json::to_vec_pretty(&self.hosts)?)?;
            std::fs::rename(temp, path)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.message = format!("Could not save connection: {error}");
        }
    }

    fn picker(&self, present: bool) {
        let payload = serde_json::json!({
            "roster": self.roster, "saved": self.hosts, "public_key": self.public_key,
            "refreshing": self.refreshing, "message": self.roster_message,
            "has_session": self.active.is_some(),
            "tailnet_ready": self.tailnet_ready,
        });
        if let Ok(json) = CString::new(payload.to_string()) {
            unsafe {
                terminal_machines(json.as_ptr(), present);
            }
        }
    }

    fn refresh_roster(&mut self, cx: &mut Context<Self>) {
        if self.refreshing {
            return;
        }
        self.refreshing = true;
        self.roster_message = "Refreshing Tailscale machines…".into();
        self.picker(false);
        unsafe { terminal_refresh_tailscale() };
        cx.notify();
    }

    fn connect(&mut self, index: usize, cx: &mut Context<Self>) {
        #[cfg(feature = "native-catalog")]
        if self.preview { native_catalog::connect(self, index, cx); return; }
        if self.busy {
            return;
        }
        if !self.tailnet_ready || self.tailscale_proxy.is_none() {
            self.message = "Connect to Tailscale before opening a machine.".into();
            cx.notify();
            return;
        }
        if let Some(session) = self
            .sessions
            .iter()
            .position(|s| s.host == index && s.connected)
        {
            self.active = Some(session);
            self.menu = false;
            self.message.clear();
            show_keyboard();
            cx.notify();
            return;
        }
        self.busy = true;
        self.menu = false;
        self.message = "Checking host identity…".into();
        cx.notify();
        let host = self.hosts[index].clone();
        let proxy = self.tailscale_proxy.clone().unwrap();
        cx.spawn(async move |this, cx| {
            let result = ssh::probe(host, proxy).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(fingerprint) => {
                        if this.hosts[index].fingerprint.as_deref() == Some(&fingerprint) {
                            this.open_session(index, cx);
                        } else {
                            this.message = "Waiting for host verification…".into();
                            let address = CString::new(this.hosts[index].address.clone()).unwrap();
                            let identity = CString::new(fingerprint.clone()).unwrap();
                            this.pending = Some((index, fingerprint));
                            unsafe {
                                terminal_show_trust(
                                    address.as_ptr(),
                                    identity.as_ptr(),
                                    this.hosts[index].fingerprint.is_some(),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        this.message = format!("{error:#}");
                        this.busy = false;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_session(&mut self, index: usize, cx: &mut Context<Self>) {
        self.message = "Connecting…".into();
        cx.notify();
        let key = match private_key() {
            Ok(key) => key,
            Err(error) => {
                self.message = error.to_string();
                self.busy = false;
                cx.notify();
                return;
            }
        };
        let host = self.hosts[index].clone();
        let Some(proxy) = self.tailscale_proxy.clone() else {
            self.message = "Connect to Tailscale before opening a machine.".into();
            self.busy = false;
            cx.notify();
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = ssh::open(host, key, proxy).await;
            let mut ended = None;
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(session) => {
                        let cancel = session.channels.cancel_tx.as_ref().unwrap().clone();
                        let settings = TerminalSettings::get_global(cx).clone();
                        let builder = TerminalBuilder::new_remote(
                            session.channels,
                            this.hosts[index].name.clone(),
                            settings.cursor_shape,
                            settings.alternate_scroll,
                            Some(10_000),
                            cx.entity_id().as_u64(),
                            cx.background_executor(),
                            util::paths::PathStyle::Posix,
                        );
                        match builder {
                            Ok(builder) => {
                                let terminal = cx.new(|cx| builder.subscribe(cx));
                                let subscriptions = vec![
                                    cx.observe(&terminal, |_, _, cx| cx.notify()),
                                    cx.subscribe(&terminal, |_, _, _: &terminal::Event, cx| {
                                        cx.notify()
                                    }),
                                ];
                                let slot = this.sessions.len();
                                ended = Some((session.ended, slot));
                                this.sessions.push(Session {
                                    host: index,
                                    terminal,
                                    connected: true,
                                    cancel,
                                    _subscriptions: subscriptions,
                                });
                                this.active = Some(slot);
                                this.message.clear();
                                show_keyboard();
                            }
                            Err(error) => this.message = error.to_string(),
                        }
                    }
                    Err(error) => this.message = format!("{error:#}"),
                }
                cx.notify();
            })
            .ok();
            if let Some((ended, slot)) = ended {
                if let Ok(message) = ended.recv().await {
                    this.update(cx, |this, cx| {
                        this.sessions[slot].connected = false;
                        if this.active == Some(slot) {
                            this.message = message;
                        }
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn terminal(&self) -> Option<Entity<Terminal>> {
        self.active.map(|i| self.sessions[i].terminal.clone())
    }

    fn send_key(&mut self, mut key: Keystroke, cx: &mut Context<Self>) {
        key.modifiers.control |= self.control;
        self.control = false;
        if let Some(terminal) = self.terminal() {
            terminal.update(cx, |terminal, cx| {
                if !terminal.try_keystroke(&key, true) {
                    if let Some(text) = key.key_char {
                        terminal.input(text.into_bytes());
                    }
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    fn start_key(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.repeat_generation += 1;
        let generation = self.repeat_generation;
        let keystroke = Keystroke {
            key: key.into(),
            modifiers: Modifiers::default(),
            key_char: None,
        };
        self.send_key(keystroke.clone(), cx);
        if !["left", "down", "up", "right"].contains(&key) {
            return;
        }
        cx.spawn(async move |this, cx| {
            smol::Timer::after(Duration::from_millis(350)).await;
            loop {
                let held = this
                    .update(cx, |this, cx| {
                        if this.repeat_generation != generation {
                            return false;
                        }
                        this.send_key(keystroke.clone(), cx);
                        true
                    })
                    .unwrap_or(false);
                if !held {
                    break;
                }
                smol::Timer::after(Duration::from_millis(65)).await;
            }
        })
        .detach();
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(terminal) = self.terminal() {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                terminal.update(cx, |terminal, cx| {
                    terminal.paste(&text);
                    cx.notify();
                });
            }
        }
    }

    fn button(&self, id: &'static str, label: &'static str, cx: &App) -> Stateful<Div> {
        let colors = cx.theme().colors();
        presentation::button(id, label, presentation::Palette {
            text: colors.text,
            muted: colors.text_muted,
            surface: colors.tab_bar_background,
            border: colors.border,
            green: colors.terminal_ansi_green,
            element_background: colors.element_background,
            element_selected: colors.element_selected,
        })
    }
}

impl Render for PhoneTerminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.focus(&self.focus, cx);
        let colors = cx.theme().colors();
        let bg = colors.terminal_background;
        let surface = colors.tab_bar_background;
        let border = colors.border;
        let text = colors.text;
        let muted = colors.text_muted;
        let green = colors.terminal_ansi_green;
        let palette = presentation::Palette {
            text, muted, surface, border, green,
            element_background: colors.element_background,
            element_selected: colors.element_selected,
        };
        #[cfg(target_os = "ios")]
        let (top, bottom, left, right) = ized_platform::safe_area_insets();
        #[cfg(target_os = "ios")]
        let keyboard = ized_platform::keyboard_height();
        #[cfg(target_os = "android")]
        let (top, bottom, left, right) = ized_platform::android::jni::platform()
            .and_then(|platform| platform.primary_window())
            .map(|window| { let i = window.safe_area_insets_logical(); (i.top, i.bottom, i.left, i.right) })
            .unwrap_or_default();
        // NativeActivity's content rectangle already accounts for the Android IME.
        #[cfg(target_os = "android")]
        let keyboard = 0.;
        let menu_progress = self.menu_motion.update(self.menu, window);
        let active = self.active.map(|i| &self.sessions[i]);
        let connected = active.map_or(false, |s| s.connected);
        let name = active
            .map(|s| self.hosts[s.host].name.clone())
            .unwrap_or_else(|| "iZed Terminal".into());
        let entity = cx.entity();
        let mut content = div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .relative()
            // Padding covers the status bar and home indicator as well as
            // the keyboard inset; keep that outer chrome pure black.
            .bg(gpui::rgb(0x000000))
            .font_family("JetBrains Mono")
            .text_color(text)
            .text_size(px(13.))
            .pt(px(top))
            .pl(px(left))
            .pr(px(right))
            .pb(px(if keyboard > 0. { keyboard } else { bottom }))
            .track_focus(&self.focus)
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.repeat_generation += 1;
                    this.selecting = false;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.repeat_generation += 1;
                    this.selecting = false;
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.key == "v" {
                    this.paste(cx);
                } else if event.keystroke.modifiers.platform && event.keystroke.key == "c" {
                    if let Some(terminal) = this.terminal() {
                        terminal.update(cx, |t, cx| {
                            t.copy(Some(true));
                            cx.notify();
                        });
                    }
                } else {
                    this.send_key(event.keystroke.clone(), cx);
                }
                cx.stop_propagation();
            }))
            .child(presentation::session_header(
                name,
                connected,
                palette,
                cx.listener(|this, _, _, cx| {
                    this.menu = !this.menu;
                    this.tools_menu = false;
                    cx.notify();
                }),
                cx.listener(|this, _, _, _| this.picker(true)),
            ).when(menu_progress > 0., |header| header.border_color(surface)));

        if menu_progress > 0. {
            let active_sessions = self.sessions.iter().filter(|session| session.connected).count();
            let can_disconnect = self.active.is_some_and(|slot| self.sessions[slot].connected);
            let rows = active_sessions.max(1) + usize::from(can_disconnect);
            let menu_height = if active_sessions == 0 { 44. } else { active_sessions as f32 * 56. }
                + if can_disconnect { 44. } else { 0. }
                + (rows - 1) as f32 * 3. + 11.;
            let mut menu = div()
                .w_full()
                .h(px(menu_height))
                .flex_shrink_0()
                .bg(surface)
                .border_b_1()
                .border_color(border)
                .py(px(5.))
                .flex()
                .flex_col()
                .gap(px(3.));
            if active_sessions == 0 {
                menu = menu.child(
                    div()
                        .p(px(12.))
                        .text_color(muted)
                        .child("No active sessions. Tap + to connect."),
                );
            }
            for (slot, session) in self.sessions.iter().enumerate().filter(|(_, session)| session.connected) {
                let host = &self.hosts[session.host];
                let selected = self.active == Some(slot);
                menu = menu.child(
                    div()
                        .id(("session", slot))
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(56.))
                        .px(px(12.))
                        .when(selected, |row| row.bg(colors.element_selected))
                        .active(|s| s.bg(colors.element_selected))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(host.name.clone())
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(muted)
                                        .child(host.address.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_color(green)
                                .child(if selected { "✓" } else { "●" }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.sessions.get(slot).is_some_and(|session| session.connected) {
                                this.active = Some(slot);
                                this.menu = false;
                                this.message.clear();
                                show_keyboard();
                                cx.notify();
                            }
                        })),
                );
            }
            if let Some(slot) = self.active.filter(|_| can_disconnect) {
                menu = menu.child(
                    self.button("disconnect", "Disconnect", cx)
                        .justify_start()
                        .text_color(muted)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sessions[slot].cancel.try_send(()).ok();
                            this.menu = false;
                            cx.notify();
                        })),
                );
            }
            content = content.child(div().w_full().flex_shrink_0()
                .h(px(menu_height * menu_progress)).overflow_hidden().child(menu));
        }

        if let Some(terminal) = self.terminal() {
            let canvas_terminal = terminal.clone();
            content = content.child(
                div()
                    .id("terminal")
                    .bg(bg)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .relative()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.selecting = true;
                            if let Some(terminal) = this.terminal() {
                                terminal.update(cx, |t, cx| {
                                    t.mouse_down(event, cx);
                                    cx.notify();
                                });
                            }
                            show_keyboard();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            if let Some(terminal) = this.terminal() {
                                terminal.update(cx, |t, cx| {
                                    t.select_word_at_event_position(event);
                                    cx.notify();
                                });
                                this.tools_menu = true;
                                this.tools_notice = None;
                                this.menu = false;
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if this.selecting && event.pressed_button == Some(MouseButton::Left) {
                            let bounds = this.terminal_bounds;
                            if let Some(terminal) = this.terminal() {
                                terminal.update(cx, |t, cx| t.mouse_drag(event, bounds, cx));
                            }
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            if let Some(terminal) = this.terminal() {
                                terminal.update(cx, |t, cx| {
                                    t.mouse_up(event, cx);
                                    cx.notify();
                                });
                            }
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        if let Some(terminal) = this.terminal() {
                            terminal.update(cx, |t, cx| {
                                t.scroll_wheel(event, 1.);
                                cx.notify();
                            });
                        }
                    }))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let font_size = px(13.);
                                let font = font("JetBrains Mono");
                                let sample = window.text_system().shape_line(
                                    "M".into(),
                                    font_size,
                                    &[TextRun {
                                        len: 1,
                                        font,
                                        color: text,
                                        background_color: None,
                                        underline: None,
                                        strikethrough: None,
                                    }],
                                    None,
                                );
                                let cell_width = sample.width;
                                let inner = Bounds::new(
                                    bounds.origin + point(px(10.), px(8.)),
                                    size(
                                        (bounds.size.width - px(20.)).max(cell_width),
                                        (bounds.size.height - px(16.)).max(px(20.)),
                                    ),
                                );
                                entity.update(cx, |this, _| this.terminal_bounds = inner);
                                let (content, bounds, cell_width) =
                                    canvas_terminal.update(cx, |terminal, cx| {
                                        terminal.set_size(TerminalBounds::new(
                                            px(19.),
                                            cell_width,
                                            inner,
                                        ));
                                        terminal.sync(window, cx);
                                        (terminal.last_content.clone(), inner, cell_width)
                                    });
                                prepare_terminal(content, bounds, cell_width, window, cx)
                            },
                            move |_, prepared, window, cx| {
                                paint_terminal(prepared, window, cx);
                            },
                        )
                        .size_full(),
                    ),
            );
        } else {
            content = content.child(presentation::machines_empty_state(
                palette,
                cx.listener(|this, _, _, _| this.picker(true)),
            ));
        }

        if !self.message.is_empty() {
            content = content.child(div().flex_shrink_0().px(px(14.)).py(px(10.)).border_t_1().border_color(border)
                .bg(surface).text_size(px(11.)).text_color(muted).child(self.message.clone())
                .when(!self.busy, |el| el.child(div().flex().gap(px(8.)).mt(px(6.))
                    .child(self.button("retry", "Reconnect", cx).on_click(cx.listener(|this, _, _, cx| {
                        if let Some(i) = this.active { this.connect(this.sessions[i].host, cx); }
                        else if !this.hosts.is_empty() { this.connect(this.hosts.len()-1, cx); }
                    })))
                    .child(self.button("copy-access-key", "Copy SSH key", cx).on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.public_key.clone()));
                        this.message = "Public key copied. Add it to ~/.ssh/authorized_keys on your host.".into(); cx.notify();
                    }))))));
        }

        if self.active.is_some() {
            content = content.child(presentation::key_strip(
                palette,
                self.control,
                cx.listener(|this, key: &&'static str, _, cx| {
                    if *key == "more" {
                        this.tools_menu = !this.tools_menu;
                        this.tools_notice = None;
                        this.menu = false;
                        cx.notify();
                    } else if *key == "control" {
                        this.control = !this.control;
                        cx.notify();
                    } else {
                        this.start_key(*key, cx);
                    }
                }),
            ));
        }

        if self.tools_menu && self.active.is_some() {
            let has_selection = self.terminal().map_or(false, |terminal| {
                terminal.read(cx).last_content.selection_text.as_ref().is_some_and(|text| !text.is_empty())
            });
            let bottom_inset = if keyboard > 0. { keyboard } else { bottom };
            let tools = div()
                .absolute()
                .bottom(px(bottom_inset + 54.))
                .left_0()
                .right_0()
                .occlude()
                .bg(colors.elevated_surface_background)
                .border_t_1().border_color(border).shadow_md()
                .py(px(5.)).flex().flex_col().gap(px(3.))
                .child(div().px(px(12.)).py(px(8.)).text_size(px(10.)).text_color(muted).child("TERMINAL OPTIONS"))
                .child(self.button("terminal-paste", "Paste", cx).justify_start().on_click(cx.listener(|this, _, _, cx| {
                    this.paste(cx);
                    this.tools_menu = false;
                    cx.notify();
                })))
                .child(self.button("terminal-copy-selection", "Copy selection", cx).justify_start()
                    .when(!has_selection, |el| el.text_color(muted))
                    .when(has_selection, |el| el.on_click(cx.listener(|this, _, _, cx| {
                        if let Some(terminal) = this.terminal() {
                            if let Some(selection) = terminal.read(cx).last_content.selection_text.clone() {
                                cx.write_to_clipboard(ClipboardItem::new_string(selection));
                                this.tools_notice = Some("Selection copied.");
                            }
                        }
                        cx.notify();
                    }))))
                .when(!has_selection, |el| el.child(div().px(px(12.)).pb(px(6.)).text_size(px(10.)).text_color(muted)
                    .child("Select text in the terminal to copy it.")))
                .child(self.button("terminal-copy-output", "Copy full output", cx).justify_start().on_click(cx.listener(|this, _, _, cx| {
                    if let Some(terminal) = this.terminal() {
                        cx.write_to_clipboard(ClipboardItem::new_string(terminal.read(cx).get_content()));
                        this.tools_notice = Some("Full output copied.");
                    }
                    cx.notify();
                })))
                .child(self.button("terminal-keyboard", if ized_platform::keyboard_height() > 0. { "Hide keyboard" } else { "Show keyboard" }, cx)
                    .justify_start().border_t_1().border_color(border).on_click(cx.listener(|this, _, _, cx| {
                        this.tools_menu = false;
                        if ized_platform::keyboard_height() > 0. { hide_keyboard(); }
                        else { show_keyboard(); }
                        cx.notify();
                    })))
                .when_some(self.tools_notice, |el, notice| el.child(div().px(px(12.)).py(px(8.)).text_size(px(11.)).text_color(green).child(notice)));
            content = content
                .child(div().id("dismiss-terminal-options").absolute().inset_0().occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.tools_menu = false;
                        cx.stop_propagation();
                        cx.notify();
                    })))
                .child(tools);
        }
        content
    }
}

struct PreparedTerminal {
    bounds: Bounds<Pixels>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    backgrounds: Vec<(Bounds<Pixels>, Hsla)>,
    cursor: Option<(Bounds<Pixels>, Hsla)>,
}

// Shape during prepaint: GPUI's retained text-layout ranges must not change in paint.
fn prepare_terminal(
    content: terminal::TerminalContent,
    bounds: Bounds<Pixels>,
    cell_width: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> PreparedTerminal {
    use terminal::alacritty_terminal::{
        term::{cell::Flags, TermMode},
        vte::ansi::Color,
    };
    let theme = cx.theme().clone();
    let rows = content.terminal_bounds.num_lines();
    let columns = content.terminal_bounds.num_columns();
    let mut visible = vec![vec![' '; columns]; rows];
    for cell in &content.cells {
        let row = cell.point.line.0 + content.display_offset as i32;
        if row >= 0 && (row as usize) < rows && cell.point.column.0 < columns {
            visible[row as usize][cell.point.column.0] = if cell.c == '\0' { ' ' } else { cell.c };
        }
    }
    let screen = visible
        .into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    if let Ok(screen) = CString::new(screen) {
        unsafe {
            terminal_accessibility(
                screen.as_ptr(),
                f64::from(f32::from(bounds.origin.x)),
                f64::from(f32::from(bounds.origin.y)),
                f64::from(f32::from(bounds.size.width)),
                f64::from(f32::from(bounds.size.height)),
            );
        }
    }
    let color = |color: Color| match color {
        Color::Named(named) => terminal::get_color_at_index(named as usize, &theme),
        Color::Indexed(i) => terminal::get_color_at_index(i as usize, &theme),
        Color::Spec(rgb) => terminal::rgba_color(rgb.r, rgb.g, rgb.b),
    };
    let mut prepared = PreparedTerminal {
        bounds,
        lines: Vec::new(),
        backgrounds: Vec::new(),
        cursor: None,
    };
    for cell in &content.cells {
        let row = cell.point.line.0 + content.display_offset as i32;
        if row < 0 || cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let position = bounds.origin
            + point(
                cell_width * cell.point.column.0 as f32,
                px(19.) * row as f32,
            );
        if position.y >= bounds.bottom() {
            continue;
        }
        let mut foreground = color(cell.fg);
        let mut background = color(cell.bg);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut foreground, &mut background);
        }
        if content
            .selection
            .as_ref()
            .is_some_and(|selection| selection.contains(cell.point))
        {
            background = theme.colors().element_selected;
        }
        let width = cell_width
            * if cell.flags.contains(Flags::WIDE_CHAR) {
                2.
            } else {
                1.
            };
        if background != theme.colors().terminal_background {
            prepared
                .backgrounds
                .push((Bounds::new(position, size(width, px(19.))), background));
        }
        if cell.c == ' ' && cell.zerowidth().is_none() {
            continue;
        }
        let mut text = cell.c.to_string();
        if let Some(chars) = cell.zerowidth() {
            text.extend(chars);
        }
        let mut font = font("JetBrains Mono");
        if cell.flags.contains(Flags::BOLD) {
            font.weight = FontWeight::BOLD;
        }
        if cell.flags.contains(Flags::ITALIC) {
            font.style = FontStyle::Italic;
        }
        let run = TextRun {
            len: text.len(),
            font,
            color: foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(text.into(), px(13.), &[run], None);
        prepared.lines.push((position, line));
    }
    let cursor = content.cursor.point;
    let row = cursor.line.0 + content.display_offset as i32;
    if content.mode.contains(TermMode::SHOW_CURSOR)
        && row >= 0
        && (row as f32 * 19.) < f32::from(bounds.size.height)
    {
        let position =
            bounds.origin + point(cell_width * cursor.column.0 as f32, px(19.) * row as f32);
        prepared.cursor = Some((
            Bounds::new(position, size(cell_width, px(19.))),
            theme.colors().text.opacity(0.45),
        ));
    }
    prepared
}

fn paint_terminal(prepared: PreparedTerminal, window: &mut Window, cx: &mut App) {
    window.with_content_mask(
        Some(ContentMask {
            bounds: prepared.bounds,
        }),
        |window| {
            for (bounds, color) in prepared.backgrounds {
                window.paint_quad(fill(bounds, color));
            }
            for (position, line) in prepared.lines {
                line.paint(position, px(19.), TextAlign::Left, None, window, cx)
                    .ok();
            }
            if let Some((bounds, color)) = prepared.cursor {
                window.paint_quad(fill(bounds, color));
            }
        },
    );
}

pub fn open(cx: &mut App) {
    let data = hosts_path().parent().unwrap().join("Zed");
    paths::set_custom_data_dir(data.to_str().unwrap());
    std::fs::create_dir_all(paths::config_dir()).ok();
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(include_bytes!("../../app/fonts/JetBrainsMono-Regular.ttf")),
            Cow::Borrowed(include_bytes!("../../app/fonts/JetBrainsMono-Medium.ttf")),
        ])
        .expect("load terminal fonts");
    settings::init(cx);
    SettingsStore::update_global(cx, |store, cx| {
        store.set_global_settings(r#"{"buffer_font_family":"JetBrains Mono","buffer_font_size":13,"ui_font_family":"JetBrains Mono","ui_font_size":14,"theme":{"mode":"dark","dark":"Vercel Dark","light":"Vercel Light"}}"#, cx).unwrap();
    });
    theme_settings::init(theme::LoadThemes::JustBase, cx);
    theme_settings::load_user_theme(
        &theme::ThemeRegistry::global(cx),
        include_bytes!("../../app/themes/vercel-theme.json"),
    )
    .unwrap();
    theme_settings::reload_theme(cx);
    release_channel::init(semver::Version::new(0, 1, 0), cx);
    #[cfg(feature = "native-catalog")]
    if std::env::var("IZED_NATIVE_CATALOG").as_deref() == Ok("1") {
        native_catalog::open(cx);
        return;
    }
    cx.open_window(WindowOptions::default(), |_, cx| {
        cx.new(|cx| {
            let mut terminal = PhoneTerminal::new(cx);
            if cfg!(debug_assertions) && std::env::var("IZED_SMOKE_TEST").as_deref() == Ok("1") {
                if let Ok(address) = std::env::var("IZED_TEST_AUTO_HOST") {
                    terminal.hosts = vec![Host {
                        name: "SSH smoke test".into(),
                        address,
                        command: String::new(),
                        fingerprint: None,
                    }];
                    terminal.connect(0, cx);
                }
            } else {
                terminal.picker(true);
            }
            terminal
        })
    })
    .expect("open terminal");
    #[cfg(target_os = "ios")]
    ized_platform::ios::set_status_bar_style(ized_platform::StatusBarContentStyle::Light);
    cx.activate(true);
}

fn show_keyboard() {
    #[cfg(target_os = "android")]
    crate::android_terminal::keyboard(true);
    #[cfg(not(target_os = "android"))]
    ized_platform::show_keyboard();
}
fn hide_keyboard() {
    #[cfg(target_os = "android")]
    crate::android_terminal::keyboard(false);
    #[cfg(not(target_os = "android"))]
    ized_platform::hide_keyboard();
}
