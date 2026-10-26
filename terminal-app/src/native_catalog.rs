//! Opt-in native fixture host. Production renderers; local data and terminal I/O.
use super::*;
use std::collections::HashMap;
use std::sync::Mutex;
static SNAPSHOT: Mutex<String> = Mutex::new(String::new());

#[no_mangle]
extern "C" fn terminal_catalog_snapshot() -> *mut c_char {
    CString::new(SNAPSHOT.lock().unwrap().clone())
        .unwrap()
        .into_raw()
}
#[no_mangle]
unsafe extern "C" fn terminal_catalog_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

static SELECTION: LazyLock<(
    smol::channel::Sender<String>,
    smol::channel::Receiver<String>,
)> = LazyLock::new(smol::channel::unbounded);

#[no_mangle]
unsafe extern "C" fn terminal_catalog_select(id: *const c_char) {
    if !id.is_null() {
        let _ = SELECTION
            .0
            .try_send(CStr::from_ptr(id).to_string_lossy().into_owned());
    }
}

fn fixture(id: &str, cx: &mut Context<PhoneTerminal>) -> PhoneTerminal {
    let has_terminal = matches!(
        id,
        "terminal" | "session-menu" | "disconnected" | "access-error" | "connecting"
    );
    let hosts = if id == "empty-menu" || id == "landing" {
        vec![]
    } else {
        vec![Host {
            name: "Studio Mac".into(),
            address: "demo@100.64.0.10".into(),
            command: String::new(),
            fingerprint: None,
        }]
    };
    let mut state = PhoneTerminal {
        preview: true, hosts, sessions: vec![], active: None, focus: cx.focus_handle(),
        menu: matches!(id, "session-menu" | "empty-menu"), control: false,
        busy: id == "connecting",
        message: match id {
            "connecting" => "Connecting…",
            "access-error" => "Permission denied (publickey). Authorize this phone’s key on the host.",
            "disconnected" => "Connection closed.", _ => "",
        }.into(),
        public_key: "DEMO PUBLIC KEY — NOT A CREDENTIAL".into(), pending: None,
        terminal_bounds: Bounds::default(), selecting: false, repeat_generation: 0,
        roster: serde_json::from_value(serde_json::json!({"machines":[
            {"name":"Studio Mac","hostname":"studio.demo.ts.net","address":"100.64.0.10","os":"macOS","online":true},
            {"name":"Build server","hostname":"build.demo.ts.net","address":"100.64.0.12","os":"Linux","online":true}
        ], "updated_at": 1792840000_u64})).unwrap(),
        refreshing: false, roster_message: String::new(), tailnet_ready: true, tailscale_proxy: None,
    };
    if has_terminal {
        connect(&mut state, 0, cx);
    }
    state.busy = id == "connecting";
    state.menu = matches!(id, "session-menu" | "empty-menu");
    state.message = match id {
        "connecting" => "Connecting…",
        "access-error" => "Permission denied (publickey). Authorize this phone’s key on the host.",
        "disconnected" => "Connection closed.",
        _ => "",
    }
    .into();
    if let Some(session) = state.sessions.first_mut() {
        session.connected = matches!(id, "terminal" | "session-menu");
    }
    state
}

pub(super) fn connect(state: &mut PhoneTerminal, index: usize, cx: &mut Context<PhoneTerminal>) {
    if index >= state.hosts.len() {
        return;
    }
    if let Some(slot) = state
        .sessions
        .iter()
        .position(|s| s.host == index && s.connected)
    {
        state.active = Some(slot);
        state.sessions[slot].connected = true;
        state.menu = false;
        state.message.clear();
        state.busy = false;
        cx.notify();
        return;
    }
    state.sessions.retain(|s| s.host != index);
    state.active = None;
    let (ended_tx, ended_rx) = smol::channel::bounded(1);
    let (input_tx, input_rx) = smol::channel::unbounded::<Vec<u8>>();
    let (output_tx, output_rx) = smol::channel::unbounded();
    let (resize_tx, resize_rx) = smol::channel::unbounded();
    let (cancel, cancel_rx) = smol::channel::unbounded();
    let settings = TerminalSettings::get_global(cx).clone();
    let builder = TerminalBuilder::new_remote(
        terminal::RemoteTerminalChannels {
            input_tx,
            resize_tx,
            output_rx,
            cancel_tx: Some(cancel.clone()),
        },
        state.hosts[index].name.clone(),
        settings.cursor_shape,
        settings.alternate_scroll,
        Some(1000),
        cx.entity_id().as_u64(),
        cx.background_executor(),
        util::paths::PathStyle::Posix,
    )
    .unwrap();
    let terminal = cx.new(|cx| builder.subscribe(cx));
    // Seed after the first layout has sized the actual terminal emulator.
    let seed_terminal = terminal.clone();
    cx.spawn(async move |_, cx| {
        smol::Timer::after(Duration::from_millis(400)).await;
        seed_terminal.update(cx, |t,cx| t.write_pty_output(
            b"\x1b[2J\x1b[H\x1b[32miZed native terminal preview\x1b[0m\r\nProduction terminal emulator and renderer.\r\nLocal echo fixture; no SSH or shell runs.\r\n\r\ndemo@studio ~ % ",cx));
    }).detach();
    let terminal_id = terminal.entity_id();
    cx.spawn(async move |this, cx| {
        if ended_rx.recv().await.is_ok() {
            this.update(cx, |state, cx| {
                if let Some(slot) = state
                    .sessions
                    .iter()
                    .position(|s| s.terminal.entity_id() == terminal_id)
                {
                    state.sessions[slot].connected = false;
                    if state.active == Some(slot) {
                        state.message = "Connection closed.".into();
                        state.busy = false;
                    }
                    cx.notify();
                }
            })
            .ok();
        }
    })
    .detach();
    cx.background_executor()
        .spawn(async move {
            // All I/O terminates in these in-memory fixture channels.
            let io = async {
                while let Ok(bytes) = input_rx.recv().await {
                    let echoed = if bytes == b"\r" {
                        b"\r\ndemo@studio ~ % ".to_vec()
                    } else {
                        bytes
                    };
                    if output_tx.send(echoed).await.is_err() {
                        break;
                    }
                }
            };
            let drain = async { while resize_rx.recv().await.is_ok() {} };
            run_fixture_io(io, drain, cancel_rx).await;
            let _ = ended_tx.send(()).await;
        })
        .detach();
    let subscriptions = vec![
        cx.observe(&terminal, |_, _, cx| cx.notify()),
        cx.subscribe(&terminal, |_, _, _: &terminal::Event, cx| cx.notify()),
    ];
    state.sessions.push(Session {
        host: index,
        terminal,
        connected: true,
        cancel,
        _subscriptions: subscriptions,
    });
    state.active = Some(state.sessions.len() - 1);
    state.menu = false;
    state.message.clear();
    state.busy = false;
    cx.notify();
}

