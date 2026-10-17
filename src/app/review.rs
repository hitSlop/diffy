use super::Diffy;
use crate::repository::{ChangedFile, GroupSnapshot, Preview};
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::FocusHandle;
use gpui_kit::ScrollStrategy;
use gpui_kit::SharedString;
use gpui_kit::component::VirtualListScrollHandle;
use gpui_kit::component::diff::DiffFile;
use gpui_kit::component::diff::DiffInlineUnit;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::diff::DiffState;
use gpui_kit::component::input::InputState;
use gpui_kit::{Pixels, Size, px, size};
use nucleo_matcher::Config;
use nucleo_matcher::Matcher;
use nucleo_matcher::Utf32Str;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;
use nucleo_matcher::pattern::Pattern;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub(super) const FILE_ROW_HEIGHT: f32 = 30.;
pub(super) const FILE_INDENT: f32 = 12.;
/// Header text row. A header after another row adds [`FOLDER_HEADER_LEADING`].
pub(super) const FOLDER_HEADER_HEIGHT: f32 = 22.;
pub(super) const FOLDER_HEADER_LEADING: f32 = 6.;

/// Labels built once when a snapshot arrives, so scrolling does not format them again.
pub(super) struct FileRow {
    pub(super) filename: SharedString,
    pub(super) group: SharedString,
    pub(super) stage_label: SharedString,
    pub(super) unstage_label: SharedString,
}

impl FileRow {
    fn from_path(path: &SharedString, project_id: u64, index: usize) -> Self {
        let filename = match path.rsplit_once('/') {
            Some((_, name)) if !name.is_empty() => SharedString::from(name),
            _ => path.clone(),
        };
        Self {
            filename,
            group: SharedString::from(format!("file-row-{project_id}-{index}")),
            stage_label: SharedString::from(format!("Stage {path}")),
            unstage_label: SharedString::from(format!("Unstage {path}")),
        }
    }
}

/// One folder line above the files that share an immediate parent directory.
pub(super) struct FolderHeader {
    pub(super) ancestor: Option<SharedString>,
    pub(super) leaf: SharedString,
    /// `src · app`, or just `src` when the directory has no parent.
    pub(super) label: SharedString,
    pub(super) count: usize,
    /// Space above this header. The first row in the list has none.
    pub(super) leading: f32,
}

pub(super) enum SidebarRow {
    Folder(FolderHeader),
    File { index: usize, indent: bool },
}

impl SidebarRow {
    pub(super) fn height(&self) -> f32 {
        match self {
            Self::Folder(header) => FOLDER_HEADER_HEIGHT + header.leading,
            Self::File { .. } => FILE_ROW_HEIGHT,
        }
    }
}

pub(super) struct ReviewView {
    loaded: bool,
    pub(super) indices: HashMap<SharedString, usize>,
    pub(super) files: Arc<Vec<ChangedFile>>,
    pub(super) preview: Option<Preview>,
    pub(super) preview_revision: Option<u64>,
    pub(super) preview_loading: bool,
    pub(super) preview_error: Option<String>,
    pub(super) preview_generation: u64,
    pub(super) preview_task: Option<gpui_kit::Task<()>>,
    reading_position: Option<gpui_kit::component::diff::DiffReadingPosition>,
    demos: HashMap<SharedString, Preview>,
    pub(super) rows: Arc<Vec<FileRow>>,
    pub(super) sidebar: Arc<Vec<SidebarRow>>,
    /// Sidebar row of each visible path. Arrow keys and scrolling use this,
    /// because folder headers sit between files.
    pub(super) visual: HashMap<SharedString, usize>,
    /// File indexes that pass the current query, in path order.
    pub(super) visible_files: Vec<usize>,
    pub(super) query: String,
    pub(super) search: Option<Entity<InputState>>,
    matcher: Matcher,
    match_buf: Vec<char>,
    pub(super) selected: Option<SharedString>,
    pub(super) diff: gpui_kit::Entity<DiffState>,
    pub(super) scroll: VirtualListScrollHandle,
    pub(super) row_sizes: Rc<Vec<Size<Pixels>>>,
    pub(super) focus: FocusHandle,
}

