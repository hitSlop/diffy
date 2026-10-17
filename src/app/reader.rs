use super::Diffy;
use gpui_kit::component::Theme;
use gpui_kit::{ClipboardItem, Context, px};
use std::path::{Path, PathBuf};

pub(super) fn reveal_target(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.exists()).map(Path::to_path_buf)
}
impl Diffy {
    pub(super) fn copy_file_path(
        &self,
        id: u64,
        path: &str,
        absolute: bool,
        cx: &mut Context<Self>,
    ) {
        if absolute && self.is_pr() {
            return;
        }
        let Some(p) = self.projects.iter().find(|p| p.id == id) else {
            return;
        };
        let text = if absolute {
            let Some(root) = &p.path else {
                return;
            };
            root.join(path).to_string_lossy().into_owned()
        } else {
            path.to_owned()
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
    pub(super) fn reveal_file_path(&self, id: u64, path: &str, cx: &mut Context<Self>) {
        if self.is_pr() {
            return;
        }
        if let Some(p) = self.projects.iter().find(|p| p.id == id)
            && let Some(root) = &p.path
            && let Some(path) = reveal_target(&root.join(path))
        {
            cx.reveal_path(&path);
        }
    }
    pub(super) fn copy_path(&self, absolute: bool, cx: &mut Context<Self>) {
        if absolute && self.is_pr() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        let Some(file) = self.active_view().and_then(|v| v.selected_entry()) else {
            return;
        };
        let text = if absolute {
            let Some(root) = &p.path else {
                return;
            };
            root.join(file.path.as_str()).to_string_lossy().into_owned()
        } else {
            file.path.to_string()
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
    pub(super) fn reveal_file(&self, cx: &mut Context<Self>) {
        if self.is_pr() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        let Some(file) = self.active_view().and_then(|v| v.selected_entry()) else {
            return;
        };
        if let Some(root) = &p.path
            && let Some(path) = reveal_target(&root.join(file.path.as_str()))
        {
            cx.reveal_path(&path);
        }
    }
    pub(super) fn zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.reader.font_size += delta;
        self.reader.normalize();
        Theme::update(cx, |theme| theme.mono_font_size = px(self.reader.font_size));
        self.persist_navigation(cx);
        cx.notify();
    }
    pub(super) fn toggle_wrap(&mut self, cx: &mut Context<Self>) {
        self.reader.wrap = !self.reader.wrap;
        self.persist(cx);
        cx.notify();
    }
}

#[cfg(feature = "ui-tests")]
impl Diffy {
    pub fn reader_settings(&self) -> &crate::state::ReaderSettings {
        &self.reader
    }
    pub fn preview_request_count(&self) -> u64 {
        self.preview_requests
    }
    pub fn selected_preview_path(&self) -> Option<&gpui_kit::SharedString> {
        self.active_view()
            .and_then(|v| v.selected_file())
            .map(|f| f.path())
    }
    pub fn preferences_flush(&self) -> Option<crate::state::Flush> {
        self.state_writer
            .as_ref()
            .map(crate::state::Writer::flush_handle)
    }
    pub fn selected_top_line(&self, cx: &gpui_kit::App) -> Option<usize> {
        self.active_view()?
            .diff
            .read(cx)
            .reading_position()
            .source_line()
            .map(|line| line.line())
    }
}

pub(super) fn file_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    id: u64,
    path: &str,
    local: bool,
) -> gpui_kit::component::menu::PopupMenu {
    menu.menu(
        "Copy relative path",
        Box::new(super::CopyPath {
            id,
            path: path.into(),
            absolute: false,
        }),
    )
    .menu_with_disabled(
        "Copy absolute path",
        Box::new(super::CopyPath {
            id,
            path: path.into(),
            absolute: true,
        }),
        !local,
    )
    .menu_with_disabled(
        crate::project_apps::reveal_label(),
        Box::new(super::RevealPath {
            id,
            path: path.into(),
        }),
        !local,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deleted_file_reveals_existing_parent() {
        let root = std::env::temp_dir();
        assert_eq!(
            reveal_target(&root.join("diffy-nonexistent-parent/deleted.rs")),
            Some(root)
        );
    }
}
