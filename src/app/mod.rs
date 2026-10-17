use crate::fonts;
use crate::repository::ChangeGroup;
use crate::state;
use crate::updates;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::FocusHandle;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::ParentElement as _;
use gpui_kit::Render;
use gpui_kit::Styled as _;
use gpui_kit::Subscription;
use gpui_kit::Task;
use gpui_kit::Window;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::command::CommandState;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::h_flex;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::resizable::ResizableState;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::rgb;
use gpui_kit::{Action, AsKeystroke as _, KeyBinding, KeyContext, Pixels};
use std::path::PathBuf;

mod files;
mod github_review;
mod import;
mod notice;
mod palette;
mod project_menu;
mod rail;
mod reader;
mod review;
mod session;
mod settings;
mod snake;
mod sync;
mod workspace;
mod worktrees;

gpui_kit::actions!(
    diffy,
    [
        OpenProject,
        ImportProjects,
        InstallCli,
        Refresh,
        NextProject,
        PreviousProject,
        NextWorktree,
        PreviousWorktree,
        NextFile,
        PreviousFile,
        NextChange,
        PreviousChange,
        FocusDiff,
        FocusFileSearch,
        CopyRelativePath,
        CopyAbsolutePath,
        RevealFile,
        ZoomIn,
        ZoomOut,
        ResetZoom,
        ToggleWrap,
        ToggleGroup,
        StageFile,
        StageAll,
        OpenCommandPalette,
        RemoveCurrentProject,
        UnifiedMode,
        SplitMode,
        CheckForUpdates,
        InstallUpdate,
        About,
        Hide,
        HideOthers,
        ShowAll,
        Quit
    ]
);
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct SelectProject {
    pub id: u64,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct RemoveProject {
    pub id: u64,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct EditProject {
    pub id: u64,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct OpenProjectIn {
    pub id: u64,
    pub app: crate::project_apps::ProjectApp,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct CopyPath {
    pub id: u64,
    pub path: String,
    pub absolute: bool,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct RevealPath {
    pub id: u64,
    pub path: String,
}

pub fn init(cx: &mut App) {
    fonts::init(cx);
    cx.set_menus([
        gpui_kit::Menu::new("Diffy").items([
            gpui_kit::MenuItem::action("About Diffy", About),
            gpui_kit::MenuItem::action("Check for Updates…", CheckForUpdates),
            gpui_kit::MenuItem::Separator,
            #[cfg(target_os = "macos")]
            gpui_kit::MenuItem::action("Hide Diffy", Hide),
            #[cfg(target_os = "macos")]
            gpui_kit::MenuItem::action("Hide Others", HideOthers),
            #[cfg(target_os = "macos")]
            gpui_kit::MenuItem::action("Show All", ShowAll),
            gpui_kit::MenuItem::Separator,
            gpui_kit::MenuItem::action("Quit Diffy", Quit),
        ]),
        gpui_kit::Menu::new("View").items([
            gpui_kit::MenuItem::action("Increase Diff Font Size", ZoomIn),
            gpui_kit::MenuItem::action("Decrease Diff Font Size", ZoomOut),
            gpui_kit::MenuItem::action("Reset Diff Font Size", ResetZoom),
            gpui_kit::MenuItem::Separator,
            gpui_kit::MenuItem::action("Toggle Line Wrapping", ToggleWrap),
            gpui_kit::MenuItem::Separator,
            gpui_kit::MenuItem::action("Previous Worktree", PreviousWorktree),
            gpui_kit::MenuItem::action("Next Worktree", NextWorktree),
        ]),
        gpui_kit::Menu::new("File").items([
            gpui_kit::MenuItem::action("Open Repository…", OpenProject),
            gpui_kit::MenuItem::action("Import Projects…", ImportProjects),
            gpui_kit::MenuItem::action("Install terminal command…", InstallCli),
            gpui_kit::MenuItem::action("Refresh Repository", Refresh),
        ]),
    ]);
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("secondary-h", Hide, Some("Diffy")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("secondary-alt-h", HideOthers, Some("Diffy")),
        KeyBinding::new("secondary-p", OpenCommandPalette, Some("Diffy")),
        KeyBinding::new("secondary-o", OpenProject, Some("Diffy")),
        KeyBinding::new("secondary-r", Refresh, Some("Diffy")),
        KeyBinding::new("secondary-shift-down", NextProject, Some("Diffy")),
        KeyBinding::new("secondary-shift-up", PreviousProject, Some("Diffy")),
        KeyBinding::new("secondary-right", NextWorktree, Some("Diffy")),
        KeyBinding::new("secondary-left", PreviousWorktree, Some("Diffy")),
        KeyBinding::new("secondary-down", NextFile, Some("Diffy")),
        KeyBinding::new("secondary-up", PreviousFile, Some("Diffy")),
        KeyBinding::new("down", NextFile, Some("DiffyFiles")),
        KeyBinding::new("up", PreviousFile, Some("DiffyFiles")),
        KeyBinding::new("alt-down", NextChange, Some("Diffy")),
        KeyBinding::new("alt-up", PreviousChange, Some("Diffy")),
        KeyBinding::new("secondary-enter", FocusDiff, Some("Diffy")),
        KeyBinding::new("secondary-f", FocusFileSearch, Some("Diffy")),
        KeyBinding::new("secondary-shift-s", ToggleGroup, Some("Diffy")),
        KeyBinding::new("secondary-s", StageFile, Some("Diffy")),
        KeyBinding::new("secondary-q", Quit, Some("Diffy")),
        KeyBinding::new("secondary-=", ZoomIn, Some("Diffy")),
        KeyBinding::new("secondary-+", ZoomIn, Some("Diffy")),
        KeyBinding::new("secondary-shift-=", ZoomIn, Some("Diffy")),
        KeyBinding::new("secondary-shift-+", ZoomIn, Some("Diffy")),
        KeyBinding::new("secondary--", ZoomOut, Some("Diffy")),
        KeyBinding::new("secondary-0", ResetZoom, Some("Diffy")),
    ]);
}

pub struct Diffy {
    window_handle: gpui_kit::AnyWindowHandle,
    import: import::Runtime,
    github: github_review::Runtime,
    projects: Vec<session::Project>,
    repositories: Vec<worktrees::RepositoryProject>,
    topology_sequence: u64,
    active: usize,
    focus: FocusHandle,
    next_id: u64,
    state_path: Option<PathBuf>,
    state_error: Option<String>,
    state_restore_failed: bool,
    restoring: bool,
    sidebar_width: f32,
    command: Entity<CommandState>,
    palette_open: bool,
    previous_focus: Option<FocusHandle>,
    hovered_project: Option<usize>,
    settings: Option<settings::ProjectSettings>,
    project_apps: project_menu::Runtime,
    repeat_direction: Option<isize>,
    file_repeat: Option<Task<()>>,
    native_repeat_at: Option<std::time::Instant>,
    native_has_repeated: bool,
    file_key_probe: fn(isize) -> bool,
    file_scroll_remainder: f32,
    file_scroll_axis: Option<files::FileScrollAxis>,
    file_scroll_gesture: files::FileScrollGesture,
    navigation_save: Option<Task<()>>,
    persisted_state: Option<state::SavedState>,
    preview_cache: crate::repository::SharedPreviewCache,
    state_writer: Option<state::Writer>,
    state_save_task: Option<Task<()>>,
    closing: bool,
    quit_subscription: Option<Subscription>,
    reader: state::ReaderSettings,
    preview_requests: u64,
    updates: updates::Controller,
    update_task: Option<Task<()>>,
    snake: Entity<snake::SnakeGame>,
    /// Project and Staged/Changes tab the snake was last shown for.
    snake_tab: Option<(u64, ChangeGroup)>,
    /// Set once per visit to an empty pane, so focus is offered once.
    snake_took_focus: bool,
    _keyboard_subscription: Subscription,
}

impl Render for Diffy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.prepare_snake(window, cx);
        let p = self.projects.get(self.active);
        let hint = |actions: &[&dyn Action], label: &str| {
            h_flex()
                .items_center()
                .gap(px(7.))
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .children(actions.iter().filter_map(|action| {
                            Kbd::binding_for_action(*action, Some("Diffy"), window).map(|kbd| {
                                kbd.appearance(false)
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .text_color(rgb(0xb3b8c2))
                            })
                        })),
                )
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_owned()),
                )
        };
        let shortcuts = h_flex()
            .items_center()
            .gap(px(22.))
            .text_size(px(12.))
            .child(hint(&[&OpenCommandPalette], "Commands"))
            .child(hint(&[&PreviousProject, &NextProject], "Projects"))
            .child(hint(&[&PreviousFile, &NextFile], "Files"))
            .child(hint(&[&FocusFileSearch], "Find"))
            .when(!self.is_pr(), |row| {
                row.child(hint(&[&ToggleGroup], "View"))
            })
            .when(!self.is_pr(), |row| {
                row.child(hint(
                    &[&StageFile],
                    if self.selected_group() == ChangeGroup::Staged {
                        "Unstage"
                    } else {
                        "Stage"
                    },
                ))
            });
        let breadcrumb = p.map(|p| {
            let path = p.path.as_ref().map_or_else(
                || p.name.to_string(),
                |path| {
                    let parent = path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned());
                    match parent {
                        Some(parent) => format!("…/{parent}/{}", p.name),
                        None => p.name.to_string(),
                    }
                },
            );
            h_flex()
                .min_w_0()
                .gap(px(9.))
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(path))
                .child("/")
                .child(div().flex_none().child(if self.is_pr() {
                    self.pr_state()
                        .and_then(|s| {
                            s.repository.as_ref().map(|r| {
                                s.selected
                                    .map_or_else(|| r.label(), |n| format!("{} #{}", r.label(), n))
                            })
                        })
                        .unwrap_or_else(|| "Pull requests".into())
                } else {
                    p.branch.to_string()
                }))
        });
        v_flex()
            .id("diffy")
            .relative()
            .key_context("Diffy")
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(rgb(0))
            .font_family(fonts::FAMILY)
            .font_features(fonts::features())
            .text_size(px(13.))
            .text_color(cx.theme().foreground)
            .capture_key_down(cx.listener(|this, event: &gpui_kit::KeyDownEvent, _, cx| {
                if cfg!(target_os = "macos") || this.palette_open || this.settings.is_some() || this.import.picker.is_some() { return; }
                let modifiers = event.keystroke.modifiers;
                if !modifiers.secondary() || modifiers.shift || modifiers.alt { return; }
                let direction = match event.keystroke.key.as_str() { "up" => -1, "down" => 1, _ => return };
                if (this.file_key_probe)(direction) { return; }
                cx.stop_propagation();
                this.native_file_key(direction, event.is_held, cx);
            }))
            .on_key_up(cx.listener(|this, _, _, _| this.stop_file_repeat()))
            .on_modifiers_changed(cx.listener(|this, event: &gpui_kit::ModifiersChangedEvent, _, _| {
                this.reset_file_scroll_unless_command(event);
            }))
            .on_action(cx.listener(|this, a: &EditProject, w, cx| this.open_settings(a.id, w, cx)))
            .on_action(cx.listener(|this, a: &OpenProjectIn, w, cx| this.open_project_in(a, w, cx)))
            .on_action(cx.listener(|this, _: &CopyRelativePath, _, cx| this.copy_path(false, cx)))
            .on_action(cx.listener(|this, action: &CopyPath, _, cx| this.copy_file_path(action.id, &action.path, action.absolute, cx)))
            .on_action(cx.listener(|this, action: &RevealPath, _, cx| this.reveal_file_path(action.id, &action.path, cx)))
            .on_action(cx.listener(|this, _: &CopyAbsolutePath, _, cx| this.copy_path(true, cx)))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| this.reveal_file(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom(1., cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom(-1., cx)))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.zoom(13. - this.reader.font_size, cx)))
            .on_action(cx.listener(|this, _: &ToggleWrap, _, cx| this.toggle_wrap(cx)))
            .on_action(cx.listener(|this, _: &StageAll, _, cx| this.stage_all(cx)))
            .on_action(cx.listener(|this, _: &OpenProject, w, cx| this.open_project(w, cx)))
            .on_action(cx.listener(|this, _: &ImportProjects, w, cx| this.open_import(w, cx)))
            .on_action(cx.listener(|this, _: &InstallCli, w, cx| this.install_cli(w, cx)))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, w, cx| this.open_palette(w, cx)))
            .on_action(cx.listener(|this, a: &SelectProject, w, cx| {
                if let Some(ix) = this.projects.iter().position(|p| p.id == a.id) {
                    this.switch_project(ix, w, cx);
                }
            }))
            .on_action(
                cx.listener(|this, a: &RemoveProject, w, cx| this.remove_project(a.id, w, cx)),
            )
            .on_action(cx.listener(|this, _: &RemoveCurrentProject, w, cx| {
                if let Some(id) = this.active_project_id() {
                    this.remove_project(id, w, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &UnifiedMode, _, cx| this.set_mode(DiffMode::Unified, cx)),
            )
            .on_action(cx.listener(|this, _: &SplitMode, _, cx| this.set_mode(DiffMode::Split, cx)))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &StageFile, _, cx| this.stage_file(cx)))
            .on_action(cx.listener(|this, _: &ToggleGroup, w, cx| {
                this.set_group(this.selected_group().other(), w, cx)
            }))
            .on_action(cx.listener(|this, _: &NextProject, w, cx| this.move_project(1, w, cx)))
            .on_action(cx.listener(|this, _: &PreviousProject, w, cx| this.move_project(-1, w, cx)))
            .on_action(cx.listener(|this, _: &NextWorktree, w, cx| this.move_worktree(1, w, cx)))
            .on_action(cx.listener(|this, _: &PreviousWorktree, w, cx| this.move_worktree(-1, w, cx)))
            .on_action(cx.listener(|this, _: &NextFile, _, cx| this.move_file(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousFile, _, cx| this.move_file(-1, cx)))
            .on_action(cx.listener(|this, _: &NextChange, _, cx| this.change(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousChange, _, cx| this.change(false, cx)))
            .on_action(cx.listener(|this, _: &FocusDiff, w, cx| this.focus_diff(w, cx)))
            .on_action(cx.listener(|this, _: &FocusFileSearch, w, cx| {
                this.focus_file_search(w, cx)
            }))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| this.check_updates(true, cx)))
            .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| this.install_update(cx)))
            .on_action(cx.listener(|_, _: &Hide, _, cx| cx.hide()))
            .on_action(cx.listener(|_, _: &HideOthers, _, cx| cx.hide_other_apps()))
            .on_action(cx.listener(|_, _: &ShowAll, _, cx| cx.unhide_other_apps()))
            .on_action(cx.listener(|_, _: &About, w, cx| {
                let prompt = w.prompt(gpui_kit::PromptLevel::Info, "Diffy", Some(concat!("Version ", env!("CARGO_PKG_VERSION"), "\nA small desktop Git diff viewer.\nhttps://github.com/hitSlop/diffy\nMIT License · © 2026 hitSlop")), &["OK"], cx);
                cx.spawn(async move |_, _| { let _ = prompt.await; }).detach();
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                if this.updates.blocks_mutations() { return; }
                this.exit(None, cx);
            }))
            .when(!cfg!(target_os = "macos") && matches!(window.window_decorations(), gpui_kit::Decorations::Client { .. }), |root| {
                root.child(gpui_kit::component::TitleBar::new())
            })
            .child(self.file_scroll_layer(cx))
            .children(self.state_error.as_ref().map(|error| {
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(cx.theme().danger)
                    .child(error.clone())
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_projects(window, cx))
                    .child(
                        div().flex_1().min_w_0().min_h_0().h_full().child(
                            h_resizable("workspace")
                                .on_resize(cx.listener(|this, state: &Entity<ResizableState>, _, cx| {
                                    if let Some(width) = state.read(cx).sizes().get(1).copied() {
                                        this.sidebar_width = f32::from(width).clamp(240., 460.);
                                        this.persist_navigation(cx);
                                    }
                                }))
                                .child(
                                    resizable_panel()
                                        .size_range(px(360.)..Pixels::MAX)
                                        .child(self.render_center(window, cx)),
                                )
                                .child(
                                    resizable_panel()
                                        .size(px(self.sidebar_width))
                                        .size_range(px(240.)..px(460.))
                                        .child(self.render_files(window, cx)),
                                ),
                        ),
                    ),
            )
            .children(self.render_update_notice(cx))
            .child(
                StatusBar::new()
                    .border_0()
                    .bg(rgb(0))
                    .px(px(14.))
                    .py(px(6.))
                    .left(h_flex().min_w_0().children(breadcrumb))
                    .right(shortcuts),
            )
            .when(self.palette_open, |root| {
                root.child(self.render_palette(cx))
            })
            .when(self.settings.is_some(), |root| {
                root.child(self.render_settings(cx))
            })
    }
}

/// Tooltips use the same bindings as dispatch and Kbd, including secondary modifiers.
pub(super) fn shortcut_label(action: &dyn Action, window: &Window, label: &str) -> String {
    let binding = window.highest_precedence_binding_for_action_in_context(
        action,
        KeyContext::parse("Diffy").expect("static context"),
    );
    match binding.and_then(|binding| {
        binding
            .keystrokes()
            .first()
            .map(|key| Kbd::format(key.as_keystroke()))
    }) {
        Some(keys) => format!("{label} · {keys}"),
        None => label.to_owned(),
    }
}
