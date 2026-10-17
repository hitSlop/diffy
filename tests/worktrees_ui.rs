#[path = "../benches/support/fixture.rs"]
mod fixture;
use diffy::{app, state};
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, Role, WindowBounds, WindowOptions, assets::Assets,
    base::test_support::snapshots, px, size, test::TestWindowExt as _,
};
use std::{path::Path, sync::Arc, time::Duration};

fn saved(path: &Path, name: &str, color: u32, staged: bool) -> state::SavedProject {
    state::SavedProject {
        path: path.to_owned(),
        name: Some(name.into()),
        color,
        icon: Some(state::ProjectIcon::Emoji("🌿".into())),
        staged,
        split: true,
        selected: [None, None],
    }
}

fn main() {
    let main = fixture::Fixture::new("worktree-ui-main");
    let other = fixture::Fixture::new("worktree-ui-other");
    main.write("selected.txt", "base\n");
    main.git(&["add", "."]);
    main.git(&["commit", "-m", "initial"]);
    main.write("selected.txt", "main change\n");
    other.write("other.txt", "other\n");
    let storage = std::env::temp_dir().join(format!("diffy-worktree-ui-{}", std::process::id()));
    std::fs::create_dir_all(&storage).unwrap();
    let linked = storage.join("linked");
    main.git(&[
        "worktree",
        "add",
        "-b",
        "feature/worktrees",
        linked.to_str().unwrap(),
    ]);
    std::fs::write(linked.join("selected.txt"), "linked change\n").unwrap();
    let mut initial = state::SavedState {
        active: Some(main.0.clone()),
        projects: vec![
            saved(&main.0, "Project", 0x74d9b5, false),
            saved(&linked, "Legacy task", 0xff8175, false),
            saved(&other.0, "Other", 0x82b6e5, false),
        ],
        ..Default::default()
    };
    initial.discovery.onboarding_completed = true;
    let prefs = storage.join("state.json");
    state::save(&prefs, &initial).unwrap();
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
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(vec![], false, Some(prefs.clone()), window, cx)
                    })
                },
            )
        })
        .unwrap();
    let wait = |cx: &mut HeadlessAppContext| {
        for _ in 0..600 {
            cx.advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            let ready = cx
                .update_window(handle, |_, window, cx| {
                    window.render_frame(cx);
                    view.read(cx).worktrees_ready() && !view.read(cx).is_busy()
                })
                .unwrap();
            if ready {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Worktree state did not settle");
    };
    let dispatch = |cx: &mut HeadlessAppContext, action: Box<dyn gpui_kit::Action>| {
        cx.update_window(handle, |_, window, cx| window.dispatch_action(action, cx))
            .unwrap();
        cx.run_until_parked();
    };
    wait(&mut cx);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            view.read(cx).project_count(),
            2,
            "Legacy checkout entries group into one rail entry"
        );
        assert_eq!(view.read(cx).worktree_paths().len(), 2);
        assert_eq!(
            view.read(cx)
                .project_changed_count(view.read(cx).active_project_id().unwrap()),
            Some(2),
            "Badge counts are combined across independent checkout indexes"
        );
        let tabs = snapshots(window)
            .into_iter()
            .filter(|s| s.role() == Some(Role::Tab))
            .filter_map(|s| s.label().map(str::to_owned))
            .collect::<Vec<_>>();
        assert!(tabs.contains(&"main".to_owned()), "{tabs:?}");
        assert!(
            tabs.contains(&"Legacy task".to_owned()),
            "Legacy labels survive migration: {tabs:?}"
        );
        assert!(window.try_find(("project", 2usize)).is_none());
    })
    .unwrap();
    let toolbar_y = cx
        .update_window(handle, |_, window, _| {
            f32::from(window.find("staged").bounds().origin.y)
        })
        .unwrap();
    dispatch(&mut cx, Box::new(app::NextProject));
    wait(&mut cx);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            f32::from(window.find("staged").bounds().origin.y),
            toolbar_y,
            "Single/multiple checkouts share a fixed header height"
        );
        assert_eq!(
            snapshots(window)
                .into_iter()
                .filter(|s| s.role() == Some(Role::Tab))
                .count(),
            1,
            "The project header remains present for a single checkout"
        );
    })
    .unwrap();
    dispatch(&mut cx, Box::new(app::PreviousProject));
    wait(&mut cx);
    let main_id = cx.update(|cx| view.read(cx).active_project_id().unwrap());
    cx.update_window(handle, |_, window, cx| window.press("secondary-right", cx))
        .unwrap();
    wait(&mut cx);
    let linked_id = cx.update(|cx| view.read(cx).active_project_id().unwrap());
    assert_ne!(main_id, linked_id);
    cx.update_window(handle, |_, window, cx| window.press("secondary-left", cx))
        .unwrap();
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(main_id)
    );
    cx.update_window(handle, |_, window, cx| window.press("secondary-left", cx))
        .unwrap();
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(linked_id),
        "Previous worktree wraps from the main checkout"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("secondary-f", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input("selected", cx);
        window.press("secondary-left", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(linked_id),
        "Command-left moves the cursor in a text input"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("secondary-right", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(linked_id),
        "Command-right moves the cursor in a text input"
    );
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(app::FocusDiff), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("secondary-right", cx);
    })
    .unwrap();
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(main_id),
        "Next worktree wraps while the diff has focus"
    );
    cx.update_window(handle, |_, window, cx| window.press("secondary-right", cx))
        .unwrap();
    wait(&mut cx);
    dispatch(&mut cx, Box::new(app::StageAll));
    wait(&mut cx);
    main.git(&["diff", "--quiet", "--cached"]); // Linked staging leaves the main index alone.
    let linked_index = std::process::Command::new("git")
        .arg("-C")
        .arg(&linked)
        .args(["show", ":selected.txt"])
        .output()
        .unwrap();
    assert_eq!(linked_index.stdout, b"linked change\n");
    assert_eq!(
        std::fs::read(main.0.join("selected.txt")).unwrap(),
        b"main change\n"
    );
    dispatch(&mut cx, Box::new(app::NextProject));
    wait(&mut cx);
    dispatch(&mut cx, Box::new(app::PreviousProject));
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(linked_id),
        "Rail navigation remembers each repository's selected checkout"
    );
    // A new checkout arrives through the topology watcher without reopening the project.
    let added = storage.join("added");
    main.git(&[
        "worktree",
        "add",
        "-b",
        "another-branch",
        added.to_str().unwrap(),
    ]);
    for _ in 0..600 {
        cx.advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        if cx.update(|cx| view.read(cx).worktree_paths().len()) == 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    wait(&mut cx);
    assert_eq!(cx.update(|cx| view.read(cx).worktree_paths().len()), 3);
    // Removing the selected worktree must select an available sibling and reject late replies.
    main.git(&["worktree", "remove", "--force", linked.to_str().unwrap()]);
    for _ in 0..600 {
        cx.advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        if cx.update(|cx| view.read(cx).worktree_paths().len()) == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).active_project_id()),
        Some(main_id)
    );
    assert_eq!(cx.update(|cx| view.read(cx).worktree_paths().len()), 2);
    // Save/restore uses the shared appearance, preserving legacy files without a schema bump.
    for _ in 0..300 {
        cx.run_until_parked();
        if state::load(&prefs).is_ok_and(|s| s.repositories.len() == 2 && s.projects.len() == 3) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let persisted = state::load(&prefs).unwrap();
    assert_eq!(persisted.repositories.len(), 2);
    let group = persisted
        .repositories
        .iter()
        .find(|r| r.checkouts.contains(&main.0.canonicalize().unwrap()))
        .unwrap();
    assert_eq!(group.name, "Project");
    assert_eq!(group.color, 0x74d9b5);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    std::fs::create_dir_all("target/previews").unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/worktrees.png")
        .unwrap();
    // Restore the grouped schema into a narrow window using independent storage.
    let overflow = storage.join("overflow");
    let overflow_branch = "zzz/a-very-long-worktree-branch-that-requires-the-overflow-menu";
    main.git(&[
        "worktree",
        "add",
        "-b",
        overflow_branch,
        overflow.to_str().unwrap(),
    ]);
    std::fs::write(overflow.join("selected.txt"), "overflow preview\n").unwrap();
    let restore_prefs = storage.join("restored.json");
    state::save(&restore_prefs, &persisted).unwrap();
    let (narrow_handle, restored) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(480.), px(560.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(
                            vec![],
                            false,
                            Some(restore_prefs.clone()),
                            window,
                            cx,
                        )
                    })
                },
            )
        })
        .unwrap();
    for _ in 0..600 {
        cx.advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        let ready = cx
            .update_window(narrow_handle, |_, window, cx| {
                window.render_frame(cx);
                restored.read(cx).worktrees_ready() && !restored.read(cx).is_busy()
            })
            .unwrap();
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.update_window(narrow_handle, |_, window, cx| {
        assert_eq!(restored.read(cx).project_count(), 2);
        assert_eq!(restored.read(cx).worktree_paths().len(), 3);
        let tabs = snapshots(window)
            .into_iter()
            .filter(|s| s.role() == Some(Role::Tab))
            .collect::<Vec<_>>();
        assert!(
            tabs.iter()
                .any(|s| s.label() == Some("main") && s.selected() == Some(true)),
            "Main project selection survives restart"
        );
        window.press("secondary-right", cx);
        window.render_frame(cx);
        window.press("secondary-right", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(narrow_handle, |_, window, cx| {
        window.render_frame(cx);
        let selected = snapshots(window)
            .into_iter()
            .find(|s| s.role() == Some(Role::Tab) && s.selected() == Some(true))
            .unwrap();
        assert_eq!(selected.label(), Some(overflow_branch));
        assert!(
            selected.bounds().left() < window.find("file-search").bounds().left(),
            "Keyboard selection reveals the overflowing worktree tab"
        );
    })
    .unwrap();
    cx.capture_screenshot(narrow_handle)
        .unwrap()
        .save("target/previews/worktrees-narrow.png")
        .unwrap();
    dispatch(&mut cx, Box::new(app::RemoveCurrentProject));
    cx.update(|cx| {
        assert_eq!(view.read(cx).project_count(), 1);
        assert!(
            view.read(cx)
                .discovery_preferences()
                .excluded_repositories
                .contains(&main.0.join(".git").canonicalize().unwrap())
        );
    });
    assert!(
        main.0.exists() && added.exists(),
        "Remove Project changes only saved app entries"
    );
    println!("Worktree migration, layout, navigation, staging, topology, and removal passed");
    drop(cx);
    let _ = std::fs::remove_dir_all(storage);
}
