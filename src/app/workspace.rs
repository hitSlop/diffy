use super::Diffy;
use crate::repository::ChangeGroup;
use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Focusable as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::MouseButton;
use gpui_kit::ParentElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::IconName;
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::diff::Diff;
use gpui_kit::component::diff::DiffHunkSeparator;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::h_flex;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;

impl Diffy {
    pub(super) fn prepare_snake(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.snake_tab_id();
        if tab != self.snake_tab {
            self.snake_tab = tab;
            self.snake.update(cx, |snake, _| snake.pause());
            self.snake_took_focus = false;
        }
        if !self.shows_waiting_snake() {
            self.snake_took_focus = false;
            return;
        }
        if self.snake_took_focus {
            return;
        }
        let file_focus = self.projects[self.active].view().focus.clone();
        self.snake_took_focus = true;
        let idle = window
            .focused(cx)
            .is_none_or(|focused| focused == self.focus || focused == file_focus);
        if idle {
            self.snake.focus_handle(cx).focus(window, cx);
        }
    }

    fn snake_tab_id(&self) -> Option<(u64, ChangeGroup)> {
        let project = self.projects.get(self.active)?;
        Some((project.id, project.group))
    }

    fn shows_waiting_snake(&self) -> bool {
        let Some(project) = self.projects.get(self.active) else {
            return false;
        };
        !self.is_pr()
            && project.view().files.is_empty()
            && !project.loading
            && self.settings.is_none()
            && !self.palette_open
            && self.import.picker.is_none()
    }

