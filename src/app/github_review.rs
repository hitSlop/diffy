//! Repository-scoped remote review. Local checkout views and their watchers stay independent.
use super::{Diffy, review::ReviewView};
use crate::{
    github::{self, Backend, PullRequest, Remote, Repository},
    repository::{ChangeGroup, ChangedFile, Preview, PreviewKey},
};
use gpui_kit::component::{
    Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    popover::{Popover, PopoverState},
    tab::{Tab, TabBar},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    WeakEntity, Window, div, px,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

pub(super) struct Runtime {
    pub states: HashMap<PathBuf, State>,
    pub backend: Arc<dyn Backend>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            states: HashMap::new(),
            backend: Arc::new(github::OctocrabBackend),
        }
    }
}
pub(super) struct State {
    pub remote_mode: bool,
    pub remotes: Vec<Remote>,
    pub repository: Option<Repository>,
    pub resolving: bool,
    pub prs: Vec<PullRequest>,
    pub selected: Option<u64>,
    pub page: u32,
    pub more: bool,
    pub listing: bool,
    pub loading: bool,
    list_request: Option<ListRequest>,
    pub error: Option<String>,
    pub view: ReviewView,
    pub cache_root: Option<PathBuf>,
    list_generation: u64,
    diff_generation: u64,
    resolve_task: Option<Task<()>>,
    list_task: Option<Task<()>>,
    diff_task: Option<Task<()>>,
    menu_search: Entity<InputState>,
    subscriptions: Vec<Subscription>,
}
#[derive(Clone, Copy)]
enum ListRequest {
    Initial,
    Refresh,
    More,
}
impl State {
    pub(super) fn loading_message(&self) -> Option<&'static str> {
        if !self.remote_mode {
            return None;
        }
        match self.list_request {
            Some(ListRequest::Initial) => Some("Loading pull requests…"),
            Some(ListRequest::Refresh) => Some("Refreshing pull requests…"),
            Some(ListRequest::More) => Some("Loading more…"),
            None if self.loading => Some("Loading diff…"),
            None => None,
        }
    }
    fn loading_more(&self) -> bool {
        matches!(self.list_request, Some(ListRequest::More))
    }
    pub(super) fn add_subscription(&mut self, subscription: Subscription) {
        self.subscriptions.push(subscription);
    }
    fn new(window: &mut Window, cx: &mut Context<Diffy>) -> Self {
        let menu_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search pull requests"));
        let subscription = cx.subscribe(&menu_search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            remote_mode: false,
            remotes: Vec::new(),
            repository: None,
            resolving: true,
            prs: Vec::new(),
            selected: None,
            page: 0,
            more: false,
            listing: false,
            loading: false,
            list_request: None,
            error: None,
            view: ReviewView::new(cx),
            cache_root: None,
            list_generation: 0,
            diff_generation: 0,
            resolve_task: None,
            list_task: None,
            diff_task: None,
            menu_search,
            subscriptions: vec![subscription],
        }
    }
    pub(super) fn cancel(&mut self, cx: &mut Context<Diffy>) {
        self.list_generation += 1;
        self.diff_generation += 1;
        self.list_task = None;
        self.diff_task = None;
        self.listing = false;
        self.list_request = None;
        self.loading = false;
        self.view.suspend(cx);
    }
}
impl Diffy {
    pub(super) fn move_pr(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(s) = self.pr_state() else {
            return;
        };
        if s.prs.is_empty() {
            return;
        }
        let index = s
            .prs
            .iter()
            .position(|p| Some(p.number) == s.selected)
            .unwrap_or(0);
        let next = (index as isize + direction).rem_euclid(s.prs.len() as isize) as usize;
        self.select_pr(s.prs[next].number, false, cx);
    }
    pub(super) fn github_key(&self) -> Option<PathBuf> {
        let p = self.projects.get(self.active)?;
        p.repository.clone().or_else(|| p.path.clone())
    }
    pub(super) fn pr_state(&self) -> Option<&State> {
        self.github.states.get(&self.github_key()?)
    }
    pub(super) fn is_pr(&self) -> bool {
        self.pr_state().is_some_and(|s| s.remote_mode)
    }
    pub(super) fn active_view(&self) -> Option<&ReviewView> {
        if let Some(s) = self.pr_state().filter(|s| s.remote_mode) {
            Some(&s.view)
        } else {
            self.projects.get(self.active).map(|p| p.view())
        }
    }
    pub(super) fn active_view_mut(&mut self) -> Option<&mut ReviewView> {
        if self.is_pr() {
            let key = self.github_key()?;
            self.github.states.get_mut(&key).map(|s| &mut s.view)
        } else {
            self.projects.get_mut(self.active).map(|p| p.view_mut())
        }
    }
    pub(super) fn ensure_github_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.github_key() else {
            return;
        };
        if self.github.states.contains_key(&key) || self.projects[self.active].loading {
            return;
        }
        let Some(path) = self.projects[self.active].path.clone() else {
            return;
        };
        let mut state = State::new(window, cx);
        let request_key = key.clone();
        state.resolve_task = Some(cx.spawn(async move |owner, cx| {
            let result = cx
                .background_spawn(async move { github::remotes(&path) })
                .await;
            let _ = owner.update(cx, |this, cx| {
                let Some(s) = this.github.states.get_mut(&request_key) else {
                    return;
                };
                s.resolving = false;
                s.resolve_task = None;
                match result {
                    Ok(remotes) => {
                        s.repository = remotes
                            .first()
                            .filter(|r| r.name == "origin" || remotes.len() == 1)
                            .map(|r| r.repository.clone());
                        s.remotes = remotes;
                    }
                    Err(error) => s.error = Some(error),
                }
                cx.notify();
            });
        }));
        self.github.states.insert(key, state);
    }
    pub(super) fn leave_github(&mut self, cx: &mut Context<Self>) {
        if let Some(key) = self.github_key()
            && let Some(s) = self.github.states.get_mut(&key)
        {
            s.cancel(cx);
        }
    }
    pub(super) fn set_review_source(
        &mut self,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_file_repeat();
        self.reset_file_scroll();
        let Some(key) = self.github_key() else {
            return;
        };
        if remote
            && self
                .pr_state()
                .is_none_or(|s| s.resolving || s.remotes.is_empty())
        {
            return;
        }
        if remote {
            if let Some(view) = self.active_view_mut() {
                view.suspend(cx);
            }
        } else {
            self.leave_github(cx);
        }
        if let Some(s) = self.github.states.get_mut(&key) {
            s.remote_mode = remote;
        }
        if remote {
            if self.pr_state().is_some_and(|s| s.page == 0) {
                self.list_prs(false, cx);
            } else {
                self.ensure_pr_preview(cx);
            }
        } else {
            self.ensure_preview(cx);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn list_prs(&mut self, more: bool, cx: &mut Context<Self>) {
        let Some(key) = self.github_key() else {
            return;
        };
        let Some(s) = self.github.states.get_mut(&key) else {
            return;
        };
        let Some(repo) = s.repository.clone() else {
            return;
        };
        if s.listing {
            return;
        }
        let page = if more { s.page + 1 } else { 1 };
        let last_page = if more { page } else { s.page.max(1) };
        s.list_generation += 1;
        let generation = s.list_generation;
        s.listing = true;
        s.list_request = Some(if more {
            ListRequest::More
        } else if s.page == 0 {
            ListRequest::Initial
        } else {
            ListRequest::Refresh
        });
        s.error = None;
        let backend = self.github.backend.clone();
        s.list_task = Some(cx.spawn(async move |owner, cx| {
            let result = cx
                .background_spawn(async move {
                    let mut combined = github::Page {
                        items: Vec::new(),
                        more: false,
                    };
                    let mut loaded_page = page;
                    for current in page..=last_page {
                        let result = backend.list(&repo, current)?;
                        combined.items.extend(result.items);
                        combined.more = result.more;
                        loaded_page = current;
                        if !combined.more {
                            break;
                        }
                    }
                    Ok::<_, String>((combined, loaded_page))
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                if this.github_key().as_ref() != Some(&key) {
                    return;
                }
                let Some(s) = this.github.states.get_mut(&key) else {
                    return;
                };
                if s.list_generation != generation || !s.remote_mode {
                    return;
                }
                s.listing = false;
                s.list_request = None;
                s.list_task = None;
                match result {
                    Ok((result, loaded_page)) => {
                        if !more {
                            s.prs.clear();
                        }
                        for pr in result.items {
                            if !s.prs.iter().any(|p| p.number == pr.number) {
                                s.prs.push(pr);
                            }
                        }
                        s.page = loaded_page;
                        s.more = result.more;
                        let selected = s
                            .selected
                            .filter(|n| s.prs.iter().any(|p| p.number == *n))
                            .or_else(|| s.prs.first().map(|p| p.number));
                        if let Some(number) = selected {
                            this.select_pr(number, !more, cx);
                        } else {
                            s.selected = None;
                            s.cache_root = None;
                            s.view.clear(cx);
                        }
                    }
                    Err(error) => s.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn select_pr(&mut self, number: u64, force: bool, cx: &mut Context<Self>) {
        self.stop_file_repeat();
        self.reset_file_scroll();
        let Some(key) = self.github_key() else {
            return;
        };
        let Some(s) = self.github.states.get_mut(&key) else {
            return;
        };
        let Some(repo) = s.repository.clone() else {
            return;
        };
        if !s.prs.iter().any(|p| p.number == number) {
            return;
        }
        let changed = s.selected != Some(number);
        if !changed && !force && s.cache_root.is_some() {
            self.ensure_pr_preview(cx);
            return;
        }
        if changed {
            s.view.clear(cx);
            s.cache_root = None;
        }
        s.selected = Some(number);
        s.loading = true;
        s.error = None;
        s.diff_generation += 1;
        let generation = s.diff_generation;
        let backend = self.github.backend.clone();
        s.diff_task = Some(cx.spawn(async move |owner, cx| {
            let request_repo = repo.clone();
            let result = cx
                .background_spawn(async move { backend.diff(&request_repo, number) })
                .await;
            let _ = owner.update(cx, |this, cx| {
                if this.github_key().as_ref() != Some(&key) {
                    return;
                }
                let Some(s) = this.github.states.get_mut(&key) else {
                    return;
                };
                if s.diff_generation != generation
                    || !s.remote_mode
                    || s.selected != Some(number)
                    || s.repository.as_ref() != Some(&repo)
                {
                    return;
                }
                s.loading = false;
                s.diff_task = None;
                match result {
                    Ok(result) => {
                        let root = result.pr.cache_root(&repo);
                        if let Some(pr) = s.prs.iter_mut().find(|p| p.number == number) {
                            *pr = result.pr;
                        }
                        // Pin the selected file even if the PR exceeds the cache's slot count.
                        let preferred = s.view.selected.clone();
                        let selected_preview = result
                            .snapshot
                            .files
                            .iter()
                            .find(|f| preferred.as_ref() == Some(f.path()))
                            .or_else(|| result.snapshot.files.first())
                            .map(|file| {
                                let revision = result.snapshot.revisions[file.path()];
                                (
                                    Preview {
                                        file: file.clone(),
                                        fingerprint: revision,
                                        bytes: file.estimated_size_bytes(),
                                    },
                                    revision,
                                )
                            });
                        let mut cache =
                            this.preview_cache.lock().unwrap_or_else(|e| e.into_inner());
                        let entries = result
                            .snapshot
                            .files
                            .into_iter()
                            .map(|file| {
                                let revision = result.snapshot.revisions[file.path()];
                                let entry = ChangedFile::from_diff(&file, revision);
                                cache.insert(
                                    PreviewKey {
                                        root: root.clone(),
                                        group: ChangeGroup::Changes,
                                        path: file.path().clone(),
                                        revision,
                                    },
                                    Preview {
                                        bytes: file.estimated_size_bytes(),
                                        fingerprint: revision,
                                        file,
                                    },
                                );
                                entry
                            })
                            .collect();
                        drop(cache);
                        s.cache_root = Some(root);
                        s.view.install_metadata(entries, number, cx);
                        if let Some((preview, revision)) = selected_preview {
                            s.view.install_preview(preview, revision, cx);
                        }
                    }
                    Err(error) => s.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub(super) fn ensure_pr_preview(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.github_key() else {
            return;
        };
        let Some(s) = self.github.states.get_mut(&key) else {
            return;
        };
        if !s.remote_mode {
            return;
        }
        if s.cache_root.is_none()
            && !s.loading
            && s.error.is_none()
            && let Some(number) = s.selected
        {
            self.select_pr(number, true, cx);
            return;
        }
        let Some(entry) = s.view.selected_entry().cloned() else {
            return;
        };
        if s.view.preview_revision == Some(entry.revision) {
            return;
        }
        if let Some(root) = s.cache_root.clone() {
            let cached = self
                .preview_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&PreviewKey {
                    root,
                    group: ChangeGroup::Changes,
                    path: entry.path.clone(),
                    revision: entry.revision,
                });
            if let Some(preview) = cached {
                s.view.install_preview(preview, entry.revision, cx);
            } else if !s.loading
                && let Some(number) = s.selected
            {
                self.select_pr(number, true, cx);
            }
        }
    }
    fn change_remote(&mut self, repo: Repository, cx: &mut Context<Self>) {
        let Some(key) = self.github_key() else {
            return;
        };
        let Some(s) = self.github.states.get_mut(&key) else {
            return;
        };
        s.cancel(cx);
        s.repository = Some(repo);
        s.prs.clear();
        s.selected = None;
        s.page = 0;
        s.more = false;
        s.cache_root = None;
        s.view.clear(cx);
        self.list_prs(false, cx);
        cx.notify();
    }
    pub(super) fn pr_empty_message(&self) -> &'static str {
        let Some(s) = self.pr_state() else {
            return "Pull requests unavailable";
        };
        if s.resolving {
            "Reading GitHub remotes…"
        } else if s.repository.is_none() {
            "Choose a GitHub remote from the pull request menu"
        } else if s.loading {
            "Loading pull request diff…"
        } else if s.listing {
            "Loading pull requests…"
        } else if s.error.is_some() {
            "Pull requests unavailable · Retry or open GitHub"
        } else if s.selected.is_some() {
            "No changes in this pull request"
        } else {
            "No open pull requests"
        }
    }
    pub(super) fn open_pr_browser(&self, cx: &mut Context<Self>) {
        if let Some(s) = self.pr_state()
            && let Some(repo) = &s.repository
        {
            cx.open_url(&s.selected.map_or_else(|| repo.url(), |n| repo.pr_url(n)));
        }
    }
    pub(super) fn render_pr_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = self.pr_state() else {
            return div().into_any_element();
        };
        let selected = s
            .prs
            .iter()
            .position(|p| Some(p.number) == s.selected)
            .unwrap_or(0);
        let mut tabs = TabBar::new("pr-tabs")
            .underline()
            .small()
            .max_width(px(180.))
            .selected_index(selected)
            .min_w_0()
            .flex_1()
            .h(px(32.));
        for pr in &s.prs {
            let title = pr.title.clone();
            let tooltip = pr.tooltip();
            tabs = tabs.child(
                Tab::new()
                    .prefix(
                        div()
                            .flex_shrink_0()
                            .mr_1()
                            .child(format!("#{}", pr.number)),
                    )
                    .label(title)
                    .tooltip(move |w, cx| {
                        Tooltip::new(tooltip.clone())
                            .max_w(px(640.))
                            .whitespace_normal()
                            .build(w, cx)
                    }),
            );
        }
        let numbers: Vec<_> = s.prs.iter().map(|p| p.number).collect();
        let owner = cx.entity().downgrade();
        h_flex()
            .min_w_0()
            .flex_1()
            .child(tabs.on_click(cx.listener(move |this, ix: &usize, _, cx| {
                if let Some(&n) = numbers.get(*ix) {
                    this.select_pr(n, false, cx);
                }
            })))
            .child(
                Popover::new("pr-menu")
                    .trigger(
                        Button::new("pr-overflow")
                            .ghost()
                            .xsmall()
                            .label("…")
                            .accessibility_label("Find pull request or choose remote"),
                    )
                    .content(move |_, window, cx| {
                        let popup = cx.entity().downgrade();
                        owner
                            .update(cx, |this, cx| this.render_pr_menu(popup, window, cx))
                            .unwrap_or_else(|_| div().into_any_element())
                    }),
            )
            .into_any_element()
    }
    fn render_pr_menu(
        &self,
        popup: WeakEntity<PopoverState>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(s) = self.pr_state() else {
            return div().into_any_element();
        };
        let query = s.menu_search.read(cx).value().to_lowercase();
        let items: Vec<_> = s
            .prs
            .iter()
            .filter(|p| {
                format!("#{} {}", p.number, p.title)
                    .to_lowercase()
                    .contains(query.as_str())
            })
            .cloned()
            .collect();
        v_flex()
            .w(px(420.))
            .max_w_full()
            .gap_2()
            .p_2()
            .children(s.remotes.iter().map(|remote| {
                let repo = remote.repository.clone();
                Button::new(format!("github-remote-{}", remote.name))
                    .ghost()
                    .small()
                    .label(format!("{} · {}", remote.name, repo.label()))
                    .selected(s.repository.as_ref() == Some(&repo))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.change_remote(repo.clone(), cx)),
                    )
            }))
            .child(Input::new(&s.menu_search).id("pr-search").small())
            .child(
                v_flex()
                    .id("pr-options")
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .children(items.into_iter().map(|pr| {
                        let popup = popup.clone();
                        Button::new(format!("choose-pr-{}", pr.number))
                            .ghost()
                            .small()
                            .w_full()
                            .label(format!(
                                "#{} {}{}",
                                pr.number,
                                pr.title,
                                if pr.draft { " · Draft" } else { "" }
                            ))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_pr(pr.number, false, cx);
                                let _ = popup.update(cx, |p, cx| p.dismiss(window, cx));
                            }))
                    })),
            )
            .when(s.more, |d| {
                d.child(
                    Button::new("pr-load-more")
                        .ghost()
                        .small()
                        .label(if s.loading_more() {
                            "Loading more…"
                        } else {
                            "Load more"
                        })
                        .loading(s.loading_more())
                        .disabled(s.listing)
                        .on_click(cx.listener(|this, _, _, cx| this.list_prs(true, cx))),
                )
            })
            .into_any_element()
    }
}

#[cfg(feature = "ui-tests")]
impl Diffy {
    pub fn set_github_fixture(
        &mut self,
        remotes: Vec<Remote>,
        backend: Arc<dyn Backend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = self
            .github_key()
            .unwrap_or_else(|| PathBuf::from("demo-github"));
        let mut s = State::new(window, cx);
        s.resolving = false;
        s.repository = remotes.first().map(|r| r.repository.clone());
        s.remotes = remotes;
        self.github.backend = backend;
        self.github.states.insert(key, s);
    }
    pub fn probe_pr_writes(&mut self, cx: &mut Context<Self>) {
        self.stage_file(cx);
        self.stage_all(cx);
        if let Some(p) = self.projects.get(self.active) {
            let id = p.id;
            if let Some(file) = p.view().selected_entry().cloned() {
                self.stage_path(id, ChangeGroup::Changes, &file.path, cx);
                self.load(Some((vec![file], true)), cx);
            }
        }
    }
    pub fn github_selected(&self) -> Option<u64> {
        self.pr_state().and_then(|s| s.selected)
    }
    pub fn github_busy(&self) -> bool {
        self.pr_state().is_some_and(|s| s.listing || s.loading)
    }
}