impl ReviewView {
    pub(super) fn new(cx: &mut Context<Diffy>) -> Self {
        Self {
            loaded: false,
            indices: HashMap::new(),
            files: Arc::default(),
            preview: None,
            preview_revision: None,
            preview_loading: false,
            preview_error: None,
            preview_generation: 0,
            preview_task: None,
            reading_position: None,
            demos: HashMap::new(),
            rows: Arc::default(),
            sidebar: Arc::default(),
            visual: HashMap::new(),
            visible_files: Vec::new(),
            query: String::new(),
            search: None,
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
            match_buf: Vec::new(),
            selected: None,
            diff: cx.new(|cx| {
                DiffState::new([], cx)
                    .with_mode(DiffMode::Unified)
                    .with_context_lines(Some(3))
                    .with_inline_unit(Some(DiffInlineUnit::Word))
            }),
            scroll: VirtualListScrollHandle::new(),
            row_sizes: Rc::default(),
            focus: cx.focus_handle(),
        }
    }

    pub(super) fn clear(&mut self, cx: &mut Context<Diffy>) {
        self.demos.clear();
        self.selected = None;
        self.reset_selection(cx);
        self.install_metadata(Vec::new(), 0, cx);
    }
    pub(super) fn loaded(&self) -> bool {
        self.loaded
    }