    pub(super) fn render_center(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let show_snake = self.shows_waiting_snake();
        let Some(p) = self.projects.get(self.active) else {
            return v_flex()
                .size_full()
                .child(
                    div()
                        .id("toolbar-drag-area")
                        .w_full()
                        .h(px(46.))
                        .flex_none()
                        .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                        .on_double_click(|_, w, _| w.titlebar_double_click()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            Button::new("open-empty")
                                .ghost()
                                .label("Open repository")
                                .icon(IconName::Plus)
                                .on_click(cx.listener(|this, _, w, cx| this.open_project(w, cx))),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .children(
                                            Kbd::binding_for_action(
                                                &super::OpenProject,
                                                Some("Diffy"),
                                                window,
                                            )
                                            .map(|k| k.appearance(false)),
                                        )
                                        .child("Open")
                                        .children(
                                            Kbd::binding_for_action(
                                                &super::OpenCommandPalette,
                                                Some("Diffy"),
                                                window,
                                            )
                                            .map(|k| k.appearance(false)),
                                        )
                                        .child("Commands"),
                                ),
                        ),
                )
                .into_any_element();
        };
        let remote = self.is_pr();
        let pr_loading = self.pr_state().and_then(|s| s.loading_message());
        let view = self.active_view().unwrap();
        let mode = view.diff.read(cx).mode();
        let empty = view.files.is_empty();
        let scope_buttons = [ChangeGroup::Staged, ChangeGroup::Changes].map(|group| {
            Button::new(if group == ChangeGroup::Staged {
                "staged"
            } else {
                "changes"
            })
            .ghost()
            .small()
            .label(format!(
                "{}  {}",
                group.label(),
                p.views[group.index()].files.len()
            ))
            .selected(p.group == group)
            .toggled(p.group == group)
            .tooltip(super::shortcut_label(
                &super::ToggleGroup,
                window,
                "Switch view",
            ))
            .on_click(cx.listener(move |this, _, window, cx| this.set_group(group, window, cx)))
        });
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .children(self.render_worktrees(cx))
            .child(
                Toolbar::new("review-toolbar")
                    .h(px(46.))
                    .flex_none()
                    .px_3()
                    .gap_1()
                    .contents(if remote {
                        Vec::new()
                    } else {
                        scope_buttons
                            .into_iter()
                            .map(IntoElement::into_any_element)
                            .collect::<Vec<_>>()
                    })
                    .when(remote, |toolbar| {
                        toolbar
                            .content(if let Some(message) = pr_loading {
                                h_flex()
                                    .id("pr-loading-status")
                                    .test_support()
                                    .role(gpui_kit::Role::Status)
                                    .aria_label(message)
                                    .min_w_0()
                                    .max_w(px(320.))
                                    .gap_2()
                                    .text_size(px(12.))
                                    .child(Spinner::new().xsmall())
                                    .child(div().truncate().child(message))
                                    .into_any_element()
                            } else {
                                div()
                                    .max_w(px(320.))
                                    .truncate()
                                    .text_size(px(12.))
                                    .child(
                                        self.pr_state()
                                            .and_then(|s| {
                                                s.prs.iter().find(|p| Some(p.number) == s.selected)
                                            })
                                            .map(|p| {
                                                format!(
                                                    "#{} · {} → {}{}",
                                                    p.number,
                                                    p.head,
                                                    p.base,
                                                    if p.draft { " · Draft" } else { "" }
                                                )
                                            })
                                            .unwrap_or_else(|| "Pull requests".into()),
                                    )
                                    .into_any_element()
                            })
                            .content(
                                Button::new("open-pr-github")
                                    .ghost()
                                    .small()
                                    .label("Open in GitHub")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.open_pr_browser(cx)),
                                    ),
                            )
                    })
                    .content(
                        div()
                            .id("toolbar-drag-area")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                            .on_double_click(|_, w, _| w.titlebar_double_click()),
                    )
                    .content(
                        Button::new("previous-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronUp)
                            .disabled(empty)
                            .tooltip(super::shortcut_label(
                                &super::PreviousChange,
                                window,
                                "Previous change",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.change(false, cx))),
                    )
                    .content(
                        Button::new("next-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronDown)
                            .disabled(empty)
                            .tooltip(super::shortcut_label(
                                &super::NextChange,
                                window,
                                "Next change",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.change(true, cx))),
                    )
                    .content(
                        Button::new("unified")
                            .ghost()
                            .small()
                            .label("Unified")
                            .selected(mode == DiffMode::Unified)
                            .toggled(mode == DiffMode::Unified)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(DiffMode::Unified, cx)),
                            ),
                    )
                    .content(
                        Button::new("split")
                            .ghost()
                            .small()
                            .label("Split")
                            .selected(mode == DiffMode::Split)
                            .toggled(mode == DiffMode::Split)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(DiffMode::Split, cx)),
                            ),
                    )
                    .content(
                        Button::new("refresh")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCw)
                            .loading(pr_loading.is_some())
                            .disabled(if remote {
                                self.pr_state().is_none_or(|s| {
                                    s.listing || s.loading || s.repository.is_none()
                                })
                            } else {
                                p.path.is_none() || p.loading
                            })
                            .tooltip(super::shortcut_label(&super::Refresh, window, "Refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                h_flex()
                    .id("preview-header")
                    .test_support()
                    .group("preview-header")
                    .h(px(34.))
                    .flex_none()
                    .px_4()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(view.selected.clone().unwrap_or_default())
                            .context_menu({
                                let id = p.id;
                                let path = view.selected.clone().unwrap_or_default();
                                let local = p.path.is_some() && !remote;
                                move |menu, _, _| {
                                    super::reader::file_menu(menu, id, path.as_str(), local)
                                }
                            }),
                    )
                    .children(view.selected_file().map(|f| {
                        h_flex()
                            .gap_2()
                            .text_size(px(12.))
                            .child(
                                div()
                                    .text_color(cx.theme().success)
                                    .child(format!("+{}", f.additions())),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().danger)
                                    .child(format!("−{}", f.deletions())),
                            )
                    }))
                    .when(!remote, |row| {
                        row.child(
                            Button::new("stage-file")
                                .ghost()
                                .xsmall()
                                .icon(if p.group == ChangeGroup::Changes {
                                    IconName::Plus
                                } else {
                                    IconName::Minus
                                })
                                .accessibility_label(if p.group == ChangeGroup::Changes {
                                    "Stage file"
                                } else {
                                    "Unstage file"
                                })
                                .when(
                                    !view
                                        .diff
                                        .read(cx)
                                        .focus_handle(cx)
                                        .contains_focused(window, cx),
                                    |button| {
                                        button.invisible().group_hover("preview-header", |button| {
                                            button.visible()
                                        })
                                    },
                                )
                                .disabled(
                                    empty
                                        || p.path.is_none()
                                        || p.loading
                                        || view.selected_entry().is_some_and(|e| !e.stageable()),
                                )
                                .tooltip(if p.path.is_none() {
                                    "Open a repository to stage files".to_owned()
                                } else {
                                    super::shortcut_label(
                                        &super::StageFile,
                                        window,
                                        "Stage / unstage file",
                                    )
                                })
                                .on_click(cx.listener(|this, _, _, cx| this.stage_file(cx))),
                        )
                    }),
            )
            .children(
                (if remote {
                    self.pr_state().and_then(|s| s.error.as_ref())
                } else {
                    p.error.as_ref()
                })
                .map(|error| {
                    h_flex()
                        .px_4()
                        .py_2()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .text_color(cx.theme().danger)
                                .child(error.clone()),
                        )
                        .child(
                            Button::new("retry")
                                .ghost()
                                .small()
                                .label("Retry")
                                .disabled(p.loading)
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        )
                }),
            )
            .children(view.preview_error.as_ref().map(|error| {
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .child(div().text_color(cx.theme().danger).child(error.clone()))
                    .child(
                        Button::new("retry-preview")
                            .ghost()
                            .label("Retry")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ensure_preview(cx);
                            })),
                    )
            }))
            .children(view.selected_entry().filter(|e| !e.stageable()).map(|_| {
                div()
                    .px_3()
                    .py_2()
                    .text_color(cx.theme().danger)
                    .child("Unmerged file · resolve and stage outside Diffy")
            }))
            .child(if !empty && view.preview.is_none() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .gap_2()
                    .children(pr_loading.map(|_| {
                        div()
                            .id("pr-loading-empty")
                            .test_support()
                            .child(Spinner::new().small())
                    }))
                    .child(if view.preview_error.is_some() && !view.preview_loading {
                        "Preview unavailable"
                    } else {
                        pr_loading.unwrap_or("Loading preview…")
                    })
                    .into_any_element()
            } else if empty {
                if show_snake {
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .text_color(cx.theme().muted_foreground)
                        .child(if p.group == ChangeGroup::Staged {
                            "No staged files yet. While you wait:"
                        } else {
                            "No changes yet. While you wait:"
                        })
                        .child(self.snake.clone())
                        .into_any_element()
                } else {
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .text_color(cx.theme().muted_foreground)
                        .gap_2()
                        .children(pr_loading.map(|_| {
                            div()
                                .id("pr-loading-empty")
                                .test_support()
                                .child(Spinner::new().small())
                        }))
                        .child(if remote {
                            pr_loading.unwrap_or_else(|| self.pr_empty_message())
                        } else if p.loading {
                            "Reading changes…"
                        } else if p.group == ChangeGroup::Staged {
                            "No staged files"
                        } else {
                            "No changes"
                        })
                        .into_any_element()
                }
            } else {
                div()
                    .id("diff-viewport")
                    .relative()
                    .overflow_hidden()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        Diff::new(&view.diff)
                            .absolute()
                            .size_full()
                            .border_0()
                            .header_visible(false)
                            .soft_wrap(self.reader.wrap)
                            .split_ratio(0.4)
                            .hunk_separator(DiffHunkSeparator::Simple),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
    pub(super) fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if self.is_pr() {
            if let Some(view) = self.active_view() {
                view.diff.update(cx, |d, cx| d.set_mode(mode, cx));
            }
            cx.notify();
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        for view in &p.views {
            view.diff.update(cx, |d, cx| d.set_mode(mode, cx));
        }
        self.persist(cx);
    }
    pub(super) fn change(&mut self, next: bool, cx: &mut Context<Self>) {
        let Some(view) = self.active_view() else {
            return;
        };
        view.diff.update(cx, |d, cx| {
            if next {
                d.next_change(cx)
            } else {
                d.previous_change(cx)
            }
        });
    }
    pub(super) fn focus_diff(&self, window: &mut Window, cx: &mut App) {
        if let Some(view) = self.active_view() {
            view.diff.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    pub fn selected_mode(&self, cx: &App) -> DiffMode {
        self.active_view()
            .map_or(DiffMode::Unified, |view| view.diff.read(cx).mode())
    }
}
