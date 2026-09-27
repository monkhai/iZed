//! An iPad host for Zed's own workspace, editor tabs, and project panel.

use std::{borrow::Cow, cell::RefCell, collections::HashMap, path::{Path, PathBuf}, rc::Rc, sync::{Arc, Mutex, OnceLock}, time::{Duration, Instant}};

use futures::channel::oneshot;
use gpui::{
    actions, Animation, AnimationExt, App, AppContext, AsyncApp, ClipboardItem, Context, DismissEvent, Empty, Entity, EventEmitter, FocusHandle,
    Focusable, KeyBinding, Render, SharedString, Subscription, Task, UpdateGlobal, Window, WindowHandle,
    WeakEntity, WindowOptions,
};
use project::trusted_worktrees::{self, PathTrust, TrustedWorktrees};
use remote::{ConnectionState, RemoteClient, RemoteClientDelegate, RemoteConnectionOptions, RemotePlatform, SshConnectionOptions};
use serde::{Deserialize, Serialize};
use theme::{ActiveTheme, GlobalTheme};
use ui::prelude::*;
use ui::CommonAnimationExt;
use util::path_list::PathList;
use workspace::{AppState, MultiWorkspace, Workspace};

struct SshConnectionStatus {
    client: Entity<RemoteClient>,
    _model_memory: Option<Entity<ProjectModelMemory>>,
    last_state: ConnectionState,
    show_transient: bool,
    transient_generation: u64,
    _subscription: Subscription,
}

impl SshConnectionStatus {
    fn new(client: Entity<RemoteClient>, cx: &mut Context<Self>) -> Self {
        let last_state = client.read(cx).connection_state();
        let subscription = cx.observe(&client, |this, client, cx| {
            let state = client.read(cx).connection_state();
            if state == this.last_state {
                return;
            }
            log::info!("iZed SSH connection state: {:?} -> {:?}", this.last_state, state);
            let previous = std::mem::replace(&mut this.last_state, state);
            match state {
                ConnectionState::Connected => {
                    this.transient_generation += 1;
                    this.show_transient = false;
                }
                ConnectionState::Disconnected => {
                    this.transient_generation += 1;
                    this.show_transient = true;
                }
                ConnectionState::Connecting | ConnectionState::HeartbeatMissed | ConnectionState::Reconnecting => {
                    if matches!(previous, ConnectionState::Connected | ConnectionState::Disconnected) {
                        this.transient_generation += 1;
                        let generation = this.transient_generation;
                        this.show_transient = false;
                        cx.spawn(async move |this, cx| {
                            smol::Timer::after(Duration::from_millis(1200)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.transient_generation == generation
                                    && !matches!(this.last_state, ConnectionState::Connected | ConnectionState::Disconnected)
                                {
                                    this.show_transient = true;
                                    cx.notify();
                                }
                            });
                        }).detach();
                    }
                }
            }
            cx.notify();
        });
        Self { client, _model_memory: None, last_state, show_transient: last_state != ConnectionState::Connected, transient_generation: 0, _subscription: subscription }
    }
}

impl Render for SshConnectionStatus {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.show_transient {
            return Empty.into_any_element();
        }
        let message = match self.client.read(cx).connection_state() {
            ConnectionState::Connected => return Empty.into_any_element(),
            ConnectionState::Connecting | ConnectionState::Reconnecting => "Reconnecting to Machine…",
            ConnectionState::HeartbeatMissed => "Connection interrupted…",
            ConnectionState::Disconnected => "Machine disconnected",
        };
        Label::new(message).into_any_element()
    }
}

impl workspace::StatusItemView for SshConnectionStatus {
    fn set_active_pane_item(
        &mut self,
        _item: Option<&dyn workspace::ItemHandle>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {}
}

fn add_git_diff_toolbar(
    workspace: &Workspace,
    pane: &Entity<workspace::Pane>,
    window: &mut Window,
    cx: &mut App,
) {
    let languages = workspace.project().read(cx).languages().clone();
    pane.update(cx, |pane, cx| {
        pane.toolbar().update(cx, |toolbar, cx| {
            let search_bar = cx.new(|cx| search::BufferSearchBar::new(Some(languages), window, cx));
            toolbar.add_item(search_bar, window, cx);
            let project_diff_toolbar = cx.new(|cx| git_ui::project_diff::ProjectDiffToolbar::new(workspace, cx));
            toolbar.add_item(project_diff_toolbar, window, cx);
            let branch_diff_toolbar = cx.new(git_ui::project_diff::BranchDiffToolbar::new);
            toolbar.add_item(branch_diff_toolbar, window, cx);
            let commit_view_toolbar = cx.new(|_| git_ui::commit_view::CommitViewToolbar::new());
            toolbar.add_item(commit_view_toolbar, window, cx);
        });
    });
}

fn ipad_picker_button(
    id: &'static str,
    label: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> ui::ButtonLike {
    Button::new(id, label)
        .size(ButtonSize::Large)
        .render(window, cx)
        .height(px(44.).into())
}

fn bundled_server_archives() -> anyhow::Result<PathBuf> {
    let bundle = std::env::current_exe()?;
    Ok(bundle.parent()
        .ok_or_else(|| anyhow::anyhow!("Could not locate the iZed app bundle"))?
        .join("RemoteServers"))
}

fn server_progress_indicator(progress: (remote::SshServerPhase, u64, u64), cx: &App) -> impl IntoElement {
    let (phase, sent, total) = progress;
    let label = match phase {
        remote::SshServerPhase::Checking => "Checking SSH access and Zed server…".to_owned(),
        remote::SshServerPhase::Uploading => format!("Transferring Zed server to this Machine… {}%", sent.saturating_mul(100) / total.max(1)),
        remote::SshServerPhase::Installing => "Verifying and installing Zed server…".to_owned(),
        remote::SshServerPhase::Ready => "Zed server is ready".to_owned(),
    };
    v_flex().gap_2().w_full().child(Label::new(label))
        .when(phase == remote::SshServerPhase::Uploading, |this| this
            .child(div().w_full().h(px(6.)).rounded_sm()
                .bg(cx.theme().colors().border)
                .child(div().h_full()
                    .w(relative((sent as f32 / total.max(1) as f32).clamp(0., 1.)))
                    .rounded_sm().bg(cx.theme().colors().border_focused))))
}

actions!(ized_ssh_picker, [NextSuggestion, PreviousSuggestion, AcceptSuggestion, SelectDirectory, PickerNext, PickerPrevious, PickerAccept, PickerBack, PickerSwitchColumn, PickerEditMachine, PickerRemoveMachine]);

#[derive(Clone, Serialize, Deserialize)]
struct SshMachine {
    name: String,
    address: String,
}

#[derive(Default, Serialize, Deserialize)]
struct MachineStore {
    machines: Vec<SshMachine>,
    selected_address: Option<String>,
    #[serde(default)]
    browse_paths: HashMap<String, String>,
}

impl MachineStore {
    fn path() -> PathBuf {
        SshTarget::config_path().with_file_name("ssh-machines.json")
    }

    fn load() -> Self {
        if let Ok(bytes) = std::fs::read(Self::path()) {
            if let Ok(store) = serde_json::from_slice(&bytes) {
                return store;
            }
        }
        // Preserve the machine that was paired before named profiles existed.
        if SshTarget::config_path().exists() {
            let target = SshTarget::load();
            Self {
                selected_address: Some(target.address.clone()),
                machines: vec![SshMachine {
                    name: "Imported Machine".into(),
                    address: target.address,
                }],
                browse_paths: HashMap::new(),
            }
        } else {
            Self::default()
        }
    }

    fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SshTarget {
    address: String,
    folder: String,
}

impl Default for SshTarget {
    fn default() -> Self {
        Self {
            address: String::new(),
            folder: "/".into(),
        }
    }
}

impl SshTarget {
    fn config_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        home.join("Documents").join("ssh-target.json")
    }

    fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("Could not read SSH target at {}: {error}", path.display());
                Self::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                log::warn!("Could not read SSH target at {}: {error}", path.display());
                Self::default()
            }
        }
    }

    fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }

    fn same_project(&self, other: &Self) -> bool {
        self.address == other.address
            && Path::new(&self.folder)
                .components()
                .eq(Path::new(&other.folder).components())
    }

    fn connection_options(&self) -> anyhow::Result<SshConnectionOptions> {
        let options = SshConnectionOptions::parse_command_line(&self.address)?;
        anyhow::ensure!(!options.host.to_string().is_empty(), "Enter an SSH host");
        anyhow::ensure!(options.username.is_some(), "Enter a username, as user@host");
        anyhow::ensure!(
            options.args.as_ref().is_none_or(Vec::is_empty) && options.port_forwards.is_none(),
            "This iPad SSH transport supports user@host and an optional port only"
        );
        anyhow::ensure!(
            PathBuf::from(&self.folder).is_absolute(),
            "Enter an absolute project folder path"
        );
        Ok(options)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectModelSelection {
    provider: String,
    model: String,
}

#[derive(Serialize, Deserialize)]
struct ProjectModelPreference {
    project: SshTarget,
    selection: ProjectModelSelection,
}

#[derive(Default, Serialize, Deserialize)]
struct ProjectModelStore {
    projects: Vec<ProjectModelPreference>,
}

impl ProjectModelStore {
    fn path() -> PathBuf {
        SshTarget::config_path().with_file_name("project-models.json")
    }

    fn load() -> Self {
        match std::fs::read(Self::path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                log::warn!("Could not read project model preferences: {error}");
                Self::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                log::warn!("Could not read project model preferences: {error}");
                Self::default()
            }
        }
    }

    fn selection(&self, project: &SshTarget) -> Option<ProjectModelSelection> {
        self.projects.iter().find(|entry| entry.project.same_project(project))
            .map(|entry| entry.selection.clone())
    }

    fn remember(&mut self, project: SshTarget, selection: ProjectModelSelection) -> anyhow::Result<()> {
        if let Some(entry) = self.projects.iter_mut().find(|entry| entry.project.same_project(&project)) {
            entry.selection = selection;
        } else {
            self.projects.push(ProjectModelPreference { project, selection });
        }
        let path = Self::path();
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)?;
        Ok(())
    }
}

struct ProjectModelMemory {
    project: SshTarget,
    panel: Entity<agent_ui::AgentPanel>,
    selection: Option<ProjectModelSelection>,
    tracked_thread: Option<Entity<agent::Thread>>,
    _panel_observation: Subscription,
    _panel_events: Subscription,
    _thread_observation: Option<Subscription>,
}