async fn run_fixture_io(
    io: impl std::future::Future<Output = ()>,
    drain: impl std::future::Future<Output = ()>,
    cancel: smol::channel::Receiver<()>,
) {
    smol::future::race(smol::future::race(io, drain), async {
        let _ = cancel.recv().await;
    })
    .await;
}

struct Catalog {
    selected: String,
    screens: HashMap<String, Entity<PhoneTerminal>>,
    control: bool,
    _subscriptions: Vec<Subscription>,
}
impl Catalog {
    fn new(cx: &mut Context<Self>) -> Self {
        let screens: HashMap<String, Entity<PhoneTerminal>> = [
            "landing",
            "terminal",
            "session-menu",
            "connecting",
            "access-error",
            "disconnected",
            "empty-menu",
        ]
        .into_iter()
        .map(|id| (id.to_owned(), cx.new(|cx| fixture(id, cx))))
        .collect();
        let subscriptions = screens
            .values()
            .map(|screen| cx.observe(screen, |_, _, cx| cx.notify()))
            .collect();
        let receiver = SELECTION.1.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(id) = receiver.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if this.selected != id {
                            this.control = id == "key-strip-control";
                            this.selected = id;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let events = NATIVE_EVENTS.1.clone();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update(cx, |this, cx| {
                        if let Some(screen) = this.screens.get(&this.selected) {
                            screen.update(cx, |state, cx| match event {
                                NativeEvent::Add(host) => {
                                    state.hosts.push(host);
                                    let i = state.hosts.len() - 1;
                                    connect(state, i, cx);
                                }
                                NativeEvent::Trust(_) => {}
                                NativeEvent::Refresh => unsafe { terminal_refresh_tailscale() },
                                _ => {}
                            });
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            selected: "landing".into(),
            screens,
            control: false,
            _subscriptions: subscriptions,
        }
    }
}
impl Render for Catalog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = if let Some(screen) = self.screens.get(&self.selected) {
            let state = screen.read(cx);
            serde_json::json!({"id":self.selected,"menu":state.menu,"control":state.control,
                "busy":state.busy,"message":state.message,"sessions":state.sessions.len(),
                "terminal":state.terminal().map(|t|t.read(cx).get_content()).unwrap_or_default()})
        } else {
            serde_json::json!({"id":self.selected,"control":self.control})
        };
        *SNAPSHOT.lock().unwrap() = snapshot.to_string();
        if let Some(screen) = self.screens.get(&self.selected) {
            return div().size_full().child(screen.clone());
        }
        let c = cx.theme().colors();
        let p = presentation::Palette {
            text: c.text,
            muted: c.text_muted,
            surface: c.tab_bar_background,
            border: c.border,
            green: c.terminal_ansi_green,
            element_background: c.element_background,
            element_selected: c.element_selected,
        };
        let body = div()
            .size_full()
            .bg(gpui::rgb(0))
            .text_color(p.text)
            .font_family("JetBrains Mono");
        match self.selected.as_str() {
            "session-header" => body.child(presentation::session_header(
                "Studio Mac",
                true,
                p,
                |_, _, _| {},
                |_, _, _| {},
            )),
            "landing-body" => body.child(presentation::machines_empty_state(p, |_, _, _| {})),
            "key-strip" | "key-strip-control" => body.child(presentation::key_strip(
                p,
                self.control,
                cx.listener(|this, key, _, cx| {
                    if *key == "control" {
                        this.control = !this.control;
                    }
                    cx.notify();
                }),
            )),
            "button" => body.p(px(20.)).child(presentation::button(
                "catalog-button",
                "Browse machines  →",
                p,
            )),
            _ => body.child("Unknown catalog entry"),
        }
    }
}
pub(super) fn open(cx: &mut App) {
    cx.open_window(WindowOptions::default(), |_, cx| cx.new(Catalog::new))
        .expect("catalog GPUI window");
    cx.activate(true);
}
