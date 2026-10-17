//! Repository projects share rail appearance; checkout runtimes stay independent.
use super::{Diffy, session::Project};
use crate::{repository, state, watch};
use gpui_kit::component::{
    Disableable as _, InteractiveElementExt as _, Selectable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    tab::{Tab, TabBar},
    tooltip::Tooltip,
};
use gpui_kit::{
    AnyElement, AppContext as _, Context, InteractiveElement as _, IntoElement, MouseButton,
    ObjectFit, ParentElement as _, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Task, Window, div, img, px, rgb,
};
use std::{cell::Cell, path::PathBuf, rc::Rc};

pub(super) struct RepositoryProject {
    pub common_dir: PathBuf,
    pub checkouts: Vec<PathBuf>,
    pub active_checkout: Option<PathBuf>,
    pub name: gpui_kit::SharedString,
    pub color: u32,
    pub icon: Option<state::ProjectIcon>,
    source: u64,
    explicit: bool,
    discovered: bool,
    refreshing: bool,
    last_request: u64,
    pending: bool,
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<Task<()>>,
    scroll: ScrollHandle,
}
impl RepositoryProject {
    pub fn mark_appearance_explicit(&mut self) {
        self.explicit = true;
    }
    pub fn restore(saved: state::SavedRepositoryProject) -> Self {
        Self {
            common_dir: saved.common_dir,
            checkouts: saved.checkouts,
            active_checkout: saved.active_checkout,
            name: saved.name.into(),
            color: saved.color,
            icon: saved.icon,
            source: u64::MAX,
            explicit: true,
            discovered: false,
            refreshing: false,
            last_request: 0,
            pending: false,
            watcher: None,
            watch_task: None,
            scroll: ScrollHandle::new(),
        }
    }
    pub fn saved(&self) -> state::SavedRepositoryProject {
        state::SavedRepositoryProject {
            common_dir: self.common_dir.clone(),
            checkouts: self.checkouts.clone(),
            active_checkout: self.active_checkout.clone(),
            name: self.name.to_string(),
            color: self.color,
            icon: self.icon.clone(),
        }
    }
}