impl ProjectModelMemory {
    fn new(project: SshTarget, panel: Entity<agent_ui::AgentPanel>, cx: &mut Context<Self>) -> Self {
        let selection = ProjectModelStore::load().selection(&project);
        let panel_observation = cx.observe(&panel, |this, _, cx| this.refresh(cx));
        let panel_events = cx.subscribe(&panel, |this, _, event: &agent_ui::AgentPanelEvent, cx| {
            if matches!(event, agent_ui::AgentPanelEvent::ActiveViewChanged) {
                this.refresh(cx);
            }
        });
        Self {
            project, panel, selection, tracked_thread: None,
            _panel_observation: panel_observation,
            _panel_events: panel_events,
            _thread_observation: None,
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.panel.read(cx).active_conversation_view().cloned() else {
            self.tracked_thread = None;
            self._thread_observation = None;
            return;
        };
        let Some(thread) = view.read(cx).as_native_thread(cx) else { return; };
        if self.tracked_thread.as_ref().is_some_and(|current| current == &thread) {
            return;
        }
        self._thread_observation = None;
        self.tracked_thread = Some(thread.clone());

        if self.panel.read(cx).active_thread_is_draft(cx) {
            if let Some(selection) = &self.selection {
                let selected = language_model::SelectedModel {
                    provider: language_model::LanguageModelProviderId(selection.provider.clone().into()),
                    model: language_model::LanguageModelId(selection.model.clone().into()),
                };
                if let Some(configured) = language_model::LanguageModelRegistry::global(cx)
                    .update(cx, |registry, cx| registry.select_model(&selected, cx))
                {
                    thread.update(cx, |thread, cx| thread.set_model(configured.model, cx));
                }
            }
        }

        self.remember_thread_model(&thread, cx);
        self._thread_observation = Some(cx.observe(&thread, |this, thread, cx| {
            this.remember_thread_model(&thread, cx);
        }));
    }

    fn remember_thread_model(&mut self, thread: &Entity<agent::Thread>, cx: &mut Context<Self>) {
        let Some(model) = thread.read(cx).model() else { return; };
        let selection = ProjectModelSelection {
            provider: model.provider_id().0.to_string(),
            model: model.id().0.to_string(),
        };
        if self.selection.as_ref() == Some(&selection) { return; }
        let mut store = ProjectModelStore::load();
        if let Err(error) = store.remember(self.project.clone(), selection.clone()) {
            log::warn!("Could not save project model preference: {error:#}");
            return;
        }
        self.selection = Some(selection);
    }
}

const DIRECTORY_CACHE_TTL: Duration = Duration::from_secs(15);
const DIRECTORY_CACHE_CAPACITY: usize = 64;
type DirectoryCacheKey = (String, String);

struct DirectoryCacheEntry {
    paths: Vec<String>,
    fetched_at: Instant,
    last_used: Instant,
}

#[derive(Default)]
struct DirectoryCache {
    entries: HashMap<DirectoryCacheKey, DirectoryCacheEntry>,
    prefetching: Option<DirectoryCacheKey>,
}

static DIRECTORY_CACHE: OnceLock<Mutex<DirectoryCache>> = OnceLock::new();

fn directory_cache() -> &'static Mutex<DirectoryCache> {
    DIRECTORY_CACHE.get_or_init(|| Mutex::new(DirectoryCache::default()))
}

fn cached_directories(key: &DirectoryCacheKey) -> Option<(Vec<String>, bool)> {
    let mut cache = directory_cache().lock().unwrap_or_else(|poison| poison.into_inner());
    let entry = cache.entries.get_mut(key)?;
    entry.last_used = Instant::now();
    Some((entry.paths.clone(), entry.fetched_at.elapsed() < DIRECTORY_CACHE_TTL))
}

fn cache_directories(key: DirectoryCacheKey, paths: Vec<String>) {
    let mut cache = directory_cache().lock().unwrap_or_else(|poison| poison.into_inner());
    if cache.entries.len() >= DIRECTORY_CACHE_CAPACITY && !cache.entries.contains_key(&key) {
        if let Some(oldest) = cache.entries.iter().min_by_key(|(_, entry)| entry.last_used).map(|(key, _)| key.clone()) {
            cache.entries.remove(&oldest);
        }
    }
    let now = Instant::now();
    cache.entries.insert(key, DirectoryCacheEntry { paths, fetched_at: now, last_used: now });
}

enum DirectoryBrowserEvent {
    Open(SshTarget),
    Navigated(SshTarget),
    Back,
}

struct SshTargetModal {
    address: Entity<editor::Editor>,
    folder: Entity<editor::Editor>,
    fixed_address: bool,
    embedded: bool,
    suggestions: Vec<String>,
    suggestions_key: Option<DirectoryCacheKey>,
    suggestion_error: Option<SharedString>,
    loading_suggestions: bool,
    suggestions_loaded: bool,
    selected_suggestion: Option<usize>,
    opening_folder: Option<String>,
    suggestion_scroll: gpui::ScrollHandle,
    folder_focused: bool,
    remote_home: Option<(String, String)>,
    resolving_home_for: Option<String>,
    query_generation: u64,
    prefetch_generation: u64,
    pending_tab_completion: Option<u64>,
    window: WindowHandle<MultiWorkspace>,
    app_state: Arc<AppState>,
    error: Option<SharedString>,
}

impl SshTargetModal {
    fn new(
        window_handle: WindowHandle<MultiWorkspace>,
        app_state: Arc<AppState>,
        initial: Option<SshTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fixed_address = initial.is_some();
        let target = initial.unwrap_or_else(SshTarget::load);
        let address = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(window, cx);
            editor.set_placeholder_text("user@host or ssh user@host -p 2222", window, cx);
            editor.set_text(target.address, window, cx);
            editor
        });
        let folder = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(window, cx);
            editor.set_placeholder_text("/absolute/path/to/project", window, cx);
            editor.set_text(target.folder, window, cx);
            editor
        });
        cx.subscribe(&address, |this, _, event: &editor::EditorEvent, cx| {
            if matches!(event, editor::EditorEvent::BufferEdited) {
                this.refresh_suggestions(cx);
            }
        }).detach();
        cx.subscribe(&folder, |this, _, event: &editor::EditorEvent, cx| {
            match event {
                editor::EditorEvent::Focused => {
                    this.folder_focused = true;
                    this.refresh_suggestions(cx);
                }
                editor::EditorEvent::Blurred => {
                    this.folder_focused = false;
                    this.refresh_suggestions(cx);
                }
                editor::EditorEvent::BufferEdited if this.folder_focused => {
                    this.refresh_suggestions(cx);
                }
                _ => {}
            }
        }).detach();
        let mut modal = Self {
            address,
            folder,
            fixed_address,
            embedded: false,
            suggestions: Vec::new(),
            suggestions_key: None,
            suggestion_error: None,
            loading_suggestions: false,
            suggestions_loaded: false,
            selected_suggestion: None,
            opening_folder: None,
            suggestion_scroll: gpui::ScrollHandle::new(),
            folder_focused: false,
            remote_home: None,
            resolving_home_for: None,
            query_generation: 0,
            prefetch_generation: 0,
            pending_tab_completion: None,
            window: window_handle,
            app_state,
            error: None,
        };
        modal.refresh_suggestions(cx);
        modal
    }

    fn resolved_path(&self, path: &str, cx: &App) -> Option<String> {
        let path = path.trim();
        if path == "~" || path.starts_with("~/") {
            let address = self.address.read(cx).text(cx);
            let (_, home) = self.remote_home.as_ref().filter(|(saved_address, _)| saved_address == address.trim())?;
            Some(format!("{}{}", home.trim_end_matches('/'), &path[1..]))
        } else {
            Some(path.to_owned())
        }
    }

    fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        self.query_generation += 1;
        self.prefetch_generation += 1;
        self.pending_tab_completion = None;
        let generation = self.query_generation;
        let address = self.address.read(cx).text(cx);
        let typed_path = self.folder.read(cx).text(cx);
        self.suggestion_error = None;
        if !self.folder_focused {
            self.loading_suggestions = false;
            cx.notify();
            return;
        }
        let Some(resolved_path) = self.resolved_path(&typed_path, cx) else {
            if self.resolving_home_for.as_deref() != Some(address.trim()) {
                let options = (SshTarget { address: address.clone(), folder: "/".into() }).connection_options();
                match options {
                    Ok(options) => {
                        self.resolving_home_for = Some(address.trim().to_owned());
                        let expected_address = address.trim().to_owned();
                        cx.spawn(async move |this, cx| {
                            let result = remote::ssh_home_directory(options).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.address.read(cx).text(cx).trim() != expected_address { return; }
                                this.resolving_home_for = None;
                                match result {
                                    Ok(home) => {
                                        this.remote_home = Some((expected_address, home));
                                        this.refresh_suggestions(cx);
                                    }
                                    Err(error) => {
                                        this.loading_suggestions = false;
                                        this.suggestion_error = Some(error.to_string().into());
                                        cx.notify();
                                    }
                                }
                            });
                        }).detach();
                    }
                    Err(error) => self.suggestion_error = Some(error.to_string().into()),
                }
            }
            self.loading_suggestions = self.suggestion_error.is_none();
            cx.notify();
            return;
        };
        let path = Path::new(resolved_path.trim());
        let directory = if typed_path == "~" || typed_path.ends_with('/') { path } else { path.parent().unwrap_or(path) };
        let directory = directory.components().collect::<PathBuf>().to_string_lossy().into_owned();
        if !directory.starts_with('/') {
            self.loading_suggestions = false;
            cx.notify();
            return;
        }
        let cache_key = (address.trim().to_owned(), directory.clone());
        if self.suggestions_key.as_ref() == Some(&cache_key) && self.suggestions_loaded {
            self.selected_suggestion = (!self.filtered_suggestions(cx).is_empty()).then_some(0);
            if self.selected_suggestion.is_some() { self.suggestion_scroll.scroll_to_item(0); }
            self.loading_suggestions = false;
            cx.notify();
            return;
        }
        self.suggestions.clear();
        self.suggestions_key = None;
        self.selected_suggestion = None;
        self.suggestions_loaded = false;
        let options = SshTarget { address, folder: directory.clone() }.connection_options();
        let Ok(options) = options else {
            self.loading_suggestions = false;
            cx.notify();
            return;
        };
        if let Some((paths, fresh)) = cached_directories(&cache_key) {
            log::info!("iZed SSH directories: cache hit ({} entries, fresh={fresh})", paths.len());
            self.suggestions = paths;
            self.suggestions_key = Some(cache_key.clone());
            self.suggestions_loaded = true;
            if !self.filtered_suggestions(cx).is_empty() {
                self.selected_suggestion = Some(0);
                self.suggestion_scroll.scroll_to_item(0);
            }
            if fresh {
                self.loading_suggestions = false;
                if self.embedded && typed_path.ends_with('/') {
                    cx.emit(DirectoryBrowserEvent::Navigated(SshTarget {
                        address: cache_key.0.clone(),
                        folder: typed_path.clone(),
                    }));
                }
                cx.notify();
                return;
            }
        }
        self.loading_suggestions = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            if !this.read_with(cx, |this, _| this.query_generation == generation).unwrap_or(false) {
                return;
            }
            let started = Instant::now();
            let result = remote::list_ssh_directories(options, directory).await;
            if let Ok(paths) = &result {
                log::info!("iZed SSH directories: listed {} entries in {} ms", paths.len(), started.elapsed().as_millis());
                cache_directories(cache_key.clone(), paths.clone());
            }
            let completion_target = this.clone();
            let _ = this.update(cx, |this, cx| {
                if this.query_generation != generation { return; }
                this.loading_suggestions = false;
                match result {
                    Ok(paths) => {
                        this.suggestions = paths;
                        this.suggestions_key = Some(cache_key);
                        this.suggestions_loaded = true;
                        let folder = this.folder.read(cx).text(cx);
                        if this.embedded && folder.ends_with('/') {
                            cx.emit(DirectoryBrowserEvent::Navigated(SshTarget {
                                address: this.address.read(cx).text(cx).trim().to_owned(),
                                folder,
                            }));
                        }
                        if !this.filtered_suggestions(cx).is_empty() {
                            this.selected_suggestion = Some(0);
                            this.suggestion_scroll.scroll_to_item(0);
                        }
                    }
                    Err(error) => this.suggestion_error = Some(error.to_string().into()),
                }
                if this.pending_tab_completion.take() == Some(generation) {
                    if let Some(path) = this.selected_suggestion
                        .and_then(|index| this.filtered_suggestions(cx).get(index).cloned()) {
                        let window_handle = this.window.clone();
                        cx.defer(move |cx| {
                            let _ = window_handle.update(cx, |_, window, cx| {
                                let _ = completion_target.update(cx, |this, cx| {
                                    if this.query_generation == generation && this.folder_focused {
                                        this.select_suggestion(path, window, cx);
                                    }
                                });
                            });
                        });
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn prefetch_selected_directory(&mut self, cx: &mut Context<Self>) {
        self.prefetch_generation += 1;
        let generation = self.prefetch_generation;
        let query_generation = self.query_generation;
        let Some(index) = self.selected_suggestion else { return; };
        let Some(directory) = self.filtered_suggestions(cx).get(index).cloned() else { return; };
        let address = self.address.read(cx).text(cx);
        let cache_key = (address.trim().to_owned(), directory.clone());
        let Ok(options) = (SshTarget { address, folder: directory.clone() }).connection_options() else { return; };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            if !this.read_with(cx, |this, _| {
                this.query_generation == query_generation && this.prefetch_generation == generation
            }).unwrap_or(false) { return; }
            if cached_directories(&cache_key).is_some_and(|(_, fresh)| fresh) { return; }
            {
                let mut cache = directory_cache().lock().unwrap_or_else(|poison| poison.into_inner());
                if cache.prefetching.is_some() { return; }
                cache.prefetching = Some(cache_key.clone());
            }
            let started = Instant::now();
            let result = remote::list_ssh_directories(options, directory).await;
            {
                let mut cache = directory_cache().lock().unwrap_or_else(|poison| poison.into_inner());
                cache.prefetching = None;
            }
            if let Ok(paths) = result {
                log::info!("iZed SSH directories: prefetched {} entries in {} ms", paths.len(), started.elapsed().as_millis());
                cache_directories(cache_key.clone(), paths);
            }
            let _ = this.update(cx, |this, cx| {
                let selected = this.selected_suggestion
                    .and_then(|index| this.filtered_suggestions(cx).get(index).cloned());
                if this.folder_focused && selected.as_deref().is_some_and(|path| path != cache_key.1) {
                    this.prefetch_selected_directory(cx);
                }
            });
        }).detach();
    }

    fn select_suggestion(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let typed_path = self.folder.read(cx).text(cx);
        let display_path = if typed_path.starts_with('~') {
            self.remote_home.as_ref().and_then(|(_, home)| {
                path.strip_prefix(home).and_then(|suffix| {
                    (suffix.is_empty() || suffix.starts_with('/')).then(|| format!("~{suffix}"))
                })
            }).unwrap_or(path)
        } else {
            path
        };
        self.folder.update(cx, |editor, cx| editor.set_text(format!("{display_path}/"), window, cx));
        if self.embedded {
            cx.emit(DirectoryBrowserEvent::Navigated(SshTarget {
                address: self.address.read(cx).text(cx).trim().to_owned(),
                folder: format!("{display_path}/"),
            }));
        }
        self.folder.focus_handle(cx).focus(window, cx);
    }

    fn filtered_suggestions(&self, cx: &App) -> Vec<String> {
        let typed_path = self.folder.read(cx).text(cx);
        let prefix = if typed_path.ends_with('/') {
            ""
        } else {
            Path::new(typed_path.trim()).file_name().and_then(|name| name.to_str()).unwrap_or("")
        };
        self.suggestions.iter()
            .filter(|path| path.rsplit('/').next().is_some_and(|name| name.to_lowercase().starts_with(&prefix.to_lowercase())))
            .cloned()
            .collect()
    }

    fn current_directory_key(&self, cx: &App) -> DirectoryCacheKey {
        let typed_path = self.folder.read(cx).text(cx);
        let resolved_path = self.resolved_path(&typed_path, cx).unwrap_or(typed_path.clone());
        let path = Path::new(resolved_path.trim());
        let directory = if typed_path == "~" || typed_path.ends_with('/') { path } else { path.parent().unwrap_or(path) };
        (self.address.read(cx).text(cx).trim().to_owned(), directory.components().collect::<PathBuf>().to_string_lossy().into_owned())
    }

    fn suggestions_are_current(&self, cx: &App) -> bool {
        self.suggestions_key.as_ref() == Some(&self.current_directory_key(cx))
    }

    fn next_suggestion(&mut self, _: &NextSuggestion, _: &mut Window, cx: &mut Context<Self>) {
        if !self.folder_focused || !self.suggestions_are_current(cx) { return; }
        let count = self.filtered_suggestions(cx).len();
        if count == 0 { return; }
        self.selected_suggestion = Some(self.selected_suggestion.map_or(0, |index| (index + 1) % count));
        self.suggestion_scroll.scroll_to_item(self.selected_suggestion.unwrap());
        self.prefetch_selected_directory(cx);
        cx.notify();
    }

    fn previous_suggestion(&mut self, _: &PreviousSuggestion, _: &mut Window, cx: &mut Context<Self>) {
        if !self.folder_focused || !self.suggestions_are_current(cx) { return; }
        let count = self.filtered_suggestions(cx).len();
        if count == 0 { return; }
        self.selected_suggestion = Some(self.selected_suggestion.map_or(count - 1, |index| (index + count - 1) % count));
        self.suggestion_scroll.scroll_to_item(self.selected_suggestion.unwrap());
        self.prefetch_selected_directory(cx);
        cx.notify();
    }

    fn accept_suggestion(&mut self, _: &AcceptSuggestion, window: &mut Window, cx: &mut Context<Self>) {
        if !self.folder_focused {
            let address = self.address.read(cx).text(cx);
            if let Err(error) = (SshTarget { address, folder: "/".into() }).connection_options() {
                self.error = Some(error.to_string().into());
                cx.notify();
                return;
            }
            self.error = None;
            self.folder.focus_handle(cx).focus(window, cx);
            return;
        }
        if !self.suggestions_are_current(cx) {
            let key = self.current_directory_key(cx);
            if let Some((paths, _)) = cached_directories(&key) {
                self.suggestions = paths;
                self.suggestions_key = Some(key);
                self.selected_suggestion = (!self.filtered_suggestions(cx).is_empty()).then_some(0);
            }
        }
        if !self.suggestions_are_current(cx) {
            if self.loading_suggestions { self.pending_tab_completion = Some(self.query_generation); }
            return;
        }
        let Some(path) = self.selected_suggestion.and_then(|index| self.filtered_suggestions(cx).get(index).cloned()) else {
            if self.loading_suggestions { self.pending_tab_completion = Some(self.query_generation); }
            else { cx.propagate(); }
            return;
        };
        self.select_suggestion(path, window, cx);
    }

    fn select_directory(&mut self, _: &SelectDirectory, _: &mut Window, cx: &mut Context<Self>) {
        let folder = self.selected_suggestion
            .and_then(|index| self.filtered_suggestions(cx).get(index).cloned())
            .unwrap_or_else(|| self.folder.read(cx).text(cx));
        let target = SshTarget { address: self.address.read(cx).text(cx), folder };
        self.open_target(target, cx);
    }

    fn open_target(&mut self, mut target: SshTarget, cx: &mut Context<Self>) {
        if self.opening_folder.is_some() { return; }
        let displayed_folder = target.folder.clone();
        let Some(folder) = self.resolved_path(&target.folder, cx) else {
            self.error = Some("Still looking up the Machine home directory".into());
            cx.notify();
            return;
        };
        target.folder = folder;
        if let Err(error) = target.connection_options() {
            self.error = Some(error.to_string().into());
            cx.notify();
            return;
        }
        if self.embedded {
            self.opening_folder = Some(displayed_folder);
            cx.notify();
            cx.emit(DirectoryBrowserEvent::Open(target));
        } else {
            cx.emit(DismissEvent);
            open_ssh_project(self.window.clone(), self.app_state.clone(), target, None, cx);
        }
    }

    fn connect(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let target = SshTarget {
            address: self.address.read(cx).text(cx),
            folder: self.folder.read(cx).text(cx),
        };
        self.open_target(target, cx);
    }
}

impl Focusable for SshTargetModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.fixed_address { self.folder.focus_handle(cx) } else { self.address.focus_handle(cx) }
    }
}

