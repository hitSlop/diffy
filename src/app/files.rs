use super::Diffy;
use super::review::FILE_INDENT;
use super::review::FILE_ROW_HEIGHT;
use super::review::FOLDER_HEADER_HEIGHT;
use super::review::SidebarRow;
use crate::repository::ChangeGroup;
use gpui_kit::AnyElement;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::DispatchPhase;
use gpui_kit::Entity;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::ModifiersChangedEvent;
use gpui_kit::MouseButton;
use gpui_kit::ParentElement as _;
use gpui_kit::ScrollDelta;
use gpui_kit::ScrollWheelEvent;
use gpui_kit::SharedString;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::TouchPhase;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::canvas;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::diff::DiffFileStatus;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputEvent;
use gpui_kit::component::input::InputState;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::v_virtual_list;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::rgb;
use std::time::{Duration, Instant};

/// Pixels of trackpad travel that move one file.
const FILE_SCROLL_STEP: f32 = 40.;
const FILE_SCROLL_IDLE: Duration = Duration::from_millis(200);

#[derive(Default)]
pub(super) struct FileScrollGesture {
    command: bool,
    phased: bool,
    last_event: Option<Instant>,
}

impl FileScrollGesture {
    /// Returns whether this event starts a new gesture. Keep the initial mode
    /// after Ended: macOS delivers the momentum tail as Moved events.
    fn update(&mut self, precise: bool, phase: TouchPhase, command: bool, now: Instant) -> bool {
        if !precise {
            *self = Self::default();
            return true;
        }
        let fresh = phase == TouchPhase::Started
            || self.last_event.is_none()
            || (!self.phased
                && self
                    .last_event
                    .is_some_and(|last| now.duration_since(last) >= FILE_SCROLL_IDLE));
        if fresh {
            self.command = command;
            self.phased = phase == TouchPhase::Started;
        }
        self.last_event = Some(now);
        if !command || phase == TouchPhase::Cancelled {
            self.command = false;
        }
        fresh
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FileScrollAxis {
    Vertical,
    Horizontal,
}

impl Diffy {
    fn ensure_file_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<InputState>> {
        if self.is_pr() {
            let key = self.github_key()?;
            if let Some(search) = self.active_view()?.search.clone() {
                return Some(search);
            }
            let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find file"));
            self.active_view_mut()?.search = Some(search.clone());
            let subscription = cx.subscribe(&search, move |this, input, event, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(state) = this.github.states.get_mut(&key)
                {
                    state.view.set_query(input.read(cx).value().to_string(), cx);
                }
            });
            self.github
                .states
                .get_mut(&self.github_key()?)
                .unwrap()
                .add_subscription(subscription);
            return Some(search);
        }
        let project = self.projects.get(self.active)?;
        if let Some(search) = project.view().search.clone() {
            return Some(search);
        }
        let project_id = project.id;
        let group = project.group.index();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Find file"));
        self.projects[self.active].view_mut().search = Some(search.clone());
        let subscription = cx.subscribe(&search, move |this, input, event, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let query = input.read(cx).value().to_string();
            let Some(project) = this.projects.iter_mut().find(|p| p.id == project_id) else {
                return;
            };
            project.views[group].set_query(query, cx);
        });
        self.projects[self.active].subscriptions.push(subscription);
        Some(search)
    }

