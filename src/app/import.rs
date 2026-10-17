use super::Diffy;
use crate::discovery::{self, Snapshot, Source};
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex, v_virtual_list,
};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::component::{VirtualListScrollHandle, scroll::ScrollableElement as _};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, WeakEntity, Window, div, px, rgb, size,
};
use std::rc::Rc;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) struct Runtime {
    pub preferences: discovery::Preferences,
    pub snapshot: Arc<Snapshot>,
    pub picker: Option<Entity<Picker>>,
    pub generation: u64,
    scanner: Option<discovery::Scanner>,
    #[cfg(feature = "ui-tests")]
    fixture: Option<Snapshot>,
    pub loading: bool,
    pub cancelled: Arc<AtomicBool>,
    pub task: Option<gpui_kit::Task<()>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            preferences: Default::default(),
            snapshot: Arc::new(Snapshot::default()),
            picker: None,
            generation: 0,
            scanner: None,
            #[cfg(feature = "ui-tests")]
            fixture: None,
            loading: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            task: None,
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(super) struct Picker {
    owner: WeakEntity<Diffy>,
    focus: FocusHandle,
    snapshot: Arc<Snapshot>,
    existing: BTreeSet<PathBuf>,
    selected: BTreeSet<PathBuf>,
    seen: BTreeSet<PathBuf>,
    excluded_repositories: BTreeSet<PathBuf>,
    loading: bool,
    scroll: VirtualListScrollHandle,
    sizes: Rc<Vec<gpui_kit::Size<gpui_kit::Pixels>>>,
}
impl Picker {
    fn install(&mut self, snapshot: Arc<Snapshot>, existing: BTreeSet<PathBuf>, loading: bool) {
        self.existing = existing;
        self.selected.retain(|path| {
            snapshot
                .candidates
                .iter()
                .any(|c| c.path == *path && c.error.is_none())
                && !self.existing.contains(path)
        });
        for candidate in &snapshot.candidates {
            if self.seen.insert(candidate.path.clone())
                && candidate.error.is_none()
                && !self.existing.contains(&candidate.path)
                && !candidate
                    .repository
                    .as_ref()
                    .is_some_and(|r| self.excluded_repositories.contains(r))
            {
                self.selected.insert(candidate.path.clone());
            }
        }
        self.snapshot = snapshot;
        self.loading = loading;
        self.sizes = Rc::new(vec![
            size(px(600.), px(60.));
            self.snapshot.candidates.len()
        ]);
    }
    fn finish(&mut self, apply: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected.clone();
        let snapshot = self.snapshot.clone();
        let _ = self.owner.update(cx, |this, cx| {
            this.finish_import(apply, selected, &snapshot, window, cx)
        });
    }
    fn render_project(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let candidate = &self.snapshot.candidates[ix];
        let row_id = candidate.path.to_string_lossy();
        let path = candidate.path.clone();
        let tooltip = path.display().to_string();
        let row_path = path.clone();
        let existing = self.existing.contains(&path);
        let disabled = existing || candidate.error.is_some();
        h_flex()
            .id(format!("import-project-{row_id}"))
            .test_support()
            .h(px(60.))
            .w_full()
            .gap_3()
            .px_2()
            .rounded_md()
            .hover(|s| s.bg(rgb(0x141414)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !disabled {
                    if !this.selected.remove(&row_path) {
                        this.selected.insert(row_path.clone());
                    }
                    cx.notify();
                }
            }))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .child(if disabled {
                Icon::new(if existing {
                    IconName::Check
                } else {
                    IconName::CircleAlert
                })
                .size_4()
                .text_color(cx.theme().muted_foreground)
                .into_any_element()
            } else {
                div()
                    .id(format!("import-check-{row_id}"))
                    .test_support()
                    .child(
                        Checkbox::new(format!("choose-project-{row_id}"))
                            .accessibility_label(format!("Import {}", candidate.path.display()))
                            .checked(self.selected.contains(&path))
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                cx.stop_propagation();
                                if *checked {
                                    this.selected.insert(path.clone());
                                } else {
                                    this.selected.remove(&path);
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().font_medium().truncate().child(candidate.name()))
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(candidate.path.display().to_string()),
                    )
                    .when(candidate.error.is_some(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(candidate.error.clone().unwrap()),
                        )
                    }),
            )
            .when(existing, |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Already added"),
                )
            })
            .into_any_element()
    }
}
impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = if self.loading {
            "Pulling latest projects from Herdr, Codex, and Claude Code…".into()
        } else {
            format!(
                "{} project{} found · Most recently updated first",
                self.snapshot.candidates.len(),
                if self.snapshot.candidates.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        };
        v_flex()
            .id("import-picker")
            .test_support()
            .track_focus(&self.focus)
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Choose local Git projects to add to Diffy."),
            )
            .child(
                div()
                    .id("import-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(status.clone())
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(status),
            )
            .children(Source::ALL.into_iter().filter_map(|source| {
                self.snapshot.errors[source.index()].as_ref().map(|error| {
                    div()
                        .id(format!("import-error-{}", source.index()))
                        .test_support()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{}: {error}", source.label()))
                })
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("import-select-all")
                            .ghost()
                            .small()
                            .label("Select all")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected = this
                                    .snapshot
                                    .candidates
                                    .iter()
                                    .filter(|c| {
                                        c.error.is_none() && !this.existing.contains(&c.path)
                                    })
                                    .map(|c| c.path.clone())
                                    .collect();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("import-clear")
                            .ghost()
                            .small()
                            .label("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected.clear();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("import-refresh")
                            .ghost()
                            .small()
                            .label("Refresh")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.loading {
                                    return;
                                }
                                this.loading = true;
                                cx.notify();
                                let owner = this.owner.clone();
                                let picker = cx.entity().downgrade();
                                // The parent updates this picker when a scan starts. Wait
                                // until the click handler has released its entity lease.
                                cx.defer(move |cx| {
                                    let _ =
                                        owner.update(cx, |owner, cx| {
                                            if owner.import.picker.as_ref().is_some_and(|p| {
                                                p.entity_id() == picker.entity_id()
                                            }) {
                                                owner.request_import_scan(cx);
                                            }
                                        });
                                });
                            })),
                    ),
            )
            .child(
                div()
                    .id("import-scroll")
                    .test_support()
                    .relative()
                    .flex_none()
                    .min_h_0()
                    .h(px(
                        (f32::from(window.viewport_size().height) * 0.52).min(460.)
                    ))
                    .when(self.snapshot.candidates.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_3()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if self.loading {
                                    "Looking for projects…"
                                } else {
                                    "No projects found."
                                }),
                        )
                    })
                    .when(!self.snapshot.candidates.is_empty(), |d| {
                        d.child(
                            v_virtual_list(
                                cx.entity(),
                                "import-results",
                                self.sizes.clone(),
                                |this, range, _, cx| {
                                    range
                                        .map(|ix| this.render_project(ix, cx))
                                        .collect::<Vec<_>>()
                                },
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                        .vertical_scrollbar(&self.scroll)
                    }),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(Button::new("import-skip").ghost().label("Cancel").on_click(
                        cx.listener(|this, _, window, cx| this.finish(false, window, cx)),
                    ))
                    .child(
                        Button::new("import-apply")
                            .ghost()
                            .disabled(self.loading)
                            .label(if self.selected.is_empty() {
                                "Done".into()
                            } else {
                                format!("Import {} projects", self.selected.len())
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.finish(true, window, cx)),
                            ),
                    ),
            )
    }
}