impl EventEmitter<DismissEvent> for SshTargetModal {}
impl EventEmitter<DirectoryBrowserEvent> for SshTargetModal {}
impl workspace::ModalView for SshTargetModal {}

impl Render for SshTargetModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed_path = self.folder.read(cx).text(cx);
        let suggestions = if self.folder_focused { self.filtered_suggestions(cx) } else { Vec::new() };
        let no_matching_folders = self.folder_focused && self.suggestions_loaded && suggestions.is_empty();
        v_flex()
            .key_context("SshTargetModal")
            .on_action(cx.listener(Self::next_suggestion))
            .on_action(cx.listener(Self::previous_suggestion))
            .on_action(cx.listener(Self::accept_suggestion))
            .on_action(cx.listener(Self::select_directory))
            .when(self.embedded, |this| this.w_full().h_full().justify_between())
            .when(!self.embedded, |this| this.w(rems(34.)).elevation_2(cx).p_3().gap_2())
            .child(v_flex().gap_2()
            .child(Label::new(if self.fixed_address { "Browse Directories" } else { "Open SSH Project" }).size(LabelSize::Large))
            .when(!self.fixed_address, |this| this
                .child(Label::new("SSH address").color(Color::Muted))
                .child(self.address.clone()))
            .child(div().w_full().p_2().rounded_sm().border_1()
                .border_color(cx.theme().colors().border)
                .child(self.folder.clone()))
            .when(self.loading_suggestions, |this| this.child(Label::new("Looking up folders…").color(Color::Muted)))
            .when_some(self.suggestion_error.clone(), |this, error| {
                this.child(Label::new(error).color(Color::Error))
            })
            .when(no_matching_folders, |this| {
                this.child(Label::new(if typed_path.ends_with('/') {
                    "No subfolders here"
                } else {
                    "No matching folders"
                }).color(Color::Muted))
            })
            .child(Label::new("Directories").color(Color::Muted))
            .child(v_flex().id("ssh-directory-list")
                .when(self.embedded, |this| this.h(rems(28.)))
                .when(!self.embedded, |this| this.max_h(rems(20.)))
                .overflow_y_scroll()
                .track_scroll(&self.suggestion_scroll)
                .children(suggestions.into_iter().enumerate().map(|(index, path)| {
                    let label = format!("{}/", path.rsplit('/').next().unwrap_or(&path));
                    ui::ListItem::new(format!("ssh-folder-{index}"))
                        .toggle_state(self.selected_suggestion == Some(index))
                        .height(px(44.))
                        .inset(true)
                        .child(h_flex().w_full().items_center().justify_between()
                            .child(Label::new(label))
                            .when(self.opening_folder.as_deref() == Some(path.as_str()), |this| this.child(
                                Icon::new(IconName::LoadCircle).size(IconSize::Small)
                                    .with_keyed_rotate_animation("opening-ssh-directory", 1)
                            )))
                        .on_click(cx.listener(move |this, _, window, cx| this.select_suggestion(path.clone(), window, cx)))
                })))
            .when_some(self.error.clone(), |this, error| {
                this.child(Label::new(error).color(Color::Error))
            }))
            .child(h_flex().w_full().justify_between()
                .when(self.embedded, |this| this.child(
                    ipad_picker_button("back-to-machine-projects", "Back to Projects", window, cx)
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DirectoryBrowserEvent::Back))),
                ))
                .child(h_flex().gap_2()
                    .when(self.opening_folder.is_some(), |this| this.child(
                        Icon::new(IconName::LoadCircle).size(IconSize::Small)
                            .with_keyed_rotate_animation("opening-ssh-folder", 1)
                    ))
                .child(
                ipad_picker_button("connect-ssh-project", "Open This Folder", window, cx)
                    .on_click(cx.listener(Self::connect)),
            )))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PickerStage { Browse, Directories, Add, Verify, Authorize, ConfirmRemove }

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectionVisualState { Idle, Checking, Success, Failure }

impl ConnectionVisualState {
    fn icon(self) -> Icon {
        match self {
            Self::Idle => Icon::new(IconName::Reconnect),
            Self::Checking => Icon::new(IconName::LoadCircle),
            Self::Success => Icon::from_path("icons/check_circle.svg"),
            Self::Failure => Icon::new(IconName::XCircle).color(Color::Error),
        }
    }
}