    pub(super) fn render_files(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_github_repository(window, cx);
        let Some(search) = self.ensure_file_search(window, cx) else {
            return div().size_full().bg(rgb(0)).into_any_element();
        };
        let Some(p) = self.projects.get(self.active) else {
            return div().size_full().bg(rgb(0)).into_any_element();
        };
        let view = self.active_view().unwrap();
        let files = view.files.clone();
        let rows = view.rows.clone();
        let sidebar = view.sidebar.clone();
        let selected = view.selected.clone();
        let show_empty = !view.query.trim().is_empty() && sidebar.is_empty() && !files.is_empty();
        let empty_color = cx.theme().muted_foreground;
        let owner = cx.entity().downgrade();
        let project_id = p.id;
        let group = p.group;
        let local = p.path.is_some() && !self.is_pr();
        let disabled = !local || p.loading;
        let files_focused = view.focus.contains_focused(window, cx);
        v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(46.))
                    .flex_none()
                    .px_4()
                    .gap_2()
                    .child(
                        div()
                            .font_medium()
                            .text_size(px(14.))
                            .child(if self.is_pr() { "Files" } else { p.group.label() }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(files.len().to_string()),
                    )
                    .child(
                        div()
                            .id("sidebar-drag-area")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                            .on_double_click(|_, w, _| w.titlebar_double_click()),
                    ),
            )
            .child(
                div().flex_none().px_2().pb_2().child(
                    Input::new(&search)
                        .id("file-search")
                        .small()
                        .w_full(),
                ),
            )
            .child(
                div()
                    .id("file-list")
                    .relative()
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .track_focus(&view.focus)
                    .key_context("DiffyFiles")
                    .child(
                        v_virtual_list(cx.entity(), "files", view.row_sizes.clone(), move |_, range, _, cx| {
                            range
                                .map(|ix| match &sidebar[ix] {
                                    SidebarRow::Folder(header) => {
                                        let ancestor = header.ancestor.clone();
                                        let leaf = header.leaf.clone();
                                        let label = header.label.clone();
                                        let count = header.count.to_string();
                                        let leading = header.leading;
                                        let muted = cx.theme().muted_foreground;
                                        let foreground = cx.theme().foreground;
                                        div()
                                            .w_full()
                                            .h(px(leading + FOLDER_HEADER_HEIGHT))
                                            .pt(px(leading))
                                            .child(
                                                h_flex()
                                                    .id(SharedString::from(format!("folder:{label}")))
                                                    .test_support()
                                                    .w_full()
                                                    .h(px(FOLDER_HEADER_HEIGHT))
                                                    .px_2()
                                                    .gap_2()
                                                    .items_center()
                                                    .text_size(px(12.))
                                                    .child(
                                                        h_flex()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .overflow_hidden()
                                                            .children(ancestor.map(|dir| {
                                                                div()
                                                                    .min_w_0()
                                                                    .truncate()
                                                                    .text_ellipsis_middle()
                                                                    .text_color(muted)
                                                                    .child(dir)
                                                            }))
                                                            .when(header.ancestor.is_some(), |row| {
                                                                row.child(
                                                                    div()
                                                                        .flex_shrink_0()
                                                                        .text_color(muted)
                                                                        .child(" · "),
                                                                )
                                                            })
                                                            .child(
                                                                div()
                                                                    .flex_shrink_0()
                                                                    .max_w_full()
                                                                    .truncate()
                                                                    .text_ellipsis_middle()
                                                                    .font_medium()
                                                                    .text_color(foreground)
                                                                    .child(leaf),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .text_color(muted)
                                                            .child(count),
                                                    )
                                                    .child(div().w(px(20.)).flex_shrink_0()),
                                            )
                                            .into_any_element()
                                    }
                                    SidebarRow::File { index, indent } => {
                                    let index = *index;
                                    let indent = *indent;
                                    let f = &files[index];
                                    let row = &rows[index];
                                    let path = f.path().clone();
                                    let owner = owner.clone();
                                    let action_owner = owner.clone();
                                    let action_path = path.clone();
                                    let tooltip_path = path.clone();
                                    let menu_path = path.clone();
                                    let filename = row.filename.clone();
                                    let row_group = row.group.clone();
                                    let action_label = if group == ChangeGroup::Changes {
                                        row.stage_label.clone()
                                    } else {
                                        row.unstage_label.clone()
                                    };
                                    let selected = selected.as_ref() == Some(&path);
                                    let (status, color) = match f.status() {
                                        DiffFileStatus::Added => ("A", cx.theme().success),
                                        DiffFileStatus::Deleted => ("D", cx.theme().danger),
                                        DiffFileStatus::Renamed => ("R", cx.theme().primary),
                                        DiffFileStatus::Copied => ("C", cx.theme().primary),
                                        DiffFileStatus::Conflicted => ("U", cx.theme().danger),
                                        _ => ("M", cx.theme().muted_foreground),
                                    };
                                    div()
                                        .w_full()
                                        .h(px(FILE_ROW_HEIGHT))
                                        .pl(px(if indent { FILE_INDENT } else { 0. }))
                                        .child(
                                    ListItem::new(path.clone())
                                        .group(row_group.clone())
                                        .h(px(FILE_ROW_HEIGHT))
                                        .px_2()
                                        .w_full()
                                        .min_w_0()
                                        .rounded(px(5.))
                                        .selected(selected)
                                        .accessibility_label(path.clone())
                                        .tooltip(move |w, cx| Tooltip::new(tooltip_path.clone()).max_w(px(640.)).whitespace_normal().build(w, cx))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .gap_2()
                                                .child(
                                                    Icon::new(IconName::FileText)
                                                        .xsmall()
                                                        .text_color(cx.theme().muted_foreground),
                                                )
                                                .child(
                                                    div()
                                                        .id(("file-name", index))
                                                        .test_support()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .truncate()
                                                        .text_ellipsis_middle()
                                                        .font_medium()
                                                        .text_size(px(13.))
                                                        .text_color(cx.theme().foreground)
                                                        .role(gpui_kit::accesskit::Role::Label)
                                                        .aria_label(filename.clone())
                                                        .child(filename),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color)
                                                        .child(status),
                                                )
                                                .when(local, |row| row.child(
                                                    Button::new(("file-action", index))
                                                        .ghost()
                                                        .xsmall()
                                                        .icon(if group == ChangeGroup::Changes {
                                                            IconName::Plus
                                                        } else {
                                                            IconName::Minus
                                                        })
                                                        .disabled(disabled || !f.stageable())
                                                        .accessibility_label(action_label)
                                                        .tooltip(if group == ChangeGroup::Changes {
                                                            "Stage file"
                                                        } else {
                                                            "Unstage file"
                                                        })
                                                        .when(
                                                            !(selected && files_focused),
                                                            |button| {
                                                                button.invisible().group_hover(
                                                                    row_group,
                                                                    |button| button.visible(),
                                                                )
                                                            },
                                                        )
                                                        .on_click(move |_, _, cx| {
                                                            cx.stop_propagation();
                                                            let _ = action_owner.update(
                                                                cx,
                                                                |this, cx| {
                                                                    this.stage_path(
                                                                        project_id,
                                                                        group,
                                                                        &action_path,
                                                                        cx,
                                                                    )
                                                                },
                                                            );
                                                        }),
                                                )),
                                        )
                                        .on_click(move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                this.select_file(path.clone(), window, cx)
                                            });
                                        }))
                                        .context_menu(move |menu, _, _| super::reader::file_menu(menu, project_id, menu_path.as_str(), local))
                                        .into_any_element()
                                    }
                                })
                                .collect()
                        })
                        .size_full()
                        .track_scroll(&view.scroll),
                    )
                    .when(show_empty, |list| {
                        list.child(
                            div()
                                .id("file-search-empty")
                                .test_support()
                                .absolute()
                                .top(px(8.))
                                .left(px(8.))
                                .text_size(px(12.))
                                .text_color(empty_color)
                                .child("No matching files"),
                        )
                    })
                    .vertical_scrollbar(&view.scroll),
            )
            .when(local, |list| list.child(
                div().flex_none().px_3().py_3().child(
                    Button::new("stage-all")
                        .ghost()
                        .w_full()
                        .icon(if group == ChangeGroup::Changes {
                            IconName::Plus
                        } else {
                            IconName::Minus
                        })
                        .label(if group == ChangeGroup::Changes {
                            if view.files.iter().any(|f| !f.stageable()) { "Stage all except conflicts" } else { "Stage all" }
                        } else {
                            "Unstage all"
                        })
                        .disabled(disabled || view.files.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.stage_all(cx))),
                ),
            ))
            .into_any_element()
    }
    pub(super) fn focus_file_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() || self.palette_open || self.import.picker.is_some() {
            return;
        }
        self.stop_file_repeat();
        let Some(search) = self.active_view().and_then(|view| view.search.clone()) else {
            return;
        };
        search.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }
    pub(super) fn select_file(
        &mut self,
        path: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.active_view_mut() else {
            return;
        };
        view.select(path, cx);
        view.focus.focus(window, cx);
        self.ensure_preview(cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn move_file(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(ix) = self
            .active_view()
            .and_then(|view| view.step_visible(direction))
        else {
            return;
        };
        let path = self.active_view().unwrap().files[ix].path().clone();
        self.active_view_mut().unwrap().select(path, cx);
        self.ensure_preview(cx);
        self.persist_navigation(cx);
        cx.notify();
    }
    pub(super) fn start_file_repeat(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.repeat_direction == Some(direction) && (self.file_key_probe)(direction) {
            return; // The timer owns repeats; ignore native autorepeat duplicates.
        }
        self.stop_file_repeat();
        self.move_file(direction, cx); // Every new press moves immediately.
        let project = self.active_project_id();
        let group = self.selected_group();
        let require_active = window.is_window_active();
        self.repeat_direction = Some(direction);
        self.file_repeat = Some(cx.spawn_in(window, async move |this, cx| {
            let mut elapsed = 0;
            let mut next_step = 320;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                elapsed += 16;
                let keep = cx
                    .update(|window, cx| {
                        this.update(cx, |this, cx| {
                            if this.repeat_direction != Some(direction)
                                || !(this.file_key_probe)(direction)
                                || this.settings.is_some()
                                || this.palette_open
                                || this.active_project_id() != project
                                || this.selected_group() != group
                                || (require_active && !window.is_window_active())
                            {
                                this.repeat_direction = None;
                                return false;
                            }
                            if elapsed >= next_step {
                                this.move_file(direction, cx);
                                next_step += 200;
                            }
                            true
                        })
                    })
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        }));
    }
    pub(super) fn stop_file_repeat(&mut self) {
        self.repeat_direction = None;
        self.native_repeat_at = None;
        self.native_has_repeated = false;
        self.file_repeat = None;
    }
    pub(super) fn native_file_key(&mut self, direction: isize, held: bool, cx: &mut Context<Self>) {
        let now = std::time::Instant::now();
        let delay = Duration::from_millis(if self.native_has_repeated { 200 } else { 320 });
        if held
            && self.repeat_direction == Some(direction)
            && self
                .native_repeat_at
                .is_some_and(|last| now.duration_since(last) < delay)
        {
            return;
        }
        self.native_has_repeated = held && self.repeat_direction == Some(direction);
        self.repeat_direction = Some(direction);
        self.native_repeat_at = Some(now);
        self.move_file(direction, cx);
    }
    /// Capture runs before the diff list applies the wheel, so stopping the
    /// event here keeps Command+scroll on the file list.
    pub(super) fn file_scroll_layer(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let view = view.clone();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let consumed =
                        view.update(cx, |this, cx| this.scroll_files_with_wheel(event, cx));
                    if consumed {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
    pub(super) fn reset_file_scroll_unless_command(&mut self, event: &ModifiersChangedEvent) {
        if !event.modifiers.secondary() || event.modifiers.shift || event.modifiers.alt {
            self.reset_file_scroll();
        }
    }
    pub(super) fn reset_file_scroll(&mut self) {
        self.file_scroll_remainder = 0.;
        self.file_scroll_axis = None;
        self.file_scroll_gesture.command = false;
    }
    /// Command plus a vertical wheel steps files. A horizontal gesture is left
    /// for the diff to pan. Returns whether this event was consumed.
    fn scroll_files_with_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let mods = event.modifiers;
        let command = mods.secondary()
            && !mods.shift
            && !mods.alt
            && !self.palette_open
            && self.settings.is_none()
            && self.import.picker.is_none();
        if self.file_scroll_gesture.update(
            event.delta.precise(),
            event.touch_phase,
            command,
            Instant::now(),
        ) {
            self.file_scroll_remainder = 0.;
            self.file_scroll_axis = None;
        }
        if !command
            || event.touch_phase == TouchPhase::Cancelled
            || (event.delta.precise() && !self.file_scroll_gesture.command)
        {
            self.reset_file_scroll();
            return false;
        }
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.x), f32::from(delta.y)),
            ScrollDelta::Lines(delta) => (delta.x, delta.y),
        };
        let vertical = match self.file_scroll_axis {
            Some(FileScrollAxis::Vertical) => true,
            Some(FileScrollAxis::Horizontal) => false,
            None => dy.abs() >= dx.abs() && (dx != 0. || dy != 0.),
        };
        if !vertical {
            if dx.abs() > dy.abs() {
                self.file_scroll_axis = Some(FileScrollAxis::Horizontal);
            }
            return false;
        }
        self.file_scroll_axis = Some(FileScrollAxis::Vertical);
        match event.delta {
            ScrollDelta::Lines(_) => self.step_files_by_lines(dy, cx),
            ScrollDelta::Pixels(_) => self.step_files_by_pixels(dy, cx),
        }
        true
    }
    fn step_files_by_lines(&mut self, dy: f32, cx: &mut Context<Self>) {
        self.file_scroll_remainder = 0.;
        let steps = dy.round() as isize;
        if steps == 0 {
            return;
        }
        let direction = if steps < 0 { 1 } else { -1 };
        for _ in 0..steps.unsigned_abs() {
            self.move_file(direction, cx);
        }
    }
    fn step_files_by_pixels(&mut self, dy: f32, cx: &mut Context<Self>) {
        if dy != 0.
            && self.file_scroll_remainder != 0.
            && dy.signum() != self.file_scroll_remainder.signum()
        {
            self.file_scroll_remainder = 0.;
        }
        self.file_scroll_remainder += dy;
        while self.file_scroll_remainder <= -FILE_SCROLL_STEP {
            self.file_scroll_remainder += FILE_SCROLL_STEP;
            self.move_file(1, cx);
        }
        while self.file_scroll_remainder >= FILE_SCROLL_STEP {
            self.file_scroll_remainder -= FILE_SCROLL_STEP;
            self.move_file(-1, cx);
        }
    }
    #[cfg(feature = "ui-tests")]
    pub fn set_file_key_probe(&mut self, probe: fn(isize) -> bool) {
        self.file_key_probe = probe;
    }
    pub fn selected_file(&self) -> Option<&SharedString> {
        self.active_view().and_then(|v| v.selected.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phaseless_scroll_requires_an_idle_gap_before_switching_modes() {
        let mut gesture = FileScrollGesture::default();
        let start = Instant::now();
        gesture.update(true, TouchPhase::Moved, false, start);
        gesture.update(
            true,
            TouchPhase::Moved,
            true,
            start + Duration::from_millis(50),
        );
        assert!(
            !gesture.command,
            "Pressing Command cannot take over a text scroll"
        );
        gesture.update(
            true,
            TouchPhase::Moved,
            true,
            start + Duration::from_millis(250),
        );
        assert!(
            gesture.command,
            "Command can navigate after a 200ms idle gap"
        );
    }

    #[test]
    fn phased_scroll_keeps_its_mode_through_delayed_momentum() {
        let mut gesture = FileScrollGesture::default();
        let start = Instant::now();
        gesture.update(true, TouchPhase::Started, false, start);
        gesture.update(
            true,
            TouchPhase::Ended,
            false,
            start + Duration::from_millis(50),
        );
        gesture.update(
            true,
            TouchPhase::Moved,
            true,
            start + Duration::from_secs(1),
        );
        assert!(
            !gesture.command,
            "Momentum belongs to the original text gesture"
        );
        gesture.update(
            true,
            TouchPhase::Started,
            true,
            start + Duration::from_secs(2),
        );
        assert!(
            gesture.command,
            "A fresh Command gesture enables navigation"
        );
    }
}