impl Diffy {
    pub(super) fn project_paths(&self) -> BTreeSet<PathBuf> {
        self.projects
            .iter()
            .filter_map(|p| p.path.clone())
            .collect()
    }
    pub(super) fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import.picker.is_some() {
            return;
        }
        self.stop_file_repeat();
        self.reset_file_scroll();
        if self.settings.is_some() {
            self.close_settings(window, cx);
        }
        if self.palette_open {
            self.close_palette(true, window, cx);
        }
        let existing = self.project_paths();
        let owner = cx.entity().downgrade();
        let snapshot = self.import.snapshot.clone();
        let excluded = self.import.preferences.excluded.clone();
        let excluded_repositories = self.import.preferences.excluded_repositories.clone();
        let picker = cx.new(|cx| {
            let mut picker = Picker {
                owner,
                focus: cx.focus_handle(),
                snapshot: Arc::new(Snapshot::default()),
                existing: BTreeSet::new(),
                selected: BTreeSet::new(),
                seen: excluded,
                excluded_repositories,
                loading: true,
                scroll: VirtualListScrollHandle::new(),
                sizes: Rc::new(Vec::new()),
            };
            picker.install(snapshot, existing, true);
            picker
        });
        self.import.picker = Some(picker.clone());
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let picker = picker.clone();
            let owner = owner.clone();
            dialog
                .title("Import projects")
                .entrance(gpui_kit::component::dialog::DialogEntrance::Fade)
                .width(px(680.))
                .bg(rgb(0x090909))
                .border_0()
                .content(move |content, _, _| content.child(picker.clone()))
                .on_close(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        this.cancel_import_scan();
                        this.import.picker = None;
                        this.import.preferences.onboarding_completed = true;
                        this.persist(cx);
                        this.focus.focus(window, cx);
                        cx.notify();
                    });
                })
        });
        self.request_import_scan(cx);
    }
    fn cancel_import_scan(&mut self) {
        self.import.cancelled.store(true, Ordering::Relaxed);
        self.import.generation += 1;
        self.import.task = None;
        self.import.loading = false;
    }
    fn request_import_scan(&mut self, cx: &mut Context<Self>) {
        if self.import.loading || self.import.picker.is_none() {
            return;
        }
        self.import.loading = true;
        self.import.generation += 1;
        let generation = self.import.generation;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.import.cancelled = cancelled.clone();
        if let Some(picker) = &self.import.picker {
            picker.update(cx, |p, cx| {
                p.loading = true;
                cx.notify();
            });
        }
        let mut scanner = self.import.scanner.take().unwrap_or_default();
        let locations = discovery::Locations::from_env();
        #[cfg(feature = "ui-tests")]
        let fixture = self.import.fixture.clone();
        self.import.task = Some(cx.spawn(async move |owner, cx| {
            let (scanner, snapshot) = cx
                .background_spawn(async move {
                    #[cfg(feature = "ui-tests")]
                    if let Some(snapshot) = fixture {
                        return (scanner, snapshot);
                    }
                    let snapshot = scanner.scan(&locations, [true; 3], &cancelled);
                    (scanner, snapshot)
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                this.complete_import_scan(generation, scanner, snapshot, cx);
            });
        }));
        cx.notify();
    }
    fn complete_import_scan(
        &mut self,
        generation: u64,
        scanner: discovery::Scanner,
        snapshot: Snapshot,
        cx: &mut Context<Self>,
    ) {
        if generation != self.import.generation || self.import.picker.is_none() {
            return;
        }
        self.import.scanner = Some(scanner);
        self.import.task = None;
        self.receive_discovery(snapshot, cx);
    }
    pub(super) fn finish_import(
        &mut self,
        apply: bool,
        selected: BTreeSet<PathBuf>,
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if apply {
            for candidate in &snapshot.candidates {
                if selected.contains(&candidate.path) {
                    self.import.preferences.excluded.remove(&candidate.path);
                } else if candidate.error.is_none()
                    && !self.project_paths().contains(&candidate.path)
                {
                    self.import
                        .preferences
                        .excluded
                        .insert(candidate.path.clone());
                }
            }
            self.import_projects(selected, cx);
        }
        self.import.preferences.onboarding_completed = true;
        self.cancel_import_scan();
        self.import.picker = None;
        self.persist(cx);
        window.close_dialog(cx);
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn import_projects(&mut self, paths: BTreeSet<PathBuf>, cx: &mut Context<Self>) {
        let active = self.active_project_id();
        let was_restoring = self.restoring;
        self.restoring = true;
        for path in paths {
            if !self.project_paths().contains(&path) {
                self.add_path(path, cx);
            }
        }
        if let Some(id) = active
            && let Some(ix) = self.projects.iter().position(|p| p.id == id)
        {
            self.active = ix;
        } else if active.is_none() {
            self.active = 0;
        }
        self.restoring = was_restoring;
        self.persist(cx);
    }
    fn receive_discovery(&mut self, mut snapshot: Snapshot, cx: &mut Context<Self>) {
        snapshot.sort();
        let snapshot = Arc::new(snapshot);
        self.import.loading = false;
        self.import.snapshot = snapshot.clone();
        if let Some(picker) = &self.import.picker {
            let existing = self.project_paths();
            picker.update(cx, |p, cx| {
                p.install(snapshot, existing, false);
                cx.notify();
            });
        }
        cx.notify();
    }
}