fn connection_icon(state: ConnectionVisualState, generation: u64) -> ui::AnyIcon {
    let icon = state.icon();
    if state == ConnectionVisualState::Checking {
        icon.with_keyed_rotate_animation(format!("connection-spin-{generation}"), 1).into()
    } else {
        icon.into()
    }
}

fn connection_icon_layer(state: ConnectionVisualState, generation: u64, outgoing: bool) -> impl IntoElement {
    h_flex().absolute().inset_0().items_center().justify_center()
        .child(connection_icon(state, generation))
        .with_animation(
            format!("connection-icon-{}-{generation}", if outgoing { "out" } else { "in" }),
            Animation::new(Duration::from_millis(180)).with_easing(gpui::ease_out_quint()),
            move |this, delta| this.opacity(if outgoing { 1. - delta } else { delta }),
        )
}

struct MachinePicker {
    store: MachineStore,
    targets: Vec<SshTarget>,
    machine_search: Entity<editor::Editor>,
    project_search: Entity<editor::Editor>,
    machine_name: Entity<editor::Editor>,
    machine_address: Entity<editor::Editor>,
    directory_browser: Option<Entity<SshTargetModal>>,
    selected_machine: usize,
    selected_project: usize,
    opening_target: Option<SshTarget>,
    editing_index: Option<usize>,
    focused_projects: bool,
    address_focused: bool,
    stage: PickerStage,
    identity: Option<remote::SshHostIdentity>,
    setup_command: Option<String>,
    server_progress: Option<Arc<remote::SshServerProgress>>,
    busy: bool,
    error: Option<SharedString>,
    status: Option<SharedString>,
    status_generation: u64,
    connection_state: ConnectionVisualState,
    connection_previous_state: Option<ConnectionVisualState>,
    connection_transition_generation: u64,
    focus_handle: FocusHandle,
    window: WindowHandle<MultiWorkspace>,
    app_state: Arc<AppState>,
}

impl MachinePicker {
    fn set_connection_state(&mut self, state: ConnectionVisualState) {
        if self.connection_state != state {
            self.connection_previous_state = Some(self.connection_state);
            self.connection_state = state;
            self.connection_transition_generation += 1;
        }
    }

    fn remember_browse_path(&mut self, address: &str, folder: &str) {
        if !(folder == "~" || folder.starts_with("~/") || Path::new(folder).is_absolute()) {
            return;
        }
        let folder = if folder == "~" { "~/".to_owned() }
            else if folder.ends_with('/') { folder.to_owned() }
            else { format!("{folder}/") };
        if self.store.browse_paths.get(address).is_some_and(|saved| saved == &folder) {
            return;
        }
        self.store.browse_paths.insert(address.to_owned(), folder);
        if let Err(error) = self.store.save() {
            log::warn!("Could not save last browsed directory: {error:#}");
        }
    }