    pub(super) fn selected_entry(&self) -> Option<&ChangedFile> {
        self.selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .and_then(|ix| self.files.get(*ix))
    }
    pub(super) fn selected_file(&self) -> Option<&DiffFile> {
        self.preview
            .as_ref()
            .filter(|preview| self.selected.as_ref() == Some(preview.file.path()))
            .map(|p| &p.file)
    }
    pub(super) fn select(&mut self, path: SharedString, cx: &mut Context<Diffy>) {
        if self.selected.as_ref() == Some(&path) {
            return;
        }
        self.selected = Some(path);
        self.reset_selection(cx);
        if let Some(ix) = self
            .selected
            .as_ref()
            .and_then(|path| self.visual.get(path))
            .copied()
        {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }
    fn reset_selection(&mut self, cx: &mut Context<Diffy>) {
        self.reading_position = None;
        self.preview_generation += 1;
        self.preview_task = None;
        self.preview_loading = false;
        self.preview_error = None;
        self.preview_revision = None;
        self.preview = self
            .selected
            .as_ref()
            .and_then(|p| self.demos.get(p))
            .cloned();
        if self.preview.is_some() {
            self.preview_revision = self.selected_entry().map(|e| e.revision);
        }
        let file = self.selected_file().cloned();
        self.diff.update(cx, |diff, cx| diff.set_files(file, cx));
    }
    pub(super) fn install_preview(
        &mut self,
        preview: Preview,
        revision: u64,
        cx: &mut Context<Diffy>,
    ) {
        let same_path = self
            .preview
            .as_ref()
            .is_some_and(|old| old.file.path() == preview.file.path());
        let unchanged = same_path
            && self
                .preview
                .as_ref()
                .is_some_and(|old| old.fingerprint == preview.fingerprint);
        if !unchanged {
            let file = preview.file.clone();
            let position = self.reading_position.take();
            self.diff.update(cx, |diff, cx| {
                if same_path {
                    diff.set_files_preserving_position(Some(file), cx);
                } else {
                    diff.set_files(Some(file), cx);
                }
                if let Some(position) = position {
                    diff.restore_reading_position(position, cx);
                }
            });
        }
        self.preview = Some(preview);
        self.preview_revision = Some(revision);
        self.preview_loading = false;
        self.preview_error = None;
    }

    pub(super) fn suspend(&mut self, cx: &mut Context<Diffy>) {
        self.preview_generation += 1;
        self.preview_task = None;
        self.preview_loading = false;
        if self.preview.take().is_some() {
            self.reading_position = Some(self.diff.read(cx).reading_position());
            self.preview_revision = None;
            self.diff.update(cx, |diff, cx| diff.set_files(None, cx));
        }
    }

    pub(super) fn set_query(&mut self, query: String, cx: &mut Context<Diffy>) {
        if self.query == query {
            return;
        }
        self.query = query;
        self.rebuild_sidebar();
        if let Some(ix) = self
            .selected
            .as_ref()
            .and_then(|path| self.visual.get(path))
            .copied()
        {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        } else if !self.sidebar.is_empty() {
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Next or previous file among the rows the current query still shows.
    /// A hidden selection starts from the first visible file, or the last when moving up.
    pub(super) fn step_visible(&self, direction: isize) -> Option<usize> {
        if self.visible_files.is_empty() {
            return None;
        }
        let position = self
            .selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .and_then(|ix| self.visible_files.iter().position(|v| v == ix));
        let next = match position {
            Some(i) => {
                let next = i
                    .saturating_add_signed(direction)
                    .min(self.visible_files.len() - 1);
                if next == i {
                    return None;
                }
                next
            }
            None => {
                if direction < 0 {
                    self.visible_files.len() - 1
                } else {
                    0
                }
            }
        };
        Some(self.visible_files[next])
    }

    fn rebuild_sidebar(&mut self) {
        let paths: Vec<&str> = self.files.iter().map(|file| file.path().as_ref()).collect();
        self.visible_files =
            filter_paths(&paths, &self.query, &mut self.matcher, &mut self.match_buf);
        self.sidebar = Arc::new(group_indexed(&paths, &self.visible_files));
        self.visual = self
            .sidebar
            .iter()
            .enumerate()
            .filter_map(|(row, entry)| match entry {
                SidebarRow::File { index, .. } => Some((self.files[*index].path().clone(), row)),
                SidebarRow::Folder(_) => None,
            })
            .collect();
        self.row_sizes = Rc::new(
            self.sidebar
                .iter()
                .map(|row| size(px(0.), px(row.height())))
                .collect(),
        );
    }

    pub(super) fn install(
        &mut self,
        snapshot: GroupSnapshot,
        project_id: u64,
        cx: &mut Context<Diffy>,
    ) {
        let entries = snapshot
            .files
            .iter()
            .map(|file| {
                let revision = snapshot.revisions[file.path()];
                self.demos.insert(
                    file.path().clone(),
                    Preview {
                        file: file.clone(),
                        fingerprint: revision,
                        bytes: file.estimated_size_bytes(),
                    },
                );
                ChangedFile::from_diff(file, revision)
            })
            .collect();
        self.install_metadata(entries, project_id, cx);
    }
    pub(super) fn install_metadata(
        &mut self,
        files: Vec<ChangedFile>,
        project_id: u64,
        cx: &mut Context<Diffy>,
    ) {
        let previous_selected = self.selected.clone();
        let previous_revision = self.selected_entry().map(|e| e.revision);
        let ordinal = self
            .selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .copied()
            .unwrap_or(0);
        let paths_changed = self.files.len() != files.len()
            || self.files.iter().zip(&files).any(|(a, b)| a.path != b.path);
        self.loaded = true;
        if paths_changed {
            self.rows = Arc::new(
                files
                    .iter()
                    .enumerate()
                    .map(|(ix, file)| FileRow::from_path(file.path(), project_id, ix))
                    .collect(),
            );
            self.indices = files
                .iter()
                .enumerate()
                .map(|(ix, file)| (file.path().clone(), ix))
                .collect();
        }
        self.files = Arc::new(files);
        if paths_changed {
            self.rebuild_sidebar();
        }
        self.selected = self
            .selected
            .take()
            .filter(|p| self.indices.contains_key(p))
            .or_else(|| {
                self.files
                    .get(ordinal.min(self.files.len().saturating_sub(1)))
                    .map(|f| f.path().clone())
            });
        if self.selected != previous_selected || !self.demos.is_empty() {
            self.reset_selection(cx);
        } else if self.selected_entry().map(|e| e.revision) != previous_revision {
            self.preview_generation += 1;
            self.preview_task = None;
            self.preview_loading = false;
            self.preview_revision = None;
            self.preview_error = None;
        }
    }
}

/// Files whose path matches `query`, in the order they were given.
/// An empty or whitespace query keeps every path.
pub(super) fn filter_paths(
    paths: &[&str],
    query: &str,
    matcher: &mut Matcher,
    buf: &mut Vec<char>,
) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..paths.len()).collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    paths
        .iter()
        .enumerate()
        .filter_map(|(ix, path)| pattern.score(Utf32Str::new(path, buf), matcher).map(|_| ix))
        .collect()
}

/// Group `visible` indexes by immediate parent directory, preserving path order.
pub(super) fn group_indexed(paths: &[&str], visible: &[usize]) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < visible.len() {
        let index = visible[i];
        let Some(dir) = parent_dir(paths[index]) else {
            rows.push(SidebarRow::File {
                index,
                indent: false,
            });
            i += 1;
            continue;
        };
        let mut count = 1;
        while i + count < visible.len() && parent_dir(paths[visible[i + count]]) == Some(dir) {
            count += 1;
        }
        let (ancestor, leaf, label) = folder_parts(dir);
        rows.push(SidebarRow::Folder(FolderHeader {
            ancestor,
            leaf,
            label,
            count,
            leading: if rows.is_empty() {
                0.
            } else {
                FOLDER_HEADER_LEADING
            },
        }));
        for offset in 0..count {
            rows.push(SidebarRow::File {
                index: visible[i + offset],
                indent: true,
            });
        }
        i += count;
    }
    rows
}

#[cfg(test)]
pub(super) fn group_paths(paths: &[&str]) -> Vec<SidebarRow> {
    let visible: Vec<usize> = (0..paths.len()).collect();
    group_indexed(paths, &visible)
}

