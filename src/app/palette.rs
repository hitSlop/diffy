use super::About;
use super::CheckForUpdates;
use super::Diffy;
use super::FocusDiff;
use super::FocusFileSearch;
use super::ImportProjects;
use super::InstallCli;
use super::InstallUpdate;
use super::NextChange;
use super::NextFile;
use super::NextProject;
use super::NextWorktree;
use super::OpenProject;
use super::PreviousChange;
use super::PreviousFile;
use super::PreviousProject;
use super::PreviousWorktree;
use super::Quit;
use super::Refresh;
use super::RemoveCurrentProject;
use super::SelectProject;
use super::SplitMode;
use super::StageAll;
use super::StageFile;
use super::ToggleGroup;
use super::UnifiedMode;
use crate::repository::ChangeGroup;
use gpui_kit::Action;
use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Focusable as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::Keystroke;
use gpui_kit::MouseButton;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::command::Command;
use gpui_kit::component::command::CommandGroup;
use gpui_kit::component::command::CommandItem;
use gpui_kit::component::h_flex;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::div;
use gpui_kit::px;
use gpui_kit::rgb;

impl Diffy {
    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() || self.import.picker.is_some() {
            return;
        }
        if self.palette_open {
            self.close_palette(true, window, cx);
            return;
        }
        self.reset_file_scroll();
        self.previous_focus = window.focused(cx);
        self.palette_open = true;
        self.command.update(cx, |state, cx| {
            state.set_query("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }
    pub(super) fn close_palette(
        &mut self,
        cancel: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = false;
        if cancel
            || self
                .command
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        {
            self.previous_focus
                .take()
                .unwrap_or_else(|| self.focus.clone())
                .focus(window, cx);
        } else {
            self.previous_focus = None;
        }
        cx.notify();
    }
    pub(super) fn render_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.projects.get(self.active);
        let has_files = self.active_view().is_some_and(|v| !v.files.is_empty());
        let mutable = !self.is_pr()
            && p.is_some_and(|p| {
                p.path.is_some()
                    && !p.loading
                    && p.view().selected_entry().is_some_and(|e| e.stageable())
            });
        let projects =
            CommandGroup::new()
                .label("Projects")
                .items(self.project_rows().into_iter().map(|ix| {
                    let p = &self.projects[ix];
                    CommandItem::new()
                        .label(self.appearance(p).0.clone())
                        .keywords(p.path.iter().map(|p| p.display().to_string()))
                        .checked(self.same_repository(ix, self.active))
                        .action(Box::new(SelectProject { id: p.id }))
                }));
        let worktrees = CommandGroup::new().label("Worktrees").items(
            self.worktree_indices(self.active).into_iter().map(|ix| {
                let p = &self.projects[ix];
                CommandItem::new()
                    .label(self.worktree_label(ix))
                    .keywords(p.path.iter().map(|p| p.display().to_string()))
                    .checked(ix == self.active)
                    .disabled(!p.available)
                    .action(Box::new(SelectProject { id: p.id }))
            }),
        );
        let item = |label: &str, action: Box<dyn Action>, disabled: bool| {
            CommandItem::new()
                .label(label.to_owned())
                .action(action)
                .disabled(disabled)
        };
        let commands = CommandGroup::new().label("Commands").items([
            item("Open repository", Box::new(OpenProject), false),
            item(
                "Import projects from Herdr / Codex / Claude",
                Box::new(ImportProjects),
                false,
            ),
            item("Install terminal command", Box::new(InstallCli), false),
            item(
                if self.is_pr() {
                    "Refresh pull requests"
                } else {
                    "Refresh repository"
                },
                Box::new(Refresh),
                !p.is_some_and(|p| p.path.is_some() && !p.loading),
            ),
            item(
                "Next project",
                Box::new(NextProject),
                self.project_count() < 2,
            ),
            item(
                "Previous project",
                Box::new(PreviousProject),
                self.project_count() < 2,
            ),
            item(
                if self.is_pr() {
                    "Next pull request"
                } else {
                    "Next worktree"
                },
                Box::new(NextWorktree),
                if self.is_pr() {
                    self.pr_state().is_none_or(|s| s.prs.len() < 2)
                } else {
                    self.worktree_indices(self.active).len() < 2
                },
            ),
            item(
                if self.is_pr() {
                    "Previous pull request"
                } else {
                    "Previous worktree"
                },
                Box::new(PreviousWorktree),
                if self.is_pr() {
                    self.pr_state().is_none_or(|s| s.prs.len() < 2)
                } else {
                    self.worktree_indices(self.active).len() < 2
                },
            ),
            item("Next file", Box::new(NextFile), !has_files),
            item("Previous file", Box::new(PreviousFile), !has_files),
            item("Next change", Box::new(NextChange), !has_files),
            item("Previous change", Box::new(PreviousChange), !has_files),
            item(
                "Switch Staged / Changes",
                Box::new(ToggleGroup),
                p.is_none() || self.is_pr(),
            ),
            item("Unified diff", Box::new(UnifiedMode), p.is_none()),
            item("Split diff", Box::new(SplitMode), p.is_none()),
            item(
                if self.selected_group() == ChangeGroup::Changes {
                    "Stage selected file"
                } else {
                    "Unstage selected file"
                },
                Box::new(StageFile),
                !mutable,
            ),
            item(
                if self.selected_group() == ChangeGroup::Changes {
                    "Stage all files"
                } else {
                    "Unstage all files"
                },
                Box::new(StageAll),
                self.is_pr()
                    || !p.is_some_and(|p| {
                        p.path.is_some()
                            && !p.loading
                            && p.view().files.iter().any(|e| e.stageable())
                    }),
            ),
            item("Focus diff", Box::new(FocusDiff), !has_files),
            item("Find file", Box::new(FocusFileSearch), p.is_none()),
            item(
                "Copy relative path",
                Box::new(super::CopyRelativePath),
                !has_files,
            ),
            item(
                "Copy absolute path",
                Box::new(super::CopyAbsolutePath),
                self.is_pr()
                    || !p.is_some_and(|p| p.path.is_some() && p.view().selected_entry().is_some()),
            ),
            item(
                crate::project_apps::reveal_label(),
                Box::new(super::RevealFile),
                self.is_pr()
                    || !p.is_some_and(|p| p.path.is_some() && p.view().selected_entry().is_some()),
            ),
            item(
                "Increase diff font size",
                Box::new(super::ZoomIn),
                self.reader.font_size >= 24.,
            ),
            item(
                "Decrease diff font size",
                Box::new(super::ZoomOut),
                self.reader.font_size <= 10.,
            ),
            item(
                "Reset diff font size",
                Box::new(super::ResetZoom),
                self.reader.font_size == 13.,
            ),
            item(
                if self.reader.wrap {
                    "Turn line wrapping off"
                } else {
                    "Turn line wrapping on"
                },
                Box::new(super::ToggleWrap),
                false,
            ),
            item(
                "Remove from Diffy",
                Box::new(RemoveCurrentProject),
                !p.is_some_and(|p| p.path.is_some() && !p.mutating),
            ),
            item(
                "Check for updates",
                Box::new(CheckForUpdates),
                self.updates.busy(),
            ),
            item(
                "Update and restart",
                Box::new(InstallUpdate),
                self.updates.release.is_none() || self.updates.busy(),
            ),
            item("About Diffy", Box::new(About), false),
            item("Quit Diffy", Box::new(Quit), false),
        ]);
        let confirm = cx.entity().downgrade();
        let cancel = confirm.clone();
        div()
            .id("palette-overlay")
            .absolute()
            .size_full()
            .occlude()
            .child(
                div()
                    .id("palette-backdrop")
                    .test_support()
                    .absolute()
                    .size_full()
                    .bg(rgb(0).opacity(0.78))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_palette(true, window, cx)),
                    ),
            )
            .child(
                h_flex()
                    .absolute()
                    .top(px(70.))
                    .w_full()
                    .justify_center()
                    .child(
                        div()
                            .id("command-palette")
                            .test_support()
                            .w(px(520.))
                            .bg(rgb(0))
                            .rounded(px(10.))
                            .overflow_hidden()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Command::new(&self.command)
                                    .bordered(false)
                                    .bg(rgb(0))
                                    .w_full()
                                    .max_h(px(360.))
                                    .placeholder("Search projects and commands…")
                                    .group(projects)
                                    .group(worktrees)
                                    .group(commands)
                                    .on_confirm(move |_, window, cx| {
                                        let owner = confirm.clone();
                                        window.defer(cx, move |window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                this.close_palette(false, window, cx)
                                            });
                                        });
                                    })
                                    .on_cancel(move |window, cx| {
                                        let _ = cancel.update(cx, |this, cx| {
                                            this.close_palette(true, window, cx)
                                        });
                                    })
                                    .footer(|_, _, cx| {
                                        h_flex()
                                            .px_3()
                                            .py_2()
                                            .gap_4()
                                            .text_size(px(12.))
                                            .text_color(cx.theme().muted_foreground)
                                            .children(
                                                [
                                                    ("up", ""),
                                                    ("down", "Navigate"),
                                                    ("enter", "Select"),
                                                    ("escape", "Close"),
                                                ]
                                                .map(|(key, label)| {
                                                    h_flex()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(
                                                            Kbd::new(
                                                                Keystroke::parse(key)
                                                                    .expect("static key"),
                                                            )
                                                            .appearance(false),
                                                        )
                                                        .child(label)
                                                }),
                                            )
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
    pub fn palette_query(&self, cx: &App) -> String {
        self.command.read(cx).query(cx).to_string()
    }
    pub fn palette_is_open(&self) -> bool {
        self.palette_open
    }
}