    fn new(window: WindowHandle<MultiWorkspace>, app_state: Arc<AppState>, gpui_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = MachineStore::load();
        let selected_machine = store.selected_address.as_ref()
            .and_then(|address| store.machines.iter().position(|machine| &machine.address == address))
            .unwrap_or(0);
        let machine_search = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(gpui_window, cx);
            editor.set_placeholder_text("Search machines…", gpui_window, cx);
            editor
        });
        let project_search = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(gpui_window, cx);
            editor.set_placeholder_text("Search projects…", gpui_window, cx);
            editor
        });
        let machine_name = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(gpui_window, cx);
            editor.set_placeholder_text("Home Mac", gpui_window, cx);
            editor
        });
        let machine_address = cx.new(|cx| {
            let mut editor = editor::Editor::single_line(gpui_window, cx);
            editor.set_placeholder_text("user@host or ssh user@host -p 2222", gpui_window, cx);
            editor
        });
        cx.subscribe(&machine_search, |this, _, event: &editor::EditorEvent, cx| {
            match event {
                editor::EditorEvent::Focused => this.focused_projects = false,
                editor::EditorEvent::BufferEdited => this.selected_machine = 0,
                _ => {}
            }
            cx.notify();
        }).detach();
        cx.subscribe(&project_search, |this, _, event: &editor::EditorEvent, cx| {
            match event {
                editor::EditorEvent::Focused => this.focused_projects = true,
                editor::EditorEvent::BufferEdited => this.selected_project = 0,
                _ => {}
            }
            cx.notify();
        }).detach();
        cx.subscribe(&machine_address, |this, _, event: &editor::EditorEvent, cx| {
            if matches!(event, editor::EditorEvent::Focused) { this.address_focused = true; }
            cx.notify();
        }).detach();
        cx.subscribe(&machine_name, |this, _, event: &editor::EditorEvent, cx| {
            if matches!(event, editor::EditorEvent::Focused) { this.address_focused = false; }
            cx.notify();
        }).detach();
        let targets = if SshTarget::config_path().exists() { vec![SshTarget::load()] } else { Vec::new() };
        let db = workspace::WorkspaceDb::global(cx);
        let fs = app_state.fs.clone();
        cx.spawn_in(gpui_window, async move |this, cx| {
            match db.recent_workspaces_on_disk(fs.as_ref()).await {
                Ok(recents) => {
                    let _ = this.update(cx, |this, cx| {
                        for (_, location, paths, _) in recents {
                            let workspace::SerializedWorkspaceLocation::Remote(RemoteConnectionOptions::Ssh(options)) = location else { continue; };
                            let Some(folder) = paths.paths().first() else { continue; };
                            let mut address = options.host.to_string();
                            if let Some(username) = options.username { address = format!("{username}@{address}"); }
                            if let Some(port) = options.port { address = format!("ssh {address} -p {port}"); }
                            let target = SshTarget { address, folder: folder.to_string_lossy().into_owned() };
                            if !this.targets.iter().any(|existing| existing.same_project(&target)) {
                                this.targets.push(target);
                            }
                        }
                        cx.notify();
                    });
                }
                Err(error) => log::warn!("Could not load recent SSH projects: {error:#}"),
            }
        }).detach();
        Self {
            store, targets, machine_search, project_search, machine_name, machine_address, directory_browser: None,
            selected_machine, selected_project: 0, opening_target: None, editing_index: None, focused_projects: false, address_focused: false,
            stage: PickerStage::Browse, identity: None, setup_command: None, server_progress: None, busy: false, error: None, status: None, status_generation: 0,
            connection_state: ConnectionVisualState::Idle, connection_previous_state: None, connection_transition_generation: 0,
            focus_handle: cx.focus_handle(),
            window, app_state,
        }
    }

    fn visible_machines(&self, cx: &App) -> Vec<usize> {
        let query = self.machine_search.read(cx).text(cx).trim().to_lowercase();
        self.store.machines.iter().enumerate().filter_map(|(index, machine)| {
            (machine.name.to_lowercase().contains(&query) || machine.address.to_lowercase().contains(&query)).then_some(index)
        }).collect()
    }

    fn current_machine(&self, cx: &App) -> Option<&SshMachine> {
        let index = *self.visible_machines(cx).get(self.selected_machine)?;
        self.store.machines.get(index)
    }

    fn visible_projects(&self, cx: &App) -> Vec<usize> {
        let Some(machine) = self.current_machine(cx) else { return Vec::new(); };
        let query = self.project_search.read(cx).text(cx).trim().to_lowercase();
        self.targets.iter().enumerate().filter_map(|(index, target)| {
            (target.address == machine.address && target.folder.to_lowercase().contains(&query)).then_some(index)
        }).collect()
    }

    fn select_machine(&mut self, position: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_target.is_some() { return; }
        self.selected_machine = position;
        self.selected_project = 0;
        self.stage = PickerStage::Browse;
        self.directory_browser = None;
        self.editing_index = None;
        self.identity = None;
        self.setup_command = None;
        self.server_progress = None;
        self.error = None;
        self.status = None;
        self.status_generation += 1;
        self.connection_state = ConnectionVisualState::Idle;
        self.connection_previous_state = None;
        self.connection_transition_generation += 1;
        self.busy = false;
        if let Some(machine) = self.current_machine(cx) {
            self.store.selected_address = Some(machine.address.clone());
            if let Err(error) = self.store.save() { log::warn!("Could not save selected machine: {error:#}"); }
        }
        self.project_search.update(cx, |editor, cx| editor.set_text("", window, cx));
        self.focused_projects = true;
        self.project_search.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn open_project(&mut self, position: usize, cx: &mut Context<Self>) {
        if self.opening_target.is_some() { return; }
        let Some(index) = self.visible_projects(cx).get(position).copied() else { return; };
        let target = self.targets[index].clone();
        self.opening_target = Some(target.clone());
        self.selected_project = position;
        cx.notify();
        open_ssh_project(self.window.clone(), self.app_state.clone(), target, Some(cx.entity().downgrade()), cx);
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_target.is_some() { return; }
        let Some(machine) = self.current_machine(cx).cloned() else { return; };
        let folder = self.store.browse_paths.get(&machine.address).cloned().or_else(|| {
            self.targets.iter().find(|target| target.address == machine.address)
                .and_then(|target| Path::new(&target.folder).parent().map(|path| path.to_string_lossy().into_owned()))
        }).unwrap_or_else(|| "~/".into());
        let target = SshTarget { address: machine.address, folder };
        let window_handle = self.window.clone();
        let app_state = self.app_state.clone();
        let browser = cx.new(|cx| {
            let mut browser = SshTargetModal::new(window_handle, app_state, Some(target), window, cx);
            browser.embedded = true;
            browser
        });
        cx.subscribe_in(&browser, window, |this, _, event, window, cx| match event {
            DirectoryBrowserEvent::Open(target) => {
                let target = target.clone();
                if this.opening_target.is_some() { return; }
                this.opening_target = Some(target.clone());
                cx.notify();
                open_ssh_project(this.window.clone(), this.app_state.clone(), target, Some(cx.entity().downgrade()), cx);
            }
            DirectoryBrowserEvent::Navigated(target) => {
                this.remember_browse_path(&target.address, &target.folder);
            }
            DirectoryBrowserEvent::Back => {
                this.stage = PickerStage::Browse;
                this.directory_browser = None;
                this.project_search.focus_handle(cx).focus(window, cx);
                cx.notify();
            }
        }).detach();
        self.stage = PickerStage::Directories;
        self.directory_browser = Some(browser.clone());
        browser.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn add_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stage = PickerStage::Add;
        self.directory_browser = None;
        self.editing_index = None;
        self.error = None;
        self.identity = None;
        self.machine_search.update(cx, |editor, cx| editor.set_text("", window, cx));
        self.machine_name.update(cx, |editor, cx| editor.set_text("", window, cx));
        self.machine_address.update(cx, |editor, cx| editor.set_text("", window, cx));
        self.machine_name.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn edit_machine(&mut self, _: &PickerEditMachine, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage != PickerStage::Browse { return; }
        let Some(index) = self.visible_machines(cx).get(self.selected_machine).copied() else { return; };
        let machine = self.store.machines[index].clone();
        self.editing_index = Some(index);
        self.stage = PickerStage::Add;
        self.error = None;
        self.machine_name.update(cx, |editor, cx| editor.set_text(machine.name, window, cx));
        self.machine_address.update(cx, |editor, cx| editor.set_text(machine.address, window, cx));
        self.machine_name.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn prompt_remove_machine(&mut self, _: &PickerRemoveMachine, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage != PickerStage::Browse || self.current_machine(cx).is_none() { return; }
        self.stage = PickerStage::ConfirmRemove;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn remove_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.visible_machines(cx).get(self.selected_machine).copied() else { return; };
        let removed = self.store.machines.remove(index);
        self.store.browse_paths.remove(&removed.address);
        self.store.selected_address = self.store.machines.first().map(|machine| machine.address.clone());
        if let Err(error) = self.store.save() { self.error = Some(error.to_string().into()); }
        self.selected_machine = 0;
        self.selected_project = 0;
        self.stage = PickerStage::Browse;
        self.machine_search.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn candidate(&self, cx: &App) -> anyhow::Result<SshMachine> {
        let name = self.machine_name.read(cx).text(cx).trim().to_owned();
        let address = self.machine_address.read(cx).text(cx).trim().to_owned();
        anyhow::ensure!(!name.is_empty(), "Give this machine a name");
        let target = SshTarget { address: address.clone(), folder: "/".into() };
        target.connection_options()?;
        anyhow::ensure!(!self.store.machines.iter().enumerate().any(|(index, machine)| Some(index) != self.editing_index && machine.address == address), "This machine is already saved");
        Ok(SshMachine { name, address })
    }

    fn inspect_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy { return; }
        let machine = match self.candidate(cx) { Ok(machine) => machine, Err(error) => {
            self.error = Some(error.to_string().into()); cx.notify(); return;
        }};
        if let Some(index) = self.editing_index {
            if self.store.machines[index].address == machine.address {
                self.store.machines[index] = machine;
                match self.store.save() {
                    Ok(()) => {
                        self.stage = PickerStage::Browse;
                        self.project_search.focus_handle(cx).focus(window, cx);
                    }
                    Err(error) => self.error = Some(error.to_string().into()),
                }
                cx.notify();
                return;
            }
        }
        let checked_address = machine.address.clone();
        let options = SshTarget { address: machine.address, folder: "/".into() }.connection_options().unwrap();
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = remote::probe_ssh_host(options).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.stage != PickerStage::Add || this.machine_address.read(cx).text(cx).trim() != checked_address {
                    return;
                }
                this.busy = false;
                match result {
                    Ok(identity) => {
                        let trusted = identity.trusted;
                        this.identity = Some(identity);
                        if trusted { this.prepare_access(cx); } else { this.stage = PickerStage::Verify; }
                        this.focus_handle.focus(window, cx);
                    }
                    Err(error) => this.error = Some(error.to_string().into()),
                }
                cx.notify();
            });
        }).detach();
    }

    fn prepare_access(&mut self, cx: &mut Context<Self>) {
        match remote::ipad_public_key() {
            Ok(public_key) => {
                self.setup_command = Some(format!(
                    "mkdir -p ~/.ssh && chmod 700 ~/.ssh && printf '%s\\n' '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
                    public_key.trim()
                ));
                self.stage = PickerStage::Authorize;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string().into()),
        }
        cx.notify();
    }

    fn verification_command(&self) -> &'static str {
        match self.identity.as_ref().and_then(|identity| identity.public_key.split_whitespace().next()) {
            Some("ssh-ed25519") => "ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256",
            Some("ssh-rsa") => "ssh-keygen -lf /etc/ssh/ssh_host_rsa_key.pub -E sha256",
            Some(algorithm) if algorithm.starts_with("ecdsa-") => "ssh-keygen -lf /etc/ssh/ssh_host_ecdsa_key.pub -E sha256",
            _ => "ssh-keygen -lf /etc/ssh/ssh_host_*_key.pub -E sha256",
        }
    }

    fn trust_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(identity) = self.identity.as_ref() else { return; };
        let Ok(machine) = self.candidate(cx) else { return; };
        let Ok(options) = (SshTarget { address: machine.address, folder: "/".into() }).connection_options() else { return; };
        match remote::trust_ssh_host(&options, identity) {
            Ok(()) => { self.prepare_access(cx); self.focus_handle.focus(window, cx); },
            Err(error) => { self.error = Some(error.to_string().into()); cx.notify(); }
        }
    }

    fn copy_setup_command(&mut self, cx: &mut Context<Self>) {
        if let Some(command) = &self.setup_command {
            cx.write_to_clipboard(ClipboardItem::new_string(command.clone()));
        }
    }

    fn check_access(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy { return; }
        let machine = match self.candidate(cx) { Ok(machine) => machine, Err(error) => {
            self.error = Some(error.to_string().into()); cx.notify(); return;
        }};
        let options = SshTarget { address: machine.address.clone(), folder: "/".into() }.connection_options().unwrap();
        let progress = Arc::new(remote::SshServerProgress::default());
        self.server_progress = Some(progress.clone());
        self.busy = true;
        self.error = None;
        cx.notify();
        let progress_for_refresh = progress.clone();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(100)).await;
                let still_active = this.update_in(cx, |this, _, cx| {
                    let active = this.busy && this.server_progress.as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &progress_for_refresh));
                    if active { cx.notify(); }
                    active
                }).unwrap_or(false);
                if !still_active { break; }
            }
        }).detach();
        cx.spawn_in(window, async move |this, cx| {
            let result: anyhow::Result<()> = async {
                remote::check_ssh_directory(options.clone(), "/".into()).await?;
                remote::ensure_ssh_server(options, bundled_server_archives()?, Some(progress)).await?;
                Ok(())
            }.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.stage != PickerStage::Authorize || this.machine_address.read(cx).text(cx).trim() != machine.address {
                    return;
                }
                this.busy = false;
                this.server_progress = None;
                match result {
                    Ok(()) => {
                        this.store.selected_address = Some(machine.address.clone());
                        if let Some(index) = this.editing_index.take() {
                            if this.store.machines[index].address != machine.address {
                                this.store.browse_paths.remove(&this.store.machines[index].address);
                            }
                            this.store.machines[index] = machine;
                        } else {
                            this.store.machines.push(machine);
                        }
                        if let Err(error) = this.store.save() {
                            this.error = Some(error.to_string().into());
                        } else {
                            this.selected_machine = this.store.machines.iter().position(|machine| Some(&machine.address) == this.store.selected_address.as_ref()).unwrap_or(0);
                            this.stage = PickerStage::Browse;
                            this.focused_projects = true;
                            this.project_search.focus_handle(cx).focus(window, cx);
                        }
                    }
                    Err(error) => this.error = Some(format!("Access is not ready: {error}").into()),
                }
                cx.notify();
            });
        }).detach();
    }

    fn check_saved_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.stage != PickerStage::Browse { return; }
        let Some(machine) = self.current_machine(cx).cloned() else { return; };
        let options = match (SshTarget { address: machine.address.clone(), folder: "/".into() }).connection_options() {
            Ok(options) => options,
            Err(error) => {
                self.error = Some(error.to_string().into());
                self.set_connection_state(ConnectionVisualState::Failure);
                cx.notify();
                return;
            }
        };
        let progress = Arc::new(remote::SshServerProgress::default());
        self.server_progress = Some(progress.clone());
        self.busy = true;
        self.error = None;
        self.status = None;
        self.status_generation += 1;
        self.set_connection_state(ConnectionVisualState::Checking);
        cx.notify();
        let progress_for_refresh = progress.clone();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                smol::Timer::after(Duration::from_millis(100)).await;
                let still_active = this.update_in(cx, |this, _, cx| {
                    let active = this.busy && this.server_progress.as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &progress_for_refresh));
                    if active { cx.notify(); }
                    active
                }).unwrap_or(false);
                if !still_active { break; }
            }
        }).detach();
        cx.spawn_in(window, async move |this, cx| {
            let result: anyhow::Result<()> = async {
                remote::check_ssh_directory(options.clone(), "/".into()).await?;
                remote::ensure_ssh_server(options, bundled_server_archives()?, Some(progress.clone())).await?;
                Ok(())
            }.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.stage != PickerStage::Browse || this.current_machine(cx).is_none_or(|current| current.address != machine.address) {
                    return;
                }
                this.busy = false;
                this.server_progress = None;
                match result {
                    Ok(()) => {
                        this.set_connection_state(ConnectionVisualState::Success);
                        let (_, sent, _) = progress.snapshot();
                        this.status = Some(if sent > 0 {
                            format!("Transferred {} MB · Ready", sent.div_ceil(1_000_000))
                        } else {
                            "Machine is ready".to_owned()
                        }.into());
                        let generation = this.status_generation;
                        cx.spawn_in(window, async move |this, cx| {
                            smol::Timer::after(Duration::from_secs(3)).await;
                            let _ = this.update(cx, |this, cx| {
                                if this.status_generation == generation {
                                    this.status = None;
                                    this.set_connection_state(ConnectionVisualState::Idle);
                                    cx.notify();
                                }
                            });
                        }).detach();
                    }
                    Err(error) => {
                        this.error = Some(format!("Connection is not ready: {error}").into());
                        this.set_connection_state(ConnectionVisualState::Failure);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    fn next(&mut self, _: &PickerNext, _: &mut Window, cx: &mut Context<Self>) {
        if self.stage != PickerStage::Browse { return; }
        if self.focused_projects {
            let count = self.visible_projects(cx).len() + 1;
            self.selected_project = (self.selected_project + 1) % count;
        } else {
            let count = self.visible_machines(cx).len() + 1;
            self.selected_machine = (self.selected_machine + 1) % count;
        }
        cx.notify();
    }

    fn previous(&mut self, _: &PickerPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if self.stage != PickerStage::Browse { return; }
        if self.focused_projects {
            let count = self.visible_projects(cx).len() + 1;
            self.selected_project = (self.selected_project + count - 1) % count;
        } else {
            let count = self.visible_machines(cx).len() + 1;
            self.selected_machine = (self.selected_machine + count - 1) % count;
        }
        cx.notify();
    }

    fn accept(&mut self, _: &PickerAccept, window: &mut Window, cx: &mut Context<Self>) {
        match self.stage {
            PickerStage::Browse if !self.focused_projects => {
                if self.selected_machine == self.visible_machines(cx).len() { self.add_machine(window, cx); }
                else { self.select_machine(self.selected_machine, window, cx); }
            }
            PickerStage::Browse => {
                if self.selected_project == self.visible_projects(cx).len() { self.browse(window, cx); }
                else { self.open_project(self.selected_project, cx); }
            }
            PickerStage::Directories => cx.propagate(),
            PickerStage::Add if !self.address_focused => self.machine_address.focus_handle(cx).focus(window, cx),
            PickerStage::Add => self.inspect_machine(window, cx),
            PickerStage::Verify => self.trust_machine(window, cx),
            PickerStage::Authorize => self.check_access(window, cx),
            PickerStage::ConfirmRemove => self.remove_machine(window, cx),
        }
    }

    fn back(&mut self, _: &PickerBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.opening_target.is_some() { return; }
        match self.stage {
            PickerStage::Browse if self.focused_projects => self.machine_search.focus_handle(cx).focus(window, cx),
            PickerStage::Browse => {
                let has_remote_project = self.window.update(cx, |multi, _, cx| {
                    multi.workspace().read(cx).project().read(cx).is_remote()
                }).unwrap_or(false);
                if has_remote_project { cx.emit(DismissEvent); }
            },
            PickerStage::Directories => {
                self.stage = PickerStage::Browse;
                self.directory_browser = None;
                self.project_search.focus_handle(cx).focus(window, cx);
            },
            PickerStage::Add => { self.stage = PickerStage::Browse; self.machine_search.focus_handle(cx).focus(window, cx); },
            PickerStage::Verify => { self.stage = PickerStage::Add; self.machine_address.focus_handle(cx).focus(window, cx); },
            PickerStage::Authorize => { self.stage = PickerStage::Verify; },
            PickerStage::ConfirmRemove => { self.stage = PickerStage::Browse; self.machine_search.focus_handle(cx).focus(window, cx); },
        }
        cx.notify();
    }

    fn switch_column(&mut self, _: &PickerSwitchColumn, window: &mut Window, cx: &mut Context<Self>) {
        if self.stage != PickerStage::Browse { return; }
        if self.focused_projects { self.machine_search.focus_handle(cx).focus(window, cx); }
        else if self.current_machine(cx).is_some() { self.project_search.focus_handle(cx).focus(window, cx); }
    }
}

impl Focusable for MachinePicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle { self.machine_search.focus_handle(cx) }
}
impl EventEmitter<DismissEvent> for MachinePicker {}
impl workspace::ModalView for MachinePicker {
    fn render_bare(&self) -> bool { true }
}

impl Render for MachinePicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let machines = self.visible_machines(cx);
        let projects = self.visible_projects(cx);
        let machine_name = self.current_machine(cx).map(|machine| machine.name.clone());
        let project_count = projects.len();
        let machine_count = machines.len();
        let server_progress = self.server_progress.as_ref().map(|progress| progress.snapshot());
        let connection_label: SharedString = if let Some((phase, sent, total)) = server_progress {
            match phase {
                remote::SshServerPhase::Checking => "Checking connection…".to_owned(),
                remote::SshServerPhase::Uploading => format!("Transferring… {}%", sent.saturating_mul(100) / total.max(1)),
                remote::SshServerPhase::Installing => "Installing server…".to_owned(),
                remote::SshServerPhase::Ready => "Machine is ready".to_owned(),
            }.into()
        } else if let Some(status) = &self.status {
            status.clone()
        } else if self.connection_state == ConnectionVisualState::Failure {
            "Connection failed · Retry".into()
        } else {
            "Check Connection".into()
        };
        let transfer_fraction = server_progress.and_then(|(phase, sent, total)| {
            (phase == remote::SshServerPhase::Uploading).then(|| (sent as f32 / total.max(1) as f32).clamp(0., 1.))
        });
        h_flex()
            .absolute()
            .inset_0()
            .size_full()
            .items_center()
            .justify_center()
            .p_4()
            .bg(cx.theme().colors().editor_background)
            .key_context("MachinePicker")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::back))
            .on_action(cx.listener(Self::switch_column))
            .on_action(cx.listener(Self::edit_machine))
            .on_action(cx.listener(Self::prompt_remove_machine))
            .child(h_flex().w_full().h(rems(42.)).items_start()
            .rounded_md().border_1().border_color(cx.theme().colors().border)
            .elevation_2(cx)
            .child(v_flex().w(rems(26.)).h_full().p_4().pb_2().justify_between()
                .border_r_1().border_color(cx.theme().colors().border)
                .child(v_flex().gap_2()
                    .child(Label::new("Machines").size(LabelSize::Large))
                    .child(div().w_full().p_2().rounded_sm().border_1()
                        .border_color(cx.theme().colors().border)
                        .child(self.machine_search.clone()))
                    .children(machines.into_iter().enumerate().map(|(position, index)| {
                        let machine = &self.store.machines[index];
                        ui::ListItem::new(format!("machine-{index}"))
                            .toggle_state(matches!(self.stage, PickerStage::Browse | PickerStage::Directories) && self.selected_machine == position)
                            .height(px(56.))
                            .inset(true)
                            .child(v_flex().child(Label::new(machine.name.clone())).child(Label::new(machine.address.clone()).color(Color::Muted)))
                            .on_click(cx.listener(move |this, _, window, cx| this.select_machine(position, window, cx)))
                    })))
                .child(ui::ListItem::new("add-machine")
                    .toggle_state(self.stage == PickerStage::Add || self.stage == PickerStage::Browse && self.selected_machine == machine_count && !self.focused_projects)
                    .height(px(44.))
                    .inset(true).child(Label::new("+ Add Machine"))
                    .on_click(cx.listener(|this, _, window, cx| this.add_machine(window, cx))))
            )
            .child(v_flex().flex_1().h_full().p_4().pb_2().justify_between()
                .when(self.stage == PickerStage::Directories, |this| {
                    this.when_some(self.directory_browser.clone(), |this, browser| this.child(browser))
                })
                .when(self.stage != PickerStage::Directories, |this| this
                .child(v_flex().gap_3()
                .when(self.stage == PickerStage::Browse, |this| {
                    this.child(Label::new(machine_name.map(|name| format!("Projects on {name}")).unwrap_or_else(|| "Select a Machine".into())))
                        .when(self.current_machine(cx).is_some(), |this| this.child(
                            div().w_full().p_2().rounded_sm().border_1()
                                .border_color(cx.theme().colors().border)
                                .child(self.project_search.clone())))
                        .children(projects.into_iter().enumerate().map(|(position, index)| {
                            let target = &self.targets[index];
                            let name = Path::new(&target.folder).file_name().and_then(|name| name.to_str()).unwrap_or(&target.folder).to_owned();
                            ui::ListItem::new(format!("machine-project-{index}"))
                                .toggle_state(self.focused_projects && self.selected_project == position)
                                .height(px(56.))
                                .inset(true)
                                .child(h_flex().w_full().items_center().justify_between()
                                    .child(v_flex().child(Label::new(name)).child(Label::new(target.folder.clone()).color(Color::Muted)))
                                    .when(self.opening_target.as_ref() == Some(target), |this| this.child(
                                        Icon::new(IconName::LoadCircle).size(IconSize::Small)
                                            .with_keyed_rotate_animation("opening-ssh-project", 1)
                                    )))
                                .on_click(cx.listener(move |this, _, _, cx| this.open_project(position, cx)))
                        }))
                        .when(self.current_machine(cx).is_some(), |this| {
                            this.child(ui::ListItem::new("browse-machine-folders")
                                .toggle_state(self.focused_projects && self.selected_project == project_count)
                                .height(px(44.))
                                .inset(true).child(Label::new("Browse Directories…"))
                                .on_click(cx.listener(|this, _, window, cx| this.browse(window, cx))))
                        })
                })
                .when(self.stage == PickerStage::Add, |this| {
                    this.child(Label::new(if self.editing_index.is_some() { "Edit Machine" } else { "Add Machine" }))
                        .child(Label::new("Give this SSH destination a name you recognize.").color(Color::Muted))
                        .child(Label::new("Name"))
                        .child(div().w_full().p_2().rounded_sm().border_1()
                            .border_color(cx.theme().colors().border)
                            .child(self.machine_name.clone()))
                        .child(Label::new("SSH address"))
                        .child(div().w_full().p_2().rounded_sm().border_1()
                            .border_color(cx.theme().colors().border)
                            .child(self.machine_address.clone()))
                })
                .when(self.stage == PickerStage::Verify, |this| {
                    this.child(Label::new("Verify Machine"))
                        .child(Label::new("Compare this SSH fingerprint with the one on the machine before trusting it.").color(Color::Muted))
                        .when(self.identity.as_ref().is_some_and(|identity| identity.changed), |this| {
                            this.child(Label::new("The trusted fingerprint for this address has changed.").color(Color::Error))
                        })
                        .when_some(self.identity.as_ref(), |this, identity| {
                            this.child(Label::new(identity.fingerprint.clone()))
                        })
                        .child(Label::new("Run this on the machine and compare the SHA256 value:").color(Color::Muted))
                        .child(Label::new(self.verification_command()))
                })
                .when(self.stage == PickerStage::Authorize, |this| {
                    this.child(Label::new("Allow This iPad"))
                        .when(!self.busy, |this| this.child(Label::new("Run the setup command on the machine as the SSH user, then check access.").color(Color::Muted)))
                })
                .when(self.stage == PickerStage::ConfirmRemove, |this| {
                    this.child(Label::new("Remove Machine?"))
                        .child(Label::new("This removes the saved Machine. It does not delete any remote files.").color(Color::Muted))
                })
                .when_some(self.error.clone(), |this, error| this.child(Label::new(error).color(Color::Error))))
                .child(h_flex().relative().w_full().items_center().justify_end()
                    .child(v_flex().absolute().left_0().top_0().h_full().w(rems(30.)).justify_center()
                        .when(self.stage == PickerStage::Browse && self.current_machine(cx).is_some(), |this| this
                            .child(div().relative().w_full().h(px(44.))
                                .child(ui::ButtonLike::new("check-saved-machine")
                                    .size(ButtonSize::Large)
                                    .height(px(44.).into())
                                    .disabled(self.busy)
                                    .child(h_flex().gap(DynamicSpacing::Base04.rems(cx))
                                        .child(div().relative().size(px(16.))
                                            .when_some(self.connection_previous_state, |this, previous| this
                                                .child(connection_icon_layer(previous, self.connection_transition_generation, true)))
                                            .child(connection_icon_layer(self.connection_state, self.connection_transition_generation, false)))
                                        .child(Label::new(connection_label)))
                                    .on_click(cx.listener(|this, _, window, cx| this.check_saved_machine(window, cx))))
                                .when_some(transfer_fraction, |this, fraction| this
                                    .child(div().absolute().left_0().bottom_0().w(rems(18.)).h(px(3.))
                                        .bg(cx.theme().colors().border)
                                        .child(div().h_full().w(relative(fraction))
                                            .bg(cx.theme().colors().border_focused))))))
                        .when(self.stage == PickerStage::Authorize, |this| this
                            .when_some(server_progress, |this, progress| this.child(server_progress_indicator(progress, cx)))))
                    .child(h_flex().gap_2()
                    .when(self.stage == PickerStage::Browse && self.current_machine(cx).is_some(), |this| this
                        .child(ipad_picker_button("edit-selected-machine", "Edit Machine", window, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.edit_machine(&PickerEditMachine, window, cx))))
                        .child(ipad_picker_button("remove-selected-machine", "Remove Machine", window, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.prompt_remove_machine(&PickerRemoveMachine, window, cx)))))
                    .when(self.stage == PickerStage::Add, |this| this
                        .child(ipad_picker_button("inspect-machine", if self.busy { "Checking…" } else if self.editing_index.is_some() { "Save Changes" } else { "Check Identity" }, window, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.inspect_machine(window, cx)))))
                    .when(self.stage == PickerStage::Verify, |this| this
                        .child(ipad_picker_button("trust-machine", "Fingerprint Matches — Continue", window, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.trust_machine(window, cx)))))
                    .when(self.stage == PickerStage::Authorize, |this| this
                        .child(ipad_picker_button("copy-ssh-setup", "Copy Setup Command", window, cx)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.copy_setup_command(cx))))
                        .child(ipad_picker_button("check-ssh-access", if self.busy { "Working…" } else { "Check Access" }, window, cx)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.check_access(window, cx)))))
                    .when(self.stage == PickerStage::ConfirmRemove, |this| this
                        .child(ipad_picker_button("confirm-remove-machine", "Remove Machine", window, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.remove_machine(window, cx))))))))
            ))
    }
}

