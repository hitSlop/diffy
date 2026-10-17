#[path = "../benches/support/fixture.rs"]
mod fixture;
use diffy::{
    app,
    discovery::{Candidate, Snapshot, Source},
};
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, ScrollDelta, WindowBounds, WindowOptions,
    assets::Assets, point, px, size, test::TestWindowExt as _,
};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

fn check(ix: usize) -> String {
    format!("import-check-/tmp/import-fixture/project-{ix:02}")
}

fn settle(cx: &mut HeadlessAppContext) {
    for _ in 0..20 {
        cx.advance_clock(Duration::from_millis(16));
        cx.run_until_parked();
    }
}

fn main() {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        app::init(cx);
        cx.set_reduce_motion(true);
    });
    let (handle, view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1200.), px(800.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| app::Diffy::demo(window, cx)),
            )
        })
        .unwrap();
    let snapshot = Snapshot {
        candidates: (0..40)
            .map(|i| Candidate {
                repository: None,
                path: PathBuf::from(format!("/tmp/import-fixture/project-{i:02}")),
                sources: if i == 39 {
                    BTreeSet::from([Source::Herdr, Source::Claude])
                } else {
                    BTreeSet::from([Source::Herdr])
                },
                error: None,
                updated_at: None,
            })
            .collect(),
        errors: Default::default(),
    };
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.open_import_fixture(snapshot.clone(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    // Finish the dialog entrance animation before clicking its visual bounds.
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    std::fs::create_dir_all("target/previews").unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/import-before-click.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).import_selected_count(cx), 40);
        window.click(check(0), cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            39,
            "Checkbox pointer clicks must change the selection"
        );
        window.click(check(0), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 40);
        assert!(
            window.try_find("import-auto-0").is_none(),
            "Source controls are retired"
        );
        for _ in 0..8 {
            window.scroll(
                "import-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-400.))),
                cx,
            );
            window.render_frame(cx);
        }
        assert!(
            window.find(check(39)).bounds().size.height > px(0.),
            "Scroll must reach the last candidate"
        );
        window.click(check(39), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 39);
        assert!(
            window.find(check(39)).bounds().size.height > px(0.),
            "Toggling must preserve scroll position"
        );
        assert!(
            window.try_find("import-check-39-2").is_none(),
            "Shared projects appear only once"
        );
        let count = view.read(cx).project_count();
        window.click("import-refresh", cx);
        window.render_frame(cx);
        assert!(view.read(cx).import_loading(cx));
        assert_eq!(
            window.find("import-status").label(),
            Some("Pulling latest projects from Herdr, Codex, and Claude Code…")
        );
        // A second native pointer click while loading must not start another scan.
        window.click("import-refresh", cx);
        assert_eq!(
            view.read(cx).project_count(),
            count,
            "Scanning must not add projects"
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).import_loading(cx));
        assert_eq!(
            window.find("import-status").label(),
            Some("40 projects found · Most recently updated first")
        );
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            39,
            "Unchanged refresh preserves deselection"
        );
        assert!(
            window.find(check(39)).bounds().size.height > px(0.),
            "Refresh preserves scrolling"
        );
        window.click("import-skip", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).import_is_open());
        assert!(
            view.read(cx).discovery_preferences().excluded.is_empty(),
            "Cancel must not save selection changes"
        );
        assert!(view.read(cx).discovery_preferences().onboarding_completed);
    })
    .unwrap();
    std::fs::create_dir_all("target/previews").unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.open_import_fixture(snapshot.clone(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("import-clear", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 0);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/diffy-import.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("import-apply", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).import_is_open());
    })
    .unwrap();
    // A refresh may reorder rows or shrink the list without losing path identity.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.open_import_fixture(snapshot.clone(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    settle(&mut cx);
    let mut reordered = snapshot.clone();
    reordered.candidates[39].updated_at = Some(SystemTime::UNIX_EPOCH);
    reordered.errors[1] = Some("One metadata source could not be read.".into());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("import-select-all", cx);
        window.render_frame(cx);
        window.click(check(0), cx);
        view.update(cx, |v, _| v.set_import_fixture(reordered.clone()));
        window.click("import-refresh", cx);
        window.render_frame(cx);
        assert!(view.read(cx).import_loading(cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_paths().len(), 40);
        assert_eq!(
            view.read(cx).import_paths()[0],
            reordered.candidates[39].path
        );
        assert_eq!(view.read(cx).import_selected_count(cx), 39);
        window.click(check(0), cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            40,
            "Deselection followed the path after reordering"
        );
        assert!(window.try_find("import-error-1").is_some());
        let mut small = reordered.clone();
        small.candidates.retain(|c| {
            c.path == snapshot.candidates[0].path || c.path == snapshot.candidates[39].path
        });
        view.update(cx, |v, _| v.set_import_fixture(small));
        for _ in 0..8 {
            window.scroll(
                "import-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-400.))),
                cx,
            );
            window.render_frame(cx);
        }
        window.click("import-refresh", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_paths().len(), 2);
        assert_eq!(view.read(cx).import_selected_count(cx), 2);
        assert!(
            window.find(check(39)).visible(),
            "Shrinking results clamps the old scroll offset"
        );
        window.click("import-refresh", cx);
        window.render_frame(cx);
        assert!(view.read(cx).import_loading(cx));
    })
    .unwrap();
    let old_generation = cx.update(|cx| view.read(cx).import_generation());
    cx.update_window(handle, |_, window, cx| {
        window.click("import-skip", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).import_is_open());
        view.update(cx, |v, cx| {
            v.open_import_fixture(Snapshot::default(), window, cx)
        });
        // Simulate completion of the cancelled generation after a new picker opens.
        view.update(cx, |v, cx| {
            v.complete_import_fixture(old_generation, snapshot.clone(), cx)
        });
        window.render_frame(cx);
        assert!(
            view.read(cx).import_paths().is_empty(),
            "Stale completion cannot replace a reopened picker"
        );
    })
    .unwrap();
    settle(&mut cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).import_paths().is_empty());
        window.click("import-refresh", cx);
        window.render_frame(cx);
        window.click("import-skip", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert!(!view.read(cx).import_is_open()));

    // Real repositories enter the sidebar only when explicitly imported.
    let first = fixture::Fixture::new("import-first");
    let second = fixture::Fixture::new("import-second");
    let paths = [&first.0, &second.0].map(|p| std::fs::canonicalize(p).unwrap());
    let real = Snapshot {
        candidates: paths
            .iter()
            .map(|path| Candidate {
                path: path.clone(),
                repository: Some(path.join(".git")),
                sources: BTreeSet::from([Source::Codex]),
                error: None,
                updated_at: None,
            })
            .collect(),
        errors: Default::default(),
    };
    let (count, active) = cx.update(|cx| {
        (
            view.read(cx).project_count(),
            view.read(cx).active_project_id(),
        )
    });
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_import_fixture(real.clone(), window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    settle(&mut cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).project_count(), count);
        assert_eq!(view.read(cx).import_selected_count(cx), 2);
        window.click(format!("import-check-{}", paths[0].display()), cx);
        window.render_frame(cx);
        window.click("import-apply", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).project_count(), count + 1);
        assert_eq!(view.read(cx).active_project_id(), active);
        assert!(
            view.read(cx)
                .discovery_preferences()
                .excluded
                .contains(&paths[0])
        );
        view.update(cx, |v, cx| v.open_import_fixture(real.clone(), window, cx));
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            0,
            "Skipped and existing projects start unselected"
        );
    })
    .unwrap();
    settle(&mut cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find(format!("import-check-{}", paths[1].display()))
                .is_none(),
            "Existing projects cannot be selected again"
        );
        window.click("import-skip", cx);
    })
    .unwrap();
    // Discovery filtering must not prevent manually opening a temporary checkout.
    first.write("manual.txt", "manually opened temporary project\n");
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_paths(vec![paths[0].clone()], window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).project_count(), count + 2);
    })
    .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        let ready = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                !view.read(cx).is_busy() && view.read(cx).selected_preview_path().is_some()
            })
            .unwrap();
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).selected_preview_path().unwrap().as_ref(),
            "manual.txt"
        )
    });
    println!(
        "Import refresh, unified ordering, selection, cancellation, and explicit import checks passed."
    );
}