#[cfg(feature = "ui-tests")]
impl Diffy {
    pub fn open_import_fixture(
        &mut self,
        snapshot: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import.fixture = Some(snapshot.clone());
        self.open_import(window, cx);
        self.cancel_import_scan();
        self.receive_discovery(snapshot, cx);
    }
    pub fn import_selected_count(&self, cx: &gpui_kit::App) -> usize {
        self.import
            .picker
            .as_ref()
            .map_or(0, |picker| picker.read(cx).selected.len())
    }
    pub fn set_import_fixture(&mut self, snapshot: Snapshot) {
        self.import.fixture = Some(snapshot);
    }
    pub fn import_loading(&self, cx: &gpui_kit::App) -> bool {
        self.import
            .picker
            .as_ref()
            .is_some_and(|p| p.read(cx).loading)
    }
    pub fn import_paths(&self) -> Vec<PathBuf> {
        self.import
            .snapshot
            .candidates
            .iter()
            .map(|c| c.path.clone())
            .collect()
    }
    pub fn import_generation(&self) -> u64 {
        self.import.generation
    }
    pub fn complete_import_fixture(
        &mut self,
        generation: u64,
        snapshot: Snapshot,
        cx: &mut Context<Self>,
    ) {
        self.complete_import_scan(generation, discovery::Scanner::default(), snapshot, cx);
    }
    pub fn import_is_open(&self) -> bool {
        self.import.picker.is_some()
    }
    pub fn discovery_preferences(&self) -> &discovery::Preferences {
        &self.import.preferences
    }
}