struct IpadRemoteDelegate {
    _cancel: oneshot::Sender<()>,
}

impl RemoteClientDelegate for IpadRemoteDelegate {
    fn ask_password(
        &self,
        _prompt: String,
        _tx: oneshot::Sender<askpass::EncryptedPassword>,
        _cx: &mut AsyncApp,
    ) {
        log::warn!("iPad SSH requires a paired key; password authentication is not configured");
    }

    fn get_download_url(
        &self,
        _platform: RemotePlatform,
        _channel: release_channel::ReleaseChannel,
        _version: Option<semver::Version>,
        _cx: &mut AsyncApp,
    ) -> Task<anyhow::Result<Option<String>>> {
        Task::ready(Ok(None))
    }

    fn download_server_binary_locally(
        &self,
        _platform: RemotePlatform,
        _channel: release_channel::ReleaseChannel,
        _version: Option<semver::Version>,
        _cx: &mut AsyncApp,
    ) -> Task<anyhow::Result<PathBuf>> {
        Task::ready(Err(anyhow::anyhow!(
            "install a matching Zed remote server on the SSH host"
        )))
    }

    fn set_status(&self, status: Option<&str>, _cx: &mut AsyncApp) {
        if let Some(status) = status {
            log::info!("iZed SSH: {status}");
        }
    }
}