fn parent_dir(path: &str) -> Option<&str> {
    let (dir, name) = path.rsplit_once('/')?;
    if dir.is_empty() || name.is_empty() {
        None
    } else {
        Some(dir)
    }
}

fn folder_parts(dir: &str) -> (Option<SharedString>, SharedString, SharedString) {
    match dir.rsplit_once('/') {
        Some((ancestor, leaf)) if !ancestor.is_empty() && !leaf.is_empty() => {
            let ancestor = SharedString::from(ancestor);
            let leaf = SharedString::from(leaf);
            let label = SharedString::from(format!("{ancestor} · {leaf}"));
            (Some(ancestor), leaf, label)
        }
        _ => {
            let leaf = SharedString::from(dir);
            (None, leaf.clone(), leaf)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[SidebarRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                SidebarRow::Folder(header) => format!("folder:{}:{}", header.label, header.count),
                SidebarRow::File { index, indent } => {
                    format!("file:{index}:{}", if *indent { "in" } else { "root" })
                }
            })
            .collect()
    }

    #[test]
    fn folder_headers_group_one_directory_level() {
        let paths = [
            "README.md",
            "hitslop-runner/lib.rs",
            "src/cli.rs",
            "src/app/files.rs",
            "src/app/mod.rs",
            "zz/very-long-directory-name/another-long-directory/deeply-nested-source/capture-build.ts",
        ];
        let rows = group_paths(&paths);
        assert_eq!(
            labels(&rows),
            vec![
                "file:0:root",
                "folder:hitslop-runner:1",
                "file:1:in",
                "folder:src:1",
                "file:2:in",
                "folder:src · app:2",
                "file:3:in",
                "file:4:in",
                "folder:zz/very-long-directory-name/another-long-directory · deeply-nested-source:1",
                "file:5:in",
            ]
        );
        let nested = rows.iter().find_map(|row| match row {
            SidebarRow::Folder(header) if header.leaf.as_ref() == "deeply-nested-source" => {
                Some(header)
            }
            _ => None,
        });
        let nested = nested.unwrap();
        assert_eq!(
            nested.ancestor.as_deref(),
            Some("zz/very-long-directory-name/another-long-directory")
        );
        assert_eq!(nested.leading, FOLDER_HEADER_LEADING);
        match &rows[0] {
            SidebarRow::File { indent, .. } => assert!(!indent),
            SidebarRow::Folder(_) => panic!("root file has no header"),
        }
        match &rows[3] {
            SidebarRow::Folder(header) => {
                assert!(header.ancestor.is_none());
                assert_eq!(header.leaf.as_ref(), "src");
                assert_eq!(header.leading, FOLDER_HEADER_LEADING);
            }
            SidebarRow::File { .. } => panic!("expected the src header"),
        }
    }

    #[test]
    fn first_header_has_no_leading_space() {
        let rows = group_paths(&["src/cli.rs", "src/app/files.rs"]);
        match &rows[0] {
            SidebarRow::Folder(header) => {
                assert_eq!(header.label.as_ref(), "src");
                assert_eq!(header.leading, 0.);
                assert_eq!(header.count, 1);
            }
            SidebarRow::File { .. } => panic!("expected a header"),
        }
        match &rows[2] {
            SidebarRow::Folder(header) => {
                assert_eq!(header.label.as_ref(), "src · app");
                assert_eq!(header.leading, FOLDER_HEADER_LEADING);
            }
            SidebarRow::File { .. } => panic!("expected a header"),
        }
    }

    #[test]
    fn file_filter_keeps_path_order() {
        let paths = [
            "README.md",
            "src/cli.rs",
            "src/app/files.rs",
            "src/app/mod.rs",
        ];
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let mut buf = Vec::new();
        assert_eq!(
            filter_paths(&paths, "   ", &mut matcher, &mut buf),
            vec![0, 1, 2, 3]
        );
        assert_eq!(
            filter_paths(&paths, "app files", &mut matcher, &mut buf),
            vec![2]
        );
        assert_eq!(
            filter_paths(&paths, "src", &mut matcher, &mut buf),
            vec![1, 2, 3]
        );
        assert_eq!(
            filter_paths(&paths, "readme", &mut matcher, &mut buf),
            vec![0]
        );
        let visible = filter_paths(&paths, "app", &mut matcher, &mut buf);
        let rows = group_indexed(&paths, &visible);
        assert_eq!(
            labels(&rows),
            vec!["folder:src · app:2", "file:2:in", "file:3:in"]
        );
        assert!(filter_paths(&paths, "zzzz-no-such", &mut matcher, &mut buf).is_empty());
    }
}
