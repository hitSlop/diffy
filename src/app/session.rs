use super::Diffy;
use super::review::ReviewView;
use crate::demo;
use crate::fonts;
use crate::keyboard;
use crate::repository::ChangeGroup;
use crate::repository::GroupSnapshot;
use crate::state;
use crate::updates;
use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::PathPromptOptions;
use gpui_kit::SharedString;
use gpui_kit::Subscription;
use gpui_kit::Task;
use gpui_kit::Window;
use gpui_kit::component::Theme;
use gpui_kit::component::ThemeMode;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::scroll::ScrollbarMode;
use gpui_kit::px;
use gpui_kit::rgb;
use std::path::PathBuf;
use std::time::Duration;

pub(super) struct Project {
    pub(super) repository: Option<PathBuf>,
    pub(super) topology_task: Option<Task<()>>,
    pub(super) pending_topology: bool,
    pub(super) available: bool,
    pub(super) main_worktree: bool,
    pub(super) locked_worktree: bool,
    pub(super) detached_worktree: bool,
    pub(super) metadata_context: Option<crate::repository::MetadataContext>,
    pub(super) id: u64,
    pub(super) watcher: Option<notify::RecommendedWatcher>,
    pub(super) watch_task: Option<Task<()>>,
    pub(super) pending_refresh: bool,
    pub(super) subscriptions: Vec<Subscription>,
    pub(super) name: SharedString,
    pub(super) custom_name: Option<String>,
    pub(super) icon: Option<state::ProjectIcon>,
    pub(super) color: u32,
    pub(super) changed_count: usize,
    pub(super) path: Option<PathBuf>,
    pub(super) branch: SharedString,
    pub(super) views: [ReviewView; 2],
    pub(super) group: ChangeGroup,
    /// User-visible busy state: the first read, or a stage/unstage. Toolbar
    /// buttons follow this and dim while it is set.
    pub(super) loading: bool,
    /// Reload of a project that already has a snapshot. The toolbar stays put;
    /// `is_busy` still waits until this generation finishes.
    pub(super) refreshing: bool,
    pub(super) mutating: bool,
    pub(super) error: Option<String>,
    pub(super) generation: u64,
    pub(super) task: Option<Task<()>>,
}

impl Project {
    pub(super) fn update_count(&mut self) {
        self.changed_count = self
            .views
            .iter()
            .flat_map(|view| view.files.iter().map(|file| file.path()))
            .collect::<std::collections::HashSet<_>>()
            .len();
    }

    pub(super) fn view(&self) -> &ReviewView {
        &self.views[self.group.index()]
    }
    pub(super) fn view_mut(&mut self) -> &mut ReviewView {
        &mut self.views[self.group.index()]
    }
}