fn register_builtin_languages(registry: &Arc<language::LanguageRegistry>) {
    registry.register_native_grammars(grammars::native_grammars());

    for name in grammars::builtin_language_names() {
        let config = grammars::load_config(&name);
        let manifest_name = match name.as_str() {
            "python" => Some(SharedString::new_static("pyproject.toml").into()),
            "rust" => Some(SharedString::new_static("Cargo.toml").into()),
            _ => None,
        };
        registry.register_language(
            config.name.clone(),
            config.grammar.clone(),
            config.matcher.clone(),
            config.hidden,
            manifest_name.clone(),
            Arc::new(move || {
                Ok(language::LoadedLanguage {
                    config: config.clone(),
                    queries: grammars::load_queries(&name),
                    context_provider: None,
                    toolchain_provider: None,
                    manifest_name: manifest_name.clone(),
                })
            }),
        );
    }
}

fn open_ssh_project(
    window: gpui::WindowHandle<MultiWorkspace>,
    app_state: Arc<AppState>,
    mut target: SshTarget,
    picker: Option<WeakEntity<MachinePicker>>,
    cx: &mut App,
) {
    target.address = target.address.trim().to_owned();
    target.folder = target.folder.trim().to_owned();
    // The directory picker includes a trailing slash, but Zed serializes the
    // worktree root without one. Both opens must use the same workspace key.
    let path = PathBuf::from(&target.folder).components().collect::<PathBuf>();
    target.folder = path.to_string_lossy().into_owned();
    let mut ssh_options = match target.connection_options() {
        Ok(options) => options,
        Err(error) => {
            log::error!("Invalid SSH target: {error:#}");
            return;
        }
    };
    if let Some(machine) = MachineStore::load()
        .machines
        .into_iter()
        .find(|machine| machine.address.trim() == target.address)
    {
        ssh_options.nickname = Some(machine.name);
    }
    let existing_workspace = window.update(cx, |multi, window, cx| {
        let paths = PathList::new(&[path.clone()]);
        let host = RemoteConnectionOptions::Ssh(ssh_options.clone());
        let Some(existing) = multi.workspace_for_paths(&paths, Some(&host), cx) else {
            return false;
        };
        multi.workspace().update(cx, |workspace, cx| {
            workspace.hide_modal(window, cx);
        });
        multi.activate(existing, window, cx);
        true
    }).unwrap_or(false);
    if existing_workspace {
        log::info!("Activated an already open SSH project without reconnecting");
        return;
    }
    let (cancel, cancel_rx) = oneshot::channel();
    let delegate: Arc<dyn RemoteClientDelegate> = Arc::new(IpadRemoteDelegate { _cancel: cancel });

    cx.spawn(async move |cx| {
        let started = Instant::now();
        let model_project = target.clone();
        let result: anyhow::Result<()> = async {
            remote::check_ssh_directory(
                ssh_options.clone(),
                path.to_string_lossy().into_owned(),
            ).await?;
            log::info!("iZed project open: directory checked in {} ms", started.elapsed().as_millis());
            delegate.set_status(Some("Preparing Zed remote server"), cx);
            remote::ensure_ssh_server(ssh_options.clone(), bundled_server_archives()?, None).await?;
            log::info!("iZed project open: server ready after {} ms", started.elapsed().as_millis());
            let options = RemoteConnectionOptions::Ssh(ssh_options);
            let connection = remote::connect(options, delegate.clone(), cx).await?;
            log::info!("iZed project open: SSH connected after {} ms", started.elapsed().as_millis());
            cx.update(|cx| {
                workspace::open_remote_project_with_new_connection(
                    window.clone(),
                    connection,
                    cancel_rx,
                    delegate,
                    app_state.clone(),
                    vec![path.clone()],
                    cx,
                )
            })
            .await?;
            log::info!("iZed project open: workspace restored after {} ms", started.elapsed().as_millis());

            if let Err(error) = target.save() {
                log::warn!("Could not save SSH project: {error:#}");
            }

            window.update(cx, |multi, window, cx| {
                let workspace = multi.workspace().clone();
                let status = if let Some(client) = workspace.read(cx).project().read(cx).remote_client() {
                    let status = cx.new(|cx| SshConnectionStatus::new(client, cx));
                    let status_bar = workspace.read(cx).status_bar().clone();
                    status_bar.update(cx, |bar, cx| bar.add_left_item(status.clone(), window, cx));
                    Some(status)
                } else { None };
                if let Some(trusted) = TrustedWorktrees::try_get_global(cx) {
                    let worktree_store = workspace.read(cx).project().read(cx).worktree_store();
                    trusted.update(cx, |trusted, cx| {
                        trusted.trust(
                            &worktree_store,
                            collections::HashSet::from_iter([PathTrust::AbsPath(path.clone())]),
                            cx,
                        );
                    });
                }
                cx.spawn_in(window, async move |_, cx| {
                    let ui_result: anyhow::Result<()> = async {
                        let panel =
                            project_panel::ProjectPanel::load(workspace.downgrade(), cx.clone())
                                .await?;
                        workspace.update_in(cx, |workspace, window, cx| {
                            workspace.add_panel(panel, window, cx);
                        })?;
                        let git_panel = git_ui::git_panel::GitPanel::load(
                            workspace.downgrade(),
                            cx.clone(),
                        )
                        .await?;
                        workspace.update_in(cx, |workspace, window, cx| {
                            workspace.add_panel(git_panel, window, cx);
                        })?;
                        let terminal_panel = terminal_view::terminal_panel::TerminalPanel::load(
                            workspace.downgrade(),
                            cx.clone(),
                        )
                        .await?;
                        workspace.update_in(cx, |workspace, window, cx| {
                            workspace.add_panel(terminal_panel, window, cx);
                        })?;
                        let debug_panel = debugger_ui::debugger_panel::DebugPanel::load(
                            workspace.downgrade(),
                            cx,
                        )
                        .await?;
                        workspace.update_in(cx, |workspace, window, cx| {
                            workspace.add_panel(debug_panel, window, cx);
                        })?;
                        let agent_panel = agent_ui::AgentPanel::load(
                            workspace.downgrade(),
                            cx.clone(),
                        )
                        .await?;
                        workspace.update_in(cx, |workspace, window, cx| {
                            workspace.add_panel(agent_panel.clone(), window, cx);
                            workspace.register_action(agent_ui::AgentPanel::toggle_focus);
                            workspace.register_action(agent_ui::AgentPanel::focus);
                            workspace.register_action(agent_ui::AgentPanel::toggle);
                        })?;
                        if let Some(status) = status {
                            workspace.update_in(cx, |_, _, cx| {
                                let memory = cx.new(|cx| ProjectModelMemory::new(model_project, agent_panel, cx));
                                memory.update(cx, |memory, cx| memory.refresh(cx));
                                status.update(cx, |status, _| status._model_memory = Some(memory));
                            })?;
                        }
                        workspace.update_in(cx, |workspace, window, cx| {
                            if workspace.active_item(cx).is_some()
                                || !workspace.left_dock().read(cx).is_open()
                            {
                                workspace.focus_center_pane(window, cx);
                            } else {
                                workspace.focus_panel::<project_panel::ProjectPanel>(window, cx);
                            }
                        })?;
                        ized_platform::show_keyboard();
                        Ok(())
                    }
                    .await;
                    if let Err(error) = ui_result {
                        log::error!("Could not show SSH workspace: {error:#}");
                    }
                })
                .detach();
            })?;
            log::info!("Opened Zed SSH workspace");
            Ok(())
        }
        .await;
        if let Err(error) = result {
            log::error!("Could not open Zed SSH workspace: {error:#}");
            let message = error.to_string();
            let picker_is_visible = window.update(cx, |multi, _, cx| {
                multi.workspace().read(cx).active_modal::<MachinePicker>(cx).is_some()
            }).unwrap_or(false);
            if let Some(picker) = picker.filter(|_| picker_is_visible) {
                if picker.update(cx, |picker, cx| {
                    picker.opening_target = None;
                    picker.error = Some(message.clone().into());
                    if let Some(browser) = &picker.directory_browser {
                        browser.update(cx, |browser, cx| {
                            browser.opening_folder = None;
                            browser.error = Some(message.clone().into());
                            cx.notify();
                        });
                    }
                    cx.notify();
                }).is_ok() {
                    return;
                }
            }
            let target_window = window.clone();
            let _ = window.update(cx, |multi, window, cx| {
                multi.workspace().update(cx, |workspace, cx| {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        let mut modal =
                            SshTargetModal::new(target_window, app_state.clone(), Some(target.clone()), window, cx);
                        modal.error = Some(message.into());
                        modal
                    });
                });
            });
        }
    })
    .detach();
}

