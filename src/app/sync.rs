use super::Diffy;
use crate::repository;
use crate::repository::ChangeGroup;
use crate::repository::{ChangedFile, PreviewKey};
use crate::watch;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::SharedString;
use std::path::PathBuf;

impl Diffy {
    pub(super) fn load(
        &mut self,
        mutation: Option<(Vec<ChangedFile>, bool)>,
        cx: &mut Context<Self>,
    ) {
        if mutation.is_some() && (self.updates.blocks_mutations() || self.is_pr()) {
            return;
        }
        if mutation.is_some()
            && let Some(id) = self.active_project_id()
        {
            self.invalidate_project(id, None);
        }
        let ix = self.active;
        let Some(p) = self.projects.get_mut(ix) else {
            return;
        };
        let id = p.id;
        if p.mutating {
            return;
        }
        let Some(path) = p.path.clone() else {
            return;
        };
        // Once a snapshot is on screen, another read must not dim Stage all and
        // Refresh. Those buttons follow `loading`. A stage click still takes
        // the visible path and supersedes the quiet read.
        let visible = mutation.is_some() || !p.views.iter().any(|view| view.loaded());
        if mutation.is_none() && (p.loading || p.refreshing) {
            p.pending_refresh = true;
            return;
        }
        p.generation += 1;
        let generation = p.generation;
        let metadata_context = p.metadata_context.clone();
        p.loading = visible;
        p.refreshing = !visible;
        p.mutating = mutation.is_some();
        if visible {
            p.error = None;
        }
        p.task = Some(cx.spawn(async move |this, cx| {
            let slot = repository::refresh_slot().await;
            if !this
                .update(cx, |this, _| {
                    this.projects
                        .iter()
                        .any(|p| p.id == id && p.generation == generation)
                })
                .unwrap_or(false)
            {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    // Keep the slot until the read ends even if the UI task is superseded.
                    let _slot = slot;
                    let mutation_error = mutation.and_then(|(file, stage)| {
                        repository::set_entries_staged(&path, &file, stage).err()
                    });
                    (
                        repository::load_metadata_with_context(&path, metadata_context.as_ref()),
                        mutation_error,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(ix) = this.projects.iter().position(|p| p.id == id) else {
                    return;
                };
                if this.projects[ix].generation != generation {
                    return;
                }
                if let Ok(snapshot) = &result.0 {
                    this.import.preferences.excluded.remove(&snapshot.root);
                }
                if let Ok(snapshot) = &result.0
                    && let Some(existing) = this
                        .projects
                        .iter()
                        .position(|p| p.id != id && p.path.as_ref() == Some(&snapshot.root))
                {
                    let existing_id = this.projects[existing].id;
                    let was_active = this.active == ix;
                    this.projects.remove(ix);
                    if was_active {
                        this.active = this
                            .projects
                            .iter()
                            .position(|p| p.id == existing_id)
                            .unwrap_or(0);
                    } else if this.active > ix {
                        this.active -= 1;
                    }
                    this.hovered_project = None;
                    this.persist(cx);
                    cx.notify();
                    return;
                }
                let mut watch_setup = None;
                let mutated = this.projects[ix].mutating;
                let p = &mut this.projects[ix];
                if p.generation != generation {
                    return;
                }
                p.loading = false;
                p.refreshing = false;
                p.mutating = false;
                match result.0 {
                    Ok(snapshot) => {
                        if p.metadata_context
                            .as_ref()
                            .is_some_and(|context| context.git_directories() != snapshot.git_dirs)
                        {
                            p.watch_task = None;
                            p.watcher = None;
                        }
                        p.metadata_context = Some(snapshot.context);
                        if p.custom_name.is_none() {
                            p.name = snapshot
                                .root
                                .file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Project".into())
                                .into();
                        }
                        if p.watch_task.is_none() {
                            watch_setup = Some((snapshot.root.clone(), snapshot.git_dirs));
                        }
                        p.path = Some(snapshot.root);
                        p.branch = snapshot.branch.into();
                        for (view, files) in p.views.iter_mut().zip(snapshot.groups) {
                            view.install_metadata(files, id, cx);
                        }
                        p.update_count();
                        p.error = result.1;
                    }
                    Err(error) => {
                        p.error = Some(result.1.map_or(error.clone(), |e| format!("{e}\n{error}")))
                    }
                }
                let again = p.pending_refresh;
                p.pending_refresh = false;
                if let Some((root, dirs)) = watch_setup {
                    this.start_watching(id, root, dirs, cx);
                }
                this.ensure_preview(cx);
                this.discover_worktrees(id, mutated, cx);
                this.persist(cx);
                if again {
                    this.projects[ix].metadata_context = None;
                    this.refresh_project(id, cx);
                }
                cx.notify();
            });
        }));
        if visible {
            cx.notify();
        }
    }
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.is_pr() {
            self.list_prs(false, cx);
            return;
        }
        if let Some(key) = self.github_key()
            && self
                .github
                .states
                .get(&key)
                .is_some_and(|s| !s.resolving && s.remotes.is_empty())
        {
            self.github.states.remove(&key);
        }
        if let Some(id) = self.active_project_id() {
            self.discover_worktrees(id, true, cx);
        }
        if let Some(p) = self.projects.get_mut(self.active)
            && (p.loading || p.refreshing)
        {
            p.pending_refresh = true;
            return;
        }
        if let Some(id) = self.active_project_id() {
            self.invalidate_project(id, None);
        }
        self.load(None, cx);
    }
    pub(super) fn refresh_project(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(ix) = self.projects.iter().position(|p| p.id == id) {
            let active = self.active;
            self.active = ix;
            self.load(None, cx);
            self.active = active;
        }
    }
    pub(super) fn start_watching(
        &mut self,
        id: u64,
        root: PathBuf,
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        let state_path = self.state_path.clone();
        self.projects[ix].watch_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { watch::start(&root, &dirs, state_path) })
                .await;
            let rx = match result {
                Ok((watcher, rx)) => {
                    let installed = this
                        .update(cx, |this, cx| {
                            if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                                p.watcher = Some(watcher);
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if !installed {
                        return;
                    }
                    // Reconcile edits made between the initial read and watcher attachment.
                    let _ = this.update(cx, |this, cx| this.refresh_project(id, cx));
                    rx
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                            p.error = Some(error);
                            p.watch_task = None;
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            while let Ok(mut hint) = rx.recv().await {
                while let Ok(next) = rx.try_recv() {
                    hint.merge(next);
                }
                if this
                    .update(cx, |this, cx| {
                        let paths: Vec<_> = hint.paths.into_iter().collect();
                        this.invalidate_project(id, if hint.full { None } else { Some(&paths) });
                        this.refresh_project(id, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    pub(super) fn stage_file(&mut self, cx: &mut Context<Self>) {
        if self.updates.blocks_mutations() || self.is_pr() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        if p.loading || p.path.is_none() {
            return;
        }
        if let Some(file) = p.view().selected_entry().filter(|f| f.stageable()).cloned() {
            self.load(Some((vec![file], p.group == ChangeGroup::Changes)), cx);
        }
    }
    pub(super) fn stage_all(&mut self, cx: &mut Context<Self>) {
        if self.updates.blocks_mutations() || self.is_pr() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        if p.loading || p.path.is_none() || p.view().files.is_empty() {
            return;
        }
        self.load(
            Some((
                p.view()
                    .files
                    .iter()
                    .filter(|f| f.stageable())
                    .cloned()
                    .collect(),
                p.group == ChangeGroup::Changes,
            )),
            cx,
        );
    }
    pub(super) fn stage_path(
        &mut self,
        id: u64,
        group: ChangeGroup,
        path: &SharedString,
        cx: &mut Context<Self>,
    ) {
        if self.is_pr() {
            return;
        }
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        let p = &self.projects[ix];
        if p.loading || p.path.is_none() {
            return;
        }
        let file = p.views[group.index()]
            .files
            .iter()
            .find(|f| f.path() == path && f.stageable())
            .cloned();
        if let Some(file) = file {
            let active = self.active;
            self.active = ix;
            self.load(Some((vec![file], group == ChangeGroup::Changes)), cx);
            self.active = active;
        }
    }
    pub(super) fn invalidate_project(&mut self, id: u64, paths: Option<&[PathBuf]>) {
        if let Some(p) = self.projects.iter_mut().find(|p| p.id == id) {
            if paths.is_none() {
                p.metadata_context = None;
            }
            if let Some(root) = &p.path {
                self.preview_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .invalidate(root, paths);
            }
            for view in &mut p.views {
                if view.selected.as_ref().is_some_and(|selected| {
                    paths.is_none_or(|paths| {
                        paths.iter().any(|path| {
                            std::path::Path::new(selected.as_str()).starts_with(path)
                                || path.starts_with(selected.as_str())
                        })
                    })
                }) {
                    view.preview_revision = None;
                    view.preview_generation += 1;
                    view.preview_loading = false;
                    view.preview_task = None;
                }
            }
        }
    }
    pub(super) fn ensure_preview(&mut self, cx: &mut Context<Self>) {
        let active_key = self.github_key();
        for (key, state) in &mut self.github.states {
            if Some(key) != active_key.as_ref() {
                state.cancel(cx);
            }
        }
        if self.is_pr() {
            self.ensure_pr_preview(cx);
            return;
        }
        for (index, project) in self.projects.iter_mut().enumerate() {
            if project.path.is_none() {
                continue;
            }
            let active_group = project.group.index();
            for (group, view) in project.views.iter_mut().enumerate() {
                if index != self.active || group != active_group {
                    view.suspend(cx);
                }
            }
        }
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        let id = p.id;
        let group = p.group;
        let Some(root) = p.path.clone() else {
            return;
        };
        let view = p.view_mut();
        let Some(entry) = view.selected_entry().cloned() else {
            return;
        };
        if view.preview_revision == Some(entry.revision) || view.preview_loading {
            return;
        }
        let key = PreviewKey {
            root,
            group,
            path: entry.path.clone(),
            revision: entry.revision,
        };
        let cached = self
            .preview_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key);
        if let Some(preview) = cached {
            view.install_preview(preview, entry.revision, cx);
            return;
        }
        view.preview_generation += 1;
        let generation = view.preview_generation;
        self.preview_requests += 1;
        view.preview_loading = true;
        view.preview_error = None;
        view.preview_task = Some(cx.spawn(async move |owner, cx| {
            let request_key = key.clone();
            let result = cx
                .background_spawn(async move {
                    let _slot = repository::refresh_slot().await;
                    repository::load_preview(&request_key.root, group, &entry)
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                let Some(p) = this
                    .projects
                    .iter_mut()
                    .find(|p| p.id == id && p.path.as_ref() == Some(&key.root))
                else {
                    return;
                };
                let view = &mut p.views[group.index()];
                if view.preview_generation != generation
                    || view
                        .selected_entry()
                        .is_none_or(|e| e.path != key.path || e.revision != key.revision)
                {
                    return;
                }
                view.preview_loading = false;
                match result {
                    Ok(preview) => {
                        let preview = view
                            .preview
                            .as_ref()
                            .filter(|old| {
                                old.file.path() == preview.file.path()
                                    && old.fingerprint == preview.fingerprint
                            })
                            .cloned()
                            .unwrap_or(preview);
                        this.preview_cache
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .insert(key.clone(), preview.clone());
                        view.install_preview(preview, key.revision, cx);
                    }
                    Err(error) => view.preview_error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    pub fn live_ready(&self) -> bool {
        self.projects
            .get(self.active)
            .is_some_and(|p| p.watcher.is_some())
    }
    pub fn selected_additions(&self) -> usize {
        self.active_view()
            .and_then(|v| v.selected_file())
            .map_or(0, |f| f.additions())
    }
}