impl Diffy {
    pub fn new(paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_options(paths, false, state::default_path(), window, cx)
    }
    pub fn demo(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_options(Vec::new(), true, None, window, cx)
    }
    pub fn with_options(
        paths: Vec<PathBuf>,
        demo: bool,
        state_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.set_window_title("Diffy");
        Theme::change(ThemeMode::Dark, Some(window), cx);
        Theme::update(cx, |theme| {
            let background = rgb(0x000000).into();
            let rail = rgb(0x000000).into();
            let border = rgb(0x000000).into();
            theme.background = background;
            theme.sidebar = rail;
            theme.title_bar = background;
            theme.title_bar_border = border;
            theme.status_bar = background;
            theme.status_bar_border = background;
            theme.border = border;
            theme.sidebar_border = border;
            theme.tab_bar = background;
            theme.tab = background;
            theme.tab_active = background;
            theme.foreground = rgb(0xe7e7e8).into();
            theme.muted_foreground = rgb(0x8d929d).into();
            theme.primary = rgb(0xf2d45c).into();
            theme.primary_foreground = rgb(0x151510).into();
            theme.colors.list = rail;
            theme.list_active = rgb(0x1b1d21).into();
            theme.list_active_border = rgb(0x1b1d21).into();
            theme.list_hover = rgb(0x14161a).into();
            theme.secondary = rgb(0x181a1e).into();
            theme.secondary_hover = rgb(0x24262b).into();
            theme.success = rgb(0x65d5a2).into();
            theme.danger = rgb(0xf1757d).into();
            theme.muted = background;
            theme.scrollbar_mode = ScrollbarMode::Always;
            theme.font_family = fonts::FAMILY.into();
            theme.font_size = px(13.);
            theme.mono_font_family = fonts::FAMILY.into();
            theme.mono_font_size = px(13.);
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            window_handle: window.window_handle(),
            import: Default::default(),
            github: Default::default(),
            projects: Vec::new(),
            repositories: Vec::new(),
            topology_sequence: 0,
            active: 0,
            focus,
            next_id: 1,
            state_path: if demo { None } else { state_path },
            state_error: None,
            state_restore_failed: false,
            restoring: true,
            sidebar_width: 300.,
            command: cx.new(|cx| CommandState::new(window, cx)),
            palette_open: false,
            previous_focus: None,
            hovered_project: None,
            settings: None,
            project_apps: Default::default(),
            repeat_direction: None,
            file_repeat: None,
            native_repeat_at: None,
            native_has_repeated: false,
            file_key_probe: keyboard::file_shortcut_pressed,
            file_scroll_remainder: 0.,
            file_scroll_axis: None,
            file_scroll_gesture: Default::default(),
            navigation_save: None,
            persisted_state: None,
            preview_cache: Default::default(),
            state_writer: None,
            state_save_task: None,
            closing: false,
            quit_subscription: None,
            reader: Default::default(),
            preview_requests: 0,
            updates: updates::Controller::default(),
            update_task: None,
            snake: cx.new(super::snake::SnakeGame::new),
            snake_tab: None,
            snake_took_focus: false,
            _keyboard_subscription: {
                let owner = cx.entity().downgrade();
                let window_id = window.window_handle().window_id();
                cx.intercept_keystrokes(move |event, w, cx| {
                    if w.window_handle().window_id() != window_id {
                        return;
                    }
                    let _ = owner.update(cx, |this, cx| {
                        if this.import.picker.is_some() {
                            return;
                        }
                        if this.settings.is_none() {
                            let mods = event.keystroke.modifiers;
                            if !this.palette_open && mods.secondary() && !mods.shift && !mods.alt {
                                let direction = match event.keystroke.key.as_str() {
                                    "up" => Some(-1),
                                    "down" => Some(1),
                                    _ => None,
                                };
                                if let Some(direction) = direction
                                    && (cfg!(target_os = "macos")
                                        || (this.file_key_probe)(direction))
                                {
                                    cx.stop_propagation();
                                    this.start_file_repeat(direction, w, cx);
                                }
                            }
                            return;
                        }
                        let key = event.keystroke.key.as_str();
                        if key == "escape" {
                            cx.stop_propagation();
                            if let Some(draft) = &this.settings
                                && draft.color.read(cx).is_open()
                            {
                                draft.color.update(cx, |s, cx| s.set_open(false, cx));
                            } else {
                                this.close_settings(w, cx);
                            }
                        } else if event.keystroke.modifiers.secondary() {
                            if matches!(key, "s" | "enter") {
                                cx.stop_propagation();
                                this.save_settings(w, cx);
                            } else if matches!(key, "up" | "down" | "p" | "o" | "r") {
                                cx.stop_propagation();
                            }
                        }
                    });
                })
            },
        };
        if demo {
            for sample in demo::projects() {
                this.add_demo(sample, cx);
            }
        } else {
            let saved = this
                .state_path
                .as_ref()
                .map(|p| state::load_for_restore(p))
                .transpose();
            match saved {
                Ok(Some(saved)) => {
                    this.repositories = saved
                        .repositories
                        .into_iter()
                        .map(super::worktrees::RepositoryProject::restore)
                        .collect();
                    this.reader = saved.reader;
                    this.reader.normalize();
                    Theme::update(cx, |theme| theme.mono_font_size = px(this.reader.font_size));
                    this.import.preferences = saved.discovery;
                    this.sidebar_width = saved
                        .sidebar_width
                        .filter(|v| v.is_finite())
                        .unwrap_or(300.)
                        .clamp(240., 460.);
                    for entry in saved.projects {
                        this.add_path(entry.path, cx);
                        let p = &mut this.projects[this.active];
                        p.color = entry.color;
                        p.custom_name = entry.name;
                        if let Some(name) = &p.custom_name {
                            p.name = name.clone().into();
                        }
                        p.icon = entry.icon;
                        p.group = if entry.staged {
                            ChangeGroup::Staged
                        } else {
                            ChangeGroup::Changes
                        };
                        for (ix, selected) in entry.selected.into_iter().enumerate() {
                            p.views[ix].selected = selected.map(Into::into);
                            p.views[ix].diff.update(cx, |d, cx| {
                                d.set_mode(
                                    if entry.split {
                                        DiffMode::Split
                                    } else {
                                        DiffMode::Unified
                                    },
                                    cx,
                                )
                            });
                        }
                    }
                    this.active = saved
                        .active
                        .and_then(|path| {
                            this.projects
                                .iter()
                                .position(|p| p.path.as_ref() == Some(&path))
                        })
                        .unwrap_or(0);
                }
                Err(e) => {
                    this.state_restore_failed = true;
                    this.state_error = Some(format!(
                        "{e}. Preferences were preserved; settings will not be saved in this session."
                    ));
                }
                _ => {}
            }
            for path in paths {
                this.add_path(path, cx);
            }
        }
        this.restoring = false;
        if !demo {
            this.detect_project_apps(cx);
            this.project_apps.activation =
                Some(cx.observe_window_activation(window, |this, window, cx| {
                    if window.is_window_active() {
                        this.detect_project_apps(cx);
                        this.refresh_topologies(cx);
                    }
                }));
        }
        this.persist(cx);
        this.quit_subscription = Some(cx.on_app_quit(|this, cx| {
            let state = this.saved_state(cx);
            if let Some(writer) = &this.state_writer {
                writer.enqueue(state);
            }
            let flush = this.state_writer.as_ref().map(state::Writer::flush_handle);
            cx.background_executor().spawn(async move {
                if let Err(error) = flush.map_or(Ok(()), state::Flush::wait) {
                    eprintln!("{error}");
                }
            })
        }));
        if !demo && this.state_path.is_some() && !this.state_restore_failed {
            cx.defer_in(window, |this, window, cx| {
                if !this.import.preferences.onboarding_completed {
                    this.open_import(window, cx);
                }
            });
        }
        let owner = cx.entity().downgrade();
        let closing_window = window.window_handle();
        window.on_window_should_close(cx, move |_, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.updates.blocks_mutations() {
                        return false;
                    }
                    this.exit(Some(closing_window), cx);
                    false
                })
                .unwrap_or(true)
        });
        if !demo
            && std::env::current_exe()
                .ok()
                .and_then(|p| updates::bundle_path(&p))
                .is_some()
        {
            this.update_task = Some(cx.spawn(async move |owner, cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let _ = owner.update(cx, |this, cx| this.check_updates(false, cx));
            }));
        }
        this
    }
    pub(super) fn push_project(&mut self, mut project: Project, cx: &mut Context<Self>) {
        project.update_count();
        // Zero means this project has not been given an id yet.
        if project.id == 0 {
            project.id = self.next_id;
            self.next_id += 1;
        }
        for view in &project.views {
            project
                .subscriptions
                .push(cx.observe(&view.diff, |_, _, cx| cx.notify()));
        }
        self.projects.push(project);
    }
    pub(super) fn add_demo(&mut self, demo: demo::Demo, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        let mut views = [ReviewView::new(cx), ReviewView::new(cx)];
        views[0].install(
            GroupSnapshot::parse(demo.staged).expect("valid demo"),
            id,
            cx,
        );
        views[1].install(
            GroupSnapshot::parse(demo.changes).expect("valid demo"),
            id,
            cx,
        );
        self.push_project(
            Project {
                repository: None,
                topology_task: None,
                pending_topology: false,
                available: true,
                main_worktree: true,
                locked_worktree: false,
                detached_worktree: false,
                metadata_context: None,
                watcher: None,
                watch_task: None,
                pending_refresh: false,
                id,
                subscriptions: Vec::new(),
                name: demo.name.into(),
                custom_name: None,
                icon: None,
                color: demo.color,
                changed_count: 0,
                path: None,
                branch: "Demo".into(),
                views,
                group: ChangeGroup::Changes,
                loading: false,
                refreshing: false,
                mutating: false,
                error: None,
                generation: 0,
                task: None,
            },
            cx,
        );
    }
    pub(super) fn add_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let repository = self
            .repositories
            .iter()
            .find(|r| r.checkouts.contains(&path))
            .map(|r| r.common_dir.clone());
        if let Some(common) = &repository {
            self.import.preferences.excluded_repositories.remove(common);
        }
        self.import.preferences.excluded.remove(&path);
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.path.as_ref() == Some(&path))
        {
            self.active = ix;
            self.remember_worktree();
            self.persist(cx);
            cx.notify();
            return;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Project".into());
        let views = [ReviewView::new(cx), ReviewView::new(cx)];
        let color = [0x74d9b5, 0xff8175, 0xb9a0e8, 0x82b6e5][self.projects.len() % 4];
        self.push_project(
            Project {
                repository,
                topology_task: None,
                pending_topology: false,
                available: true,
                main_worktree: false,
                locked_worktree: false,
                detached_worktree: false,
                metadata_context: None,
                watcher: None,
                watch_task: None,
                pending_refresh: false,
                id: 0,
                subscriptions: Vec::new(),
                name: name.into(),
                custom_name: None,
                icon: None,
                color,
                changed_count: 0,
                path: Some(path),
                branch: "…".into(),
                views,
                group: ChangeGroup::Changes,
                loading: false,
                refreshing: false,
                mutating: false,
                error: None,
                generation: 0,
                task: None,
            },
            cx,
        );
        self.active = self.projects.len() - 1;
        self.load(None, cx);
        self.persist(cx);
    }
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        for path in paths {
            self.add_path(path, cx);
        }
        self.focus.focus(window, cx);
        window.activate_window();
        cx.activate(true);
        cx.notify();
    }

    pub(super) fn install_cli(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match crate::cli::install_current() {
            Ok(report) => {
                let prompt = window.prompt(
                    gpui_kit::PromptLevel::Info,
                    "Terminal command installed",
                    Some(&report),
                    &["Copy PATH command", "Done"],
                    cx,
                );
                cx.spawn(async move |this, cx| {
                    if prompt.await.ok() == Some(0) {
                        let _ = this.update(cx, |_, cx| {
                            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
                                "export PATH=\"$HOME/.local/bin:$PATH\"".to_owned(),
                            ))
                        });
                    }
                })
                .detach();
            }
            Err(error) => {
                let prompt = window.prompt(
                    gpui_kit::PromptLevel::Critical,
                    "Couldn't install terminal command",
                    Some(&error),
                    &["OK"],
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = prompt.await;
                })
                .detach();
            }
        }
    }

    pub(super) fn open_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open Git repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = cx.update(|window, cx| {
                    this.update(cx, |this, cx| {
                        for path in paths {
                            this.add_path(path, cx);
                        }
                        this.focus.focus(window, cx);
                    })
                });
            }
        })
        .detach();
    }
    pub(super) fn switch_project(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ix < self.projects.len() {
            if !self.projects[ix].available {
                return;
            }
            self.stop_file_repeat();
            self.leave_github(cx);
            self.active = ix;
            self.remember_worktree();
            self.reveal_worktree();
            self.ensure_preview(cx);
            self.focus.focus(window, cx);
            self.persist(cx);
            cx.notify();
        }
    }
    pub(super) fn remove_project(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        if self.repository_mutating(ix) {
            return;
        }
        let active_id = self.active_project_id();
        let ids = self
            .worktree_indices(ix)
            .into_iter()
            .map(|ix| self.projects[ix].id)
            .collect::<Vec<_>>();
        if let Some(common) = self.projects[ix].repository.clone() {
            self.import
                .preferences
                .excluded_repositories
                .insert(common.clone());
            self.github.states.remove(&common);
            self.repositories.retain(|r| r.common_dir != common);
        }
        for p in &self.projects {
            if ids.contains(&p.id)
                && let Some(path) = &p.path
            {
                self.import.preferences.excluded.insert(path.clone());
            }
        }
        self.projects.retain(|p| !ids.contains(&p.id));
        self.active = active_id
            .and_then(|id| self.projects.iter().position(|p| p.id == id))
            .unwrap_or_else(|| ix.min(self.projects.len().saturating_sub(1)));
        self.hovered_project = None;
        self.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn move_project(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.projects.is_empty() {
            return;
        }
        let rows = self.project_rows();
        let current = rows
            .iter()
            .position(|&ix| self.same_repository(ix, self.active))
            .unwrap_or(0);
        let next = (current as isize + direction).rem_euclid(rows.len() as isize) as usize;
        self.switch_project(rows[next], window, cx);
    }
    pub(super) fn set_group(
        &mut self,
        group: ChangeGroup,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_pr() {
            return;
        }
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        p.group = group;
        self.ensure_preview(cx);
        self.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    fn saved_state(&self, cx: &App) -> state::SavedState {
        state::SavedState {
            repositories: self
                .repositories
                .iter()
                .map(super::worktrees::RepositoryProject::saved)
                .collect(),
            projects: self
                .projects
                .iter()
                .filter_map(|p| {
                    p.path.as_ref().map(|path| state::SavedProject {
                        path: path.clone(),
                        color: p.color,
                        name: p.custom_name.clone(),
                        icon: p.icon.clone(),
                        staged: p.group == ChangeGroup::Staged,
                        split: p.view().diff.read(cx).mode() == DiffMode::Split,
                        selected: std::array::from_fn(|ix| {
                            p.views[ix].selected.as_ref().map(ToString::to_string)
                        }),
                    })
                })
                .collect(),
            active: self.projects.get(self.active).and_then(|p| p.path.clone()),
            sidebar_width: Some(self.sidebar_width),
            discovery: self.import.preferences.clone(),
            reader: self.reader.clone(),
            ..Default::default()
        }
    }
    pub(super) fn persist(&mut self, cx: &mut Context<Self>) {
        if self.restoring || self.state_restore_failed {
            return;
        }
        let Some(path) = &self.state_path else {
            return;
        };
        let state = self.saved_state(cx);
        if self.persisted_state.as_ref() == Some(&state) && self.state_error.is_none() {
            return;
        }
        if self.state_writer.is_none() {
            match state::Writer::new(path.clone()) {
                Ok(writer) => {
                    let events = writer.events();
                    self.state_writer = Some(writer);
                    self.state_save_task = Some(cx.spawn(async move |owner, cx| {
                        while events.recv().await.is_ok() {
                            if owner
                                .update(cx, |this, cx| {
                                    this.state_error =
                                        this.state_writer.as_ref().and_then(|w| w.result().err());
                                    if this.state_error.is_some() {
                                        this.persisted_state = None;
                                    }
                                    cx.notify();
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }));
                }
                Err(error) => {
                    self.state_error = Some(error);
                    return;
                }
            }
        }
        self.state_writer.as_ref().unwrap().enqueue(state.clone());
        self.persisted_state = Some(state);
    }
    pub(super) fn exit(
        &mut self,
        window: Option<gpui_kit::AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        if self.closing {
            return;
        }
        self.closing = true;
        self.navigation_save = None;
        self.persist(cx);
        let flush = self.state_writer.as_ref().map(state::Writer::flush_handle);
        let error = self
            .state_error
            .clone()
            .filter(|_| self.state_writer.is_none() && !self.state_restore_failed);
        cx.spawn(async move |owner, cx| {
            let result = cx
                .background_spawn(async move {
                    if let Some(error) = error {
                        Err(error)
                    } else {
                        flush.map_or(Ok(()), state::Flush::wait)
                    }
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                this.closing = false;
                if let Err(error) = result {
                    this.state_error = Some(error);
                    cx.notify();
                    return;
                }
                if let Some(window) = window {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                } else {
                    cx.quit();
                }
            });
        })
        .detach();
    }
    pub(super) fn persist_navigation(&mut self, cx: &mut Context<Self>) {
        self.navigation_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |this, cx| this.persist(cx));
        }));
    }
    #[cfg(feature = "ui-tests")]
    pub fn project_changed_count(&self, id: u64) -> Option<usize> {
        self.projects
            .iter()
            .position(|p| p.id == id)
            .map(|ix| self.repository_changed_count(ix))
    }
    #[cfg(feature = "ui-tests")]
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar_width
    }
    pub fn project_count(&self) -> usize {
        self.project_rows().len()
    }
    pub fn active_project_id(&self) -> Option<u64> {
        self.projects.get(self.active).map(|p| p.id)
    }
    pub fn selected_group(&self) -> ChangeGroup {
        self.projects
            .get(self.active)
            .map_or(ChangeGroup::Changes, |p| p.group)
    }
    pub fn is_busy(&self) -> bool {
        self.projects
            .get(self.active)
            .is_some_and(|p| p.loading || p.refreshing || p.view().preview_loading)
    }
}