fn ensure_preview_theme_in_settings() {
    let path = paths::settings_file();
    let theme = serde_json::json!({
        "mode": "system",
        "light": "Vercel Light",
        "dark": "Vercel Dark"
    });
    match std::fs::read_to_string(path) {
        Ok(mut text) => {
            let (Some(mut settings), _) = settings::parse_json::<serde_json::Value>(&text) else {
                log::warn!("Could not parse iPad settings to preserve the preview theme");
                return;
            };
            let current_theme = settings.get("theme");
            let uses_zed_default = current_theme.is_none()
                || current_theme.is_some_and(|current| {
                    current.get("light").and_then(|value| value.as_str()) == Some("One Light")
                        && current.get("dark").and_then(|value| value.as_str()) == Some("One Dark")
                });
            if !uses_zed_default {
                return;
            }
            let original = settings.clone();
            settings["theme"] = theme;
            settings::update_value_in_json_text(
                &mut text,
                &mut Vec::new(),
                2,
                &original,
                &settings,
                &mut Vec::new(),
            );
            if let Err(error) = std::fs::write(path, text) {
                log::error!("Could not save the iPad preview theme: {error}");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(error) = std::fs::write(path, serde_json::json!({"theme": theme}).to_string()) {
                log::error!("Could not initialize iPad settings: {error}");
            }
        }
        Err(error) => log::error!("Could not read iPad settings: {error}"),
    }
}

pub fn open(cx: &mut App) {
    // Zed's desktop data paths resolve under hidden home directories, which
    // iPadOS does not let this app create. Keep its database and agent threads
    // together in the app's writable Documents container.
    let zed_data_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Documents")
        .join("Zed");
    paths::set_custom_data_dir(zed_data_dir.to_string_lossy().as_ref());
    if let Err(error) = std::fs::create_dir_all(paths::config_dir()) {
        log::error!("Could not create iPad Zed config directory: {error}");
    }
    ensure_preview_theme_in_settings();
    if let Err(error) = cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../fonts/JetBrainsMonoNerdFont-Light.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/JetBrainsMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../fonts/JetBrainsMono-Medium.ttf")),
    ]) {
        log::error!("Could not load iPad workspace fonts: {error}");
    }

    cx.set_global(db::AppDatabase::new());
    let trusted_paths = workspace::WorkspaceDb::global(cx)
        .fetch_trusted_worktrees()
        .unwrap_or_default();
    trusted_worktrees::init(trusted_paths, cx);
    settings::init(cx);
    settings::SettingsStore::update_global(cx, |store, cx| {
        let _ = store.set_global_settings(
            r#"{"vim_mode":true,"relative_line_numbers":"enabled","load_direnv":"disabled","buffer_font_family":"JetBrainsMono Nerd Font","buffer_font_weight":300,"buffer_font_size":15,"ui_font_family":"JetBrains Mono","ui_font_weight":500,"ui_font_size":16,"theme":{"mode":"system","light":"Vercel Light","dark":"Vercel Dark"}}"#,
            cx,
        );
    });
    theme_settings::init(theme::LoadThemes::JustBase, cx);
    if let Err(error) = theme_settings::load_user_theme(
        &theme::ThemeRegistry::global(cx),
        include_bytes!("../themes/vercel-theme.json"),
    ) {
        log::error!("Could not load bundled Vercel theme: {error:#}");
    } else {
        theme_settings::reload_theme(cx);
    }
    release_channel::init(semver::Version::new(0, 233, 0), cx);
    gpui_tokio::init(cx);

    let http_client: reqwest_client::ReqwestClient = reqwest::Client::new().into();
    cx.set_http_client(Arc::new(http_client));

    let fs: Arc<dyn fs::Fs> = Arc::new(fs::RealFs::new(None, cx.background_executor().clone()));
    <dyn fs::Fs>::set_global(fs.clone(), cx);

    let client = client::Client::production(cx);
    client::Client::set_global(client.clone(), cx);
    project::Project::init(&client, cx);
    dap_adapters::init(cx);
    debugger_ui::init(cx);
    client::init(&client, cx);

    let languages = Arc::new(language::LanguageRegistry::new(
        cx.background_executor().clone(),
    ));
    register_builtin_languages(&languages);
    languages.set_theme(cx.theme().clone());
    cx.observe_global::<GlobalTheme>({
        let languages = languages.clone();
        move |cx| languages.set_theme(cx.theme().clone())
    })
    .detach();
    let user_store = cx.new(|cx| client::UserStore::new(client.clone(), cx));
    let workspace_store = cx.new(|cx| workspace::WorkspaceStore::new(client.clone(), cx));
    let session = cx.foreground_executor().block_on(session::Session::new(
        "ized-workspace".into(),
        db::kvp::KeyValueStore::global(cx),
    ));
    let session = cx.new(|cx| session::AppSession::new(session, cx));
    let app_state = Arc::new(AppState {
        languages,
        client,
        user_store,
        workspace_store,
        fs,
        build_window_options: |_, _| WindowOptions::default(),
        node_runtime: node_runtime::NodeRuntime::unavailable(),
        session,
    });
    AppState::set_global(app_state.clone(), cx);

    workspace::init(app_state.clone(), cx);
    search::init(cx);
    cx.set_global(workspace::PaneSearchBarCallbacks {
        setup_search_bar: |languages, toolbar, window, cx| {
            let search_bar = cx.new(|cx| search::BufferSearchBar::new(languages, window, cx));
            toolbar.update(cx, |toolbar, cx| toolbar.add_item(search_bar, window, cx));
        },
        wrap_div_with_search_actions: search::buffer_search::register_pane_search_actions,
    });
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        add_git_diff_toolbar(workspace, workspace.active_pane(), window, cx);
        let workspace_handle = cx.entity();
        cx.subscribe_in(&workspace_handle, window, |workspace, _, event, window, cx| {
            if let workspace::Event::PaneAdded(pane) = event {
                add_git_diff_toolbar(workspace, pane, window, cx);
            }
        }).detach();
    }).detach();
    // The macOS keymap binds Command-Plus/Minus/Zero to these actions. Zed's
    // desktop app registers their handlers in zed.rs, which the iPad host does
    // not run, so install the in-memory editor zoom behavior here.
    cx.on_action(|_: &zed_actions::IncreaseBufferFontSize, cx| {
        theme_settings::increase_buffer_font_size(cx);
    });
    cx.on_action(|_: &zed_actions::DecreaseBufferFontSize, cx| {
        theme_settings::decrease_buffer_font_size(cx);
    });
    cx.on_action(|_: &zed_actions::ResetBufferFontSize, cx| {
        theme_settings::reset_buffer_font_size(cx);
    });
    language_model::init(cx);
    client::RefreshLlmTokenListener::register(app_state.client.clone(), app_state.user_store.clone(), cx);
    language_models::init(app_state.user_store.clone(), app_state.client.clone(), cx);
    let prompt_builder = prompt_store::PromptBuilder::load(app_state.fs.clone(), false, cx);
    project::AgentRegistryStore::init_global(cx, app_state.fs.clone(), app_state.client.http_client());
    agent_ui::init(app_state.fs.clone(), prompt_builder, app_state.languages.clone(), false, false, cx);
    let remote_action_state = app_state.clone();
    let remote_action_window: Rc<RefCell<Option<WindowHandle<MultiWorkspace>>>> =
        Rc::new(RefCell::new(None));
    let registered_window = remote_action_window.clone();
    cx.on_action(move |_: &zed_actions::OpenRemote, cx| {
        let Some(window_handle) = registered_window.borrow().clone() else {
            log::warn!("Open Remote: iZed window is not ready");
            return;
        };
        let target_state = remote_action_state.clone();
        cx.defer(move |cx| {
            if let Err(error) = window_handle.update(cx, |multi, window, cx| {
                let workspace = multi.workspace().clone();
                let target_window = window_handle.clone();
                workspace.update(cx, |workspace, cx| {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        MachinePicker::new(target_window, target_state, window, cx)
                    });
                });
            }) {
                log::error!("Could not show Open Remote dialog: {error}");
            } else {
                ized_platform::show_keyboard();
            }
        });
    });
    let recent_action_state = app_state.clone();
    let recent_action_window = remote_action_window.clone();
    cx.on_action(move |_: &zed_actions::OpenRecent, cx| {
        let Some(window_handle) = recent_action_window.borrow().clone() else {
            return;
        };
        let app_state = recent_action_state.clone();
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, |multi, window, cx| {
                multi.workspace().update(cx, |workspace, cx| {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        MachinePicker::new(window_handle.clone(), app_state, window, cx)
                    });
                });
            });
            ized_platform::show_keyboard();
        });
    });
    editor::init(cx);
    git_ui::init(cx);
    diagnostics::init(cx);
    file_finder::init(cx);
    command_palette::init(cx);
    command_palette_hooks::CommandPaletteFilter::update_global(cx, |filter, _| {
        filter.show_only_action_names([
            "file_finder::Toggle",
            "workspace::NewFile",
            "workspace::Save",
            "workspace::SaveAll",
            "workspace::CopyPath",
            "pane::CloseActiveItem",
            "pane::CloseOtherItems",
            "pane::ReopenClosedItem",
            "project_panel::ToggleFocus",
            "pane::ActivateNextItem",
            "pane::ActivatePreviousItem",
            "pane::GoBack",
            "pane::GoForward",
            "editor::Undo",
            "editor::Redo",
            "buffer_search::Deploy",
            "editor::SelectAll",
            "editor::DuplicateLineDown",
            "editor::ToggleComments",
            "editor::ToggleSoftWrap",
            "editor::ToggleLineNumbers",
            "editor::Format",
            "editor::OrganizeImports",
            "editor::ShowCompletions",
            "editor::ShowSignatureHelp",
            "editor::Hover",
            "editor::ToggleCodeActions",
            "editor::GoToDefinition",
            "editor::GoToTypeDefinition",
            "editor::GoToImplementation",
            "editor::FindAllReferences",
            "editor::Rename",
            "editor::GoToDiagnostic",
            "editor::GoToPreviousDiagnostic",
            "workspace::ToggleVimMode",
            "project_panel::Rename",
            "project_panel::Duplicate",
            "project_panel::Delete",
            "project_panel::CollapseAllEntries",
            "pane::SplitVertical",
            "pane::SplitHorizontal",
            "workspace::ActivateNextPane",
            "workspace::ActivatePreviousPane",
            "project_panel::Toggle",
            "git_panel::Toggle",
            "git_panel::ToggleFocus",
            "git::Diff",
            "git::BranchDiff",
            "git::OpenModifiedFiles",
            "terminal_panel::Toggle",
            "terminal_panel::ToggleFocus",
            "workspace::NewTerminal",
            "debug_panel::ToggleFocus",
            "agent::ToggleFocus",
            "agent::NewThread",
            "debugger::Start",
            "debugger::Continue",
            "debugger::Pause",
            "debugger::StepOver",
            "debugger::StepInto",
            "debugger::StepOut",
            "debugger::Stop",
            "editor::ToggleBreakpoint",
            "projects::OpenRemote",
        ]);
    });
    project_panel::init(cx);
    terminal_view::init(cx);
    tasks_ui::init(cx);
    vim::init(cx);
    // iPad hardware keyboards use Command for app shortcuts, as on macOS.
    // settings::DEFAULT_KEYMAP_PATH falls back to Linux on iOS.
    for path in ["keymaps/default-macos.json", settings::VIM_KEYMAP_PATH] {
        match settings::KeymapFile::load_asset_allow_partial_failure(path, cx) {
            Ok(bindings) => cx.bind_keys(bindings),
            Err(error) => log::warn!("Could not load {path}: {error}"),
        }
    }
    match settings::KeymapFile::load(include_str!("../keymaps/ized-macos.jsonc"), cx) {
        settings::KeymapFileLoadResult::Success { key_bindings } => cx.bind_keys(key_bindings),
        settings::KeymapFileLoadResult::SomeFailedToLoad {
            key_bindings,
            error_message,
        } => {
            log::warn!("Could not load some iPad key bindings: {error_message}");
            cx.bind_keys(key_bindings);
        }
        settings::KeymapFileLoadResult::JsonParseFailure { error } => {
            log::error!("Could not parse iPad keymap: {error}");
        }
    }
    cx.bind_keys([
        KeyBinding::new("ctrl-n", NextSuggestion, Some("SshTargetModal > Editor")),
        KeyBinding::new("ctrl-p", PreviousSuggestion, Some("SshTargetModal > Editor")),
        KeyBinding::new("down", NextSuggestion, Some("SshTargetModal > Editor")),
        KeyBinding::new("up", PreviousSuggestion, Some("SshTargetModal > Editor")),
        KeyBinding::new("tab", AcceptSuggestion, Some("SshTargetModal > Editor")),
        KeyBinding::new("enter", SelectDirectory, Some("SshTargetModal > Editor")),
        KeyBinding::new("ctrl-n", PickerNext, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("ctrl-p", PickerPrevious, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("up", PickerPrevious, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("down", PickerNext, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("enter", PickerAccept, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("escape", PickerBack, Some("MachinePicker")),
        KeyBinding::new("tab", PickerSwitchColumn, Some("MachinePicker > (Editor && !SshTargetModal)")),
        KeyBinding::new("cmd-e", PickerEditMachine, Some("MachinePicker")),
        KeyBinding::new("cmd-backspace", PickerRemoveMachine, Some("MachinePicker")),
    ]);

    let project = project::Project::local(
        app_state.client.clone(),
        app_state.node_runtime.clone(),
        app_state.user_store.clone(),
        app_state.languages.clone(),
        app_state.fs.clone(),
        None,
        Default::default(),
        cx,
    );
    let recent_state = app_state.clone();
    match cx.open_window(WindowOptions::default(), move |window, cx| {
        let workspace = cx.new(|cx| Workspace::new(None, project, app_state, window, cx));
        let initial_pane = workspace.read(cx).active_pane().clone();
        initial_pane.update(cx, |pane, cx| {
            pane.set_should_display_welcome_page(false);
            cx.notify();
        });
        cx.new(|cx| MultiWorkspace::new(workspace, window, cx))
    }) {
        Ok(window) => {
            log::info!("Opened Zed recent projects");
            *remote_action_window.borrow_mut() = Some(window.clone());
            let sidebar_window = window.clone();
            if let Ok(multi_workspace) = window.entity(cx) {
                cx.defer(move |cx| {
                    if let Ok(sidebar) = cx.update_window(sidebar_window.into(), |_, window, cx| {
                        cx.new(|cx| sidebar::Sidebar::new(multi_workspace.clone(), window, cx))
                    }) {
                        multi_workspace.update(cx, |multi, cx| multi.register_sidebar(sidebar, cx));
                    }
                });
            }
            let target_window = window.clone();
            if let Err(error) = window.update(cx, |multi, window, cx| {
                multi.workspace().update(cx, |workspace, cx| {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        MachinePicker::new(target_window, recent_state, window, cx)
                    });
                });
            }) {
                log::error!("Could not show recent SSH projects: {error}");
            } else {
                ized_platform::show_keyboard();
            }
        }
        Err(error) => log::error!("Could not open Zed workspace: {error}"),
    }
}