impl Diffy {
    pub(super) fn repository_project(&self, p: &Project) -> Option<&RepositoryProject> {
        self.repositories
            .iter()
            .find(|r| Some(&r.common_dir) == p.repository.as_ref())
    }
    pub(super) fn appearance<'a>(
        &'a self,
        p: &'a Project,
    ) -> (
        &'a gpui_kit::SharedString,
        u32,
        &'a Option<state::ProjectIcon>,
    ) {
        self.repository_project(p)
            .map_or((&p.name, p.color, &p.icon), |r| (&r.name, r.color, &r.icon))
    }
    pub(super) fn same_repository(&self, a: usize, b: usize) -> bool {
        let (Some(a), Some(b)) = (self.projects.get(a), self.projects.get(b)) else {
            return false;
        };
        a.id == b.id || (a.repository.is_some() && a.repository == b.repository)
    }
    pub(super) fn project_rows(&self) -> Vec<usize> {
        let mut rows = Vec::new();
        for (ix, p) in self.projects.iter().enumerate() {
            if rows.iter().any(|&other| self.same_repository(other, ix)) {
                continue;
            }
            let selected = self.repository_project(p).and_then(|r| {
                self.projects.iter().position(|p| {
                    p.repository.as_ref() == Some(&r.common_dir)
                        && p.path == r.active_checkout
                        && p.available
                })
            });
            rows.push(selected.unwrap_or(ix));
        }
        rows
    }
    pub(super) fn worktree_indices(&self, ix: usize) -> Vec<usize> {
        let mut indices = (0..self.projects.len())
            .filter(|&other| self.same_repository(ix, other))
            .collect::<Vec<_>>();
        indices.sort_by_key(|&ix| {
            let p = &self.projects[ix];
            (
                !p.main_worktree,
                self.tab_label(ix).to_lowercase(),
                p.path.clone(),
            )
        });
        indices
    }
    fn tab_label(&self, ix: usize) -> String {
        let p = &self.projects[ix];
        p.custom_name.clone().unwrap_or_else(|| {
            let folder = p
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.name.to_string());
            if p.detached_worktree {
                format!("{folder} (detached)")
            } else if p.branch.as_ref() != "…" && p.branch.as_ref() != "Unavailable" {
                p.branch.to_string()
            } else {
                folder
            }
        })
    }
    pub(super) fn worktree_label(&self, ix: usize) -> String {
        let label = self.tab_label(ix);
        let duplicate = self
            .worktree_indices_without_sort(ix)
            .any(|other| other != ix && self.tab_label(other) == label);
        if duplicate {
            let parent = self.projects[ix]
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            format!("{label} · {parent}")
        } else {
            label
        }
    }
    fn worktree_indices_without_sort(&self, ix: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.projects.len()).filter(move |&other| self.same_repository(ix, other))
    }
    pub(super) fn repository_changed_count(&self, ix: usize) -> usize {
        self.worktree_indices_without_sort(ix)
            .map(|ix| self.projects[ix].changed_count)
            .sum()
    }
    pub(super) fn repository_mutating(&self, ix: usize) -> bool {
        self.worktree_indices_without_sort(ix)
            .any(|ix| self.projects[ix].mutating)
    }
    pub(super) fn remember_worktree(&mut self) {
        if let Some(p) = self.projects.get(self.active)
            && let Some(r) = self
                .repositories
                .iter_mut()
                .find(|r| Some(&r.common_dir) == p.repository.as_ref())
        {
            r.active_checkout = p.path.clone();
        }
    }
    pub(super) fn reveal_worktree(&self) {
        if let Some(p) = self.projects.get(self.active)
            && let Some(r) = self.repository_project(p)
            && let Some(position) = self
                .worktree_indices(self.active)
                .iter()
                .position(|&ix| ix == self.active)
        {
            r.scroll.scroll_to_item(position);
        }
    }
    pub(super) fn move_worktree(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_pr() {
            self.move_pr(direction, cx);
            return;
        }
        let indices = self
            .worktree_indices(self.active)
            .into_iter()
            .filter(|&ix| self.projects[ix].available)
            .collect::<Vec<_>>();
        if indices.is_empty() {
            return;
        }
        let current = indices
            .iter()
            .position(|&ix| ix == self.active)
            .unwrap_or(0);
        let next = (current as isize + direction).rem_euclid(indices.len() as isize) as usize;
        self.switch_project(indices[next], window, cx);
    }
    pub(super) fn render_worktrees(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let indices = self.worktree_indices(self.active);
        let project = self.projects.get(self.active)?;
        let (name, color, icon) = self.appearance(project);
        let selected = indices
            .iter()
            .position(|&ix| ix == self.active)
            .unwrap_or(0);
        let mut bar = TabBar::new("worktree-tabs")
            .underline()
            .small()
            .menu(indices.len() > 1)
            .max_width(px(180.))
            .selected_index(selected)
            .w_full()
            .h(px(32.))
            .flex_none();
        if let Some(r) = self
            .projects
            .get(self.active)
            .and_then(|p| self.repository_project(p))
        {
            bar = bar.track_scroll(&r.scroll);
        }
        for (position, &ix) in indices.iter().enumerate() {
            let p = &self.projects[ix];
            let label = if position == 0 && p.branch.as_ref() != "…" {
                p.branch.to_string()
            } else {
                self.worktree_label(ix)
            };
            let tooltip = format!(
                "{}\n{}{}{}",
                p.path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                p.branch,
                if p.locked_worktree { " · locked" } else { "" },
                if p.available { "" } else { " · unavailable" }
            );
            let tab = Tab::new()
                .label(label)
                .disabled(!p.available)
                .tooltip(move |window, cx| {
                    Tooltip::new(tooltip.clone())
                        .max_w(px(640.))
                        .whitespace_normal()
                        .build(window, cx)
                });
            bar = bar.child(tab);
        }
        let content = match icon {
            Some(state::ProjectIcon::Image(path)) => img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            Some(state::ProjectIcon::Emoji(emoji)) => div().child(emoji.clone()).into_any_element(),
            None => div()
                .child(
                    name.chars()
                        .next()
                        .unwrap_or('D')
                        .to_uppercase()
                        .to_string(),
                )
                .into_any_element(),
        };
        let identity = h_flex()
            .gap_2()
            .px_2()
            .max_w(px(180.))
            .min_w_0()
            .flex_shrink_0()
            .child(
                div()
                    .size(px(18.))
                    .flex_shrink_0()
                    .rounded(px(5.))
                    .overflow_hidden()
                    .bg(rgb(color))
                    .text_color(rgb(0x141618))
                    .text_size(px(11.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(content),
            )
            .child(div().truncate().font_medium().child(name.clone()));
        let remote = self.is_pr();
        let ready = self
            .pr_state()
            .is_some_and(|s| !s.resolving && !s.remotes.is_empty());
        let source = h_flex()
            .flex_shrink_0()
            .gap_1()
            .child(
                Button::new("source-worktrees")
                    .ghost()
                    .xsmall()
                    .label("Worktrees")
                    .selected(!remote)
                    .toggled(!remote)
                    .on_click(cx.listener(|this, _, w, cx| this.set_review_source(false, w, cx))),
            )
            .child(
                Button::new("source-prs")
                    .ghost()
                    .xsmall()
                    .label("Pull requests")
                    .selected(remote)
                    .toggled(remote)
                    .disabled(!ready)
                    .tooltip(if ready {
                        "Review open GitHub pull requests"
                    } else {
                        "No GitHub remote available · Refresh after adding one"
                    })
                    .on_click(cx.listener(|this, _, w, cx| this.set_review_source(true, w, cx))),
            );
        // Wait for pointer movement before handing the gesture to macOS, so
        // ordinary clicks still reach the tabs and their overflow menu.
        let dragging = Rc::new(Cell::new(false));
        let down = dragging.clone();
        let outside = dragging.clone();
        let up = dragging.clone();
        Some(
            div()
                .id("project-header")
                .w_full()
                .h(px(32.))
                .flex_none()
                .on_mouse_down(MouseButton::Left, move |_, _, _| down.set(true))
                .on_mouse_down_out(move |_, _, _| outside.set(false))
                .on_mouse_up(MouseButton::Left, move |_, _, _| up.set(false))
                .on_mouse_move(move |_, window, _| {
                    if dragging.replace(false) {
                        window.start_window_move();
                    }
                })
                .on_double_click(|_, window, _| window.titlebar_double_click())
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .h_full()
                        .gap_2()
                        .child(identity)
                        .child(source)
                        .child(if remote {
                            self.render_pr_tabs(cx)
                        } else {
                            bar.min_w_0()
                                .flex_1()
                                .on_click(cx.listener(move |this, selected: &usize, window, cx| {
                                    if let Some(&ix) = indices.get(*selected) {
                                        this.switch_project(ix, window, cx);
                                    }
                                }))
                                .into_any_element()
                        }),
                )
                .into_any_element(),
        )
    }

    pub(super) fn discover_worktrees(&mut self, id: u64, force: bool, cx: &mut Context<Self>) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        if self.projects[ix].topology_task.is_some() {
            self.projects[ix].pending_topology |= force;
            return;
        }
        let Some(mut path) = self.projects[ix].path.clone() else {
            return;
        };
        self.topology_sequence += 1;
        let request = self.topology_sequence;
        if let Some(r) = self
            .repositories
            .iter_mut()
            .find(|r| Some(&r.common_dir) == self.projects[ix].repository.as_ref())
        {
            if r.refreshing {
                r.pending |= force;
                return;
            }
            if r.discovered && !force {
                return;
            }
            path = r.common_dir.clone();
            r.refreshing = true;
            r.last_request = request;
        }
        self.projects[ix].topology_task = Some(cx.spawn(async move |owner, cx| {
            let result = cx
                .background_spawn(async move {
                    let _slot = repository::refresh_slot().await;
                    repository::list_worktrees(&path)
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                let Some(ix) = this.projects.iter().position(|p| p.id == id) else {
                    return;
                };
                this.projects[ix].topology_task = None;
                let common = this.projects[ix].repository.clone();
                let mut again = std::mem::take(&mut this.projects[ix].pending_topology);
                if let Some(r) = this
                    .repositories
                    .iter_mut()
                    .find(|r| Some(&r.common_dir) == common.as_ref())
                {
                    if request < r.last_request {
                        r.pending |= again;
                        return;
                    }
                    if request == r.last_request {
                        r.refreshing = false;
                        again |= std::mem::take(&mut r.pending);
                    }
                }
                match result {
                    Ok(listing) => this.install_worktrees(id, request, listing, cx),
                    Err(error) if force => {
                        this.projects[ix].error =
                            Some(format!("Couldn't refresh worktrees: {error}"))
                    }
                    Err(_) => {} // The regular metadata read supplies the actionable repository error.
                }
                if again {
                    this.discover_worktrees(id, true, cx);
                }
                this.persist(cx);
                cx.notify();
            });
        }));
    }

    fn install_worktrees(
        &mut self,
        id: u64,
        request: u64,
        listing: repository::WorktreeListing,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        let common = listing.common_dir;
        self.import
            .preferences
            .excluded_repositories
            .remove(&common);
        let active_id = self.active_project_id();
        let appearance_source = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.path
                    .as_ref()
                    .is_some_and(|path| listing.worktrees.iter().any(|w| &w.path == path))
            })
            .min_by_key(|(_, p)| p.id)
            .map(|(ix, _)| ix)
            .unwrap_or(source);
        let rix = if let Some(ix) = self
            .repositories
            .iter()
            .position(|r| r.common_dir == common)
        {
            ix
        } else {
            let p = &self.projects[appearance_source];
            let mut r = RepositoryProject::restore(state::SavedRepositoryProject {
                common_dir: common.clone(),
                checkouts: vec![],
                active_checkout: p.path.clone(),
                name: p
                    .custom_name
                    .clone()
                    .unwrap_or_else(|| listing.name.clone()),
                color: p.color,
                icon: p.icon.clone(),
            });
            r.source = p.id;
            r.explicit = false;
            self.repositories.push(r);
            self.repositories.len() - 1
        };
        // Completion order must not decide which legacy appearance wins.
        if !self.repositories[rix].explicit
            && self.projects[appearance_source].id < self.repositories[rix].source
        {
            let p = &self.projects[appearance_source];
            let r = &mut self.repositories[rix];
            r.source = p.id;
            r.name = p
                .custom_name
                .clone()
                .unwrap_or_else(|| listing.name.clone())
                .into();
            r.icon = p.icon.clone();
            r.color = p.color;
        }
        self.projects[source].repository = Some(common.clone());
        self.repositories[rix].discovered = true;
        self.repositories[rix].last_request = request;
        let mut paths = listing
            .worktrees
            .iter()
            .map(|w| w.path.clone())
            .collect::<Vec<_>>();
        // An index write keeps its runtime until it finishes, even if the checkout disappears.
        for p in &self.projects {
            if p.repository.as_ref() == Some(&common)
                && p.mutating
                && let Some(path) = &p.path
                && !paths.contains(path)
            {
                paths.push(path.clone());
            }
        }
        self.repositories[rix].checkouts = paths.clone();
        self.projects.retain(|p| {
            p.repository.as_ref() != Some(&common)
                || p.path.as_ref().is_some_and(|p| paths.contains(p))
        });
        for info in listing.worktrees {
            let ix = if let Some(ix) = self
                .projects
                .iter()
                .position(|p| p.path.as_ref() == Some(&info.path))
            {
                ix
            } else {
                self.add_path(info.path.clone(), cx);
                self.projects.len() - 1
            };
            let p = &mut self.projects[ix];
            p.repository = Some(common.clone());
            p.available = info.available;
            p.main_worktree = info.main;
            p.locked_worktree = info.locked;
            p.detached_worktree = info.branch.ends_with(" (detached)");
            if p.branch.as_ref() == "…" {
                p.branch = info.branch.clone().into();
            }
            if !info.available {
                p.branch = info.branch.into();
                p.error = Some(
                    "This worktree is unavailable. Refresh after reconnecting or repairing it."
                        .into(),
                );
            }
        }
        self.active = active_id
            .and_then(|id| self.projects.iter().position(|p| p.id == id))
            .unwrap_or_else(|| {
                self.projects
                    .iter()
                    .position(|p| {
                        p.repository.as_ref() == Some(&common) && p.available && p.main_worktree
                    })
                    .or_else(|| {
                        self.projects
                            .iter()
                            .position(|p| p.repository.as_ref() == Some(&common) && p.available)
                    })
                    .unwrap_or(0)
            });
        if self
            .projects
            .get(self.active)
            .is_some_and(|p| p.repository.as_ref() == Some(&common) && !p.available)
            && let Some(ix) = self
                .worktree_indices(self.active)
                .into_iter()
                .find(|&ix| self.projects[ix].available)
        {
            self.active = ix;
        }
        if self.same_repository(
            self.active,
            self.projects
                .iter()
                .position(|p| p.id == id)
                .unwrap_or(self.active),
        ) {
            self.remember_worktree();
        }
        self.hovered_project = None;
        self.ensure_preview(cx);
        self.reveal_worktree();
        if active_id != self.active_project_id() {
            // The old checkout's focus handle disappears with its runtime.
            // Restore the app scope so keyboard actions reach the new checkout.
            let window = self.window_handle;
            let focus = self.focus.clone();
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| focus.focus(window, cx));
            });
        }
        self.start_topology_watch(common, cx);
    }
    fn start_topology_watch(&mut self, common: PathBuf, cx: &mut Context<Self>) {
        let Some(r) = self
            .repositories
            .iter_mut()
            .find(|r| r.common_dir == common)
        else {
            return;
        };
        if r.watch_task.is_some() {
            return;
        }
        r.watch_task = Some(cx.spawn(async move |owner, cx| {
            let root = common.clone();
            let result = cx.background_spawn(async move { watch::start_topology(&root) }).await;
            let rx = match result {
                Ok((watcher, rx)) => {
                    let installed = owner.update(cx, |this, cx| {
                        if let Some(r) = this.repositories.iter_mut().find(|r| r.common_dir == common) {
                            r.watcher = Some(watcher);
                            // Reconcile changes between enumeration and watcher attachment.
                            if let Some(id) = this.projects.iter().find(|p| p.available && p.repository.as_ref() == Some(&common)).map(|p| p.id) {
                                this.discover_worktrees(id, true, cx);
                            }
                            true
                        } else { false }
                    }).unwrap_or(false);
                    if !installed { return; } rx
                },
                Err(error) => {
                    let _ = owner.update(cx, |this, cx| {
                        if let Some(r) = this.repositories.iter_mut().find(|r| r.common_dir == common) { r.watch_task = None; }
                        if let Some(p) = this.projects.iter_mut().find(|p| p.repository.as_ref() == Some(&common)) {
                            p.error = Some(format!("Live worktree discovery unavailable: {error}. Use Refresh to retry."));
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            while rx.recv().await.is_ok() {
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
                while rx.try_recv().is_ok() {}
                if owner.update(cx, |this, cx| {
                    if let Some(id) = this.projects.iter().filter(|p| p.available).find(|p| p.repository.as_ref() == Some(&common)).map(|p| p.id) {
                        this.discover_worktrees(id, true, cx);
                    }
                }).is_err() { break; }
            }
        }));
    }
    pub(super) fn refresh_topologies(&mut self, cx: &mut Context<Self>) {
        let ids = self
            .project_rows()
            .into_iter()
            .filter(|&ix| self.projects[ix].available)
            .map(|ix| self.projects[ix].id)
            .collect::<Vec<_>>();
        for id in ids {
            self.discover_worktrees(id, true, cx);
        }
    }
    #[cfg(feature = "ui-tests")]
    pub fn worktree_paths(&self) -> Vec<PathBuf> {
        self.worktree_indices(self.active)
            .into_iter()
            .filter_map(|ix| self.projects[ix].path.clone())
            .collect()
    }
    #[cfg(feature = "ui-tests")]
    pub fn worktrees_ready(&self) -> bool {
        self.projects
            .get(self.active)
            .and_then(|p| self.repository_project(p))
            .is_some_and(|r| r.discovered && !r.refreshing)
            && self
                .projects
                .iter()
                .all(|p| !p.loading && !p.refreshing && p.topology_task.is_none())
    }
}
