# <img src="assets/icon/icon.iconset/icon_128x128.png" alt="Diffy chameleon" width="40" height="40" align="middle"> Diffy

[Downloads and installation](https://diffy.hitslop.com/downloads) · macOS **beta** · Windows/Linux **alpha**

![Diffy on macOS: a split Rust diff, project change badges, a searchable file list grouped by directory, and project import](assets/screenshot.png)

A small native Rust/GPUI Kit diff viewer. macOS is in beta; Windows and Linux are early alphas with limited hardware testing. One purchase supports every platform: **$14.99 during beta**, regular price ~~$24.99~~. Windows/Linux download links become available after release validation and publication.

- Pure-black surfaces and colored squircle project icons that grow gently on hover. Badges count unique changed files across Staged and Changes (partially staged files count once).
- Bundled Commit Mono Regular and Medium throughout the app, with programming ligatures disabled.
- A compact alphabetical file list on the right and one selected diff in the center. Files are grouped under their folder. A nested folder reads as `src · app`, with the ancestor dimmed and shortened in the middle when it is long. Filenames are indented under that header and shortened in the middle when needed. Root files stay flush. Hover shows the complete path. Find file, under the scope header, filters the list with nucleo. ⌘F focuses it. Stage all / Unstage all still cover the whole scope.
- **Staged** first, then **Changes**, with separate counts. Changes is selected initially.
- A fixed project header shows a small project icon and the project name, with linked worktrees alongside it. Staged/Changes and review controls stay in the row below for every project. Attached checkout tabs use branch names; detached checkouts are marked explicitly. Hover shows the checkout path and branch, and the overflow menu keeps long lists accessible.
- Checkouts sharing one Git common directory occupy one rail entry, with a combined badge. Each checkout retains its own scope, selected file, mode, and reading position. The rail remembers the last selected checkout; the palette provides worktree selection and Next/Previous worktree commands. Worktree creation/removal refreshes automatically. Removing a project removes its saved checkout group and leaves it unselected on future imports without changing Git files.
- Empty diff panes include a little Snake game while you wait for changes. Enter starts or pauses it; arrow keys or WASD steer. Switching checkouts or Staged/Changes pauses the game, and reduced motion disables its animation.
- Staged compares HEAD with the index; Changes compares the index with disk and includes untracked files. A file can appear in both views.
- Unified or split diffs with highlighting, inline changes, expandable context, source selection, and copying. Long lines wrap; split mode gives 40% to the original and 60% to the modified source. Added files fill one pane. Red and green marks beside the scrollbar show where further changes appear when the diff needs scrolling.
- Hover **+ / −** actions in file rows and the preview header for whole-file staging/unstaging. Row actions affect that row without selecting it.
- Sticky **Stage all / Unstage all** below the file list runs one Git operation for the current scope; it is also available in the command palette.
- A searchable **⌘P** palette for switching projects and executing commands.
- Right-click a file or its preview path to copy its relative/absolute path or reveal it in Finder. Deleted files reveal the nearest existing parent.
- **⌘+ / ⌘− / ⌘0** increase, decrease, or reset the diff font size. The View menu and palette also toggle line wrapping; both reading settings are remembered.
- Opened repositories, their order, active project, scopes, diff modes, selected files, custom names, icons, and sidebar width are remembered across launches. Drag the sidebar divider to resize it; the handle appears on hover.
- Right-click a project for **Edit Project…**, **Open in Finder**, **Remove Project**, and shortcuts to installed terminals and editors. Diffy detects iTerm, Warp, Ghostty, Terminal, VS Code, VS Code Insiders, Cursor, and Zed through macOS, including apps installed outside `/Applications`. Detection refreshes when Diffy regains focus. Open actions target the clicked project and follow the destination app's window/tab preferences.
- **Edit Project…** lets you rename it, change its color with GPUI Kit's color picker, enter an emoji (⌃⌘Space opens the macOS picker), or choose an image. Selected images are copied into Diffy's own storage in the background, so moving or deleting the original won't remove the icon. Images use rounded square crops and stay fixed in size during hover.
- Removing from Diffy only removes the saved entry. It never deletes a project or changes its Git state.
- Holding ⌘↑/↓ advances through files at approximately five per second after a brief initial delay; individual taps stay immediate. macOS holds use a timer and physical key-state checks, independent of OS shortcut autorepeat. Navigation preferences save after a short idle debounce.
- Drag the project header or unused toolbar area to move the window. A short repository path and branch sit on the left of the footer, with keyboard hints on the right; no app title, change description, commit, approval, or messaging controls.

## Run

Requires Rust 1.92+ and the native platform toolchain. macOS needs Xcode Command Line Tools; Windows needs Visual Studio 2022 C++ Build Tools, the Windows SDK, and CMake with the MSVC Rust toolchain; Linux needs the development libraries listed in RELEASING.md. Git must be installed for repository mode.

```sh
cargo run --locked
cargo run --locked -- /path/to/repository /path/to/another/repository
cargo run --locked -- --demo
```

Normal launch restores repositories. The project import picker appears once, including for existing users. Use **+**, **⌘O**, or the command palette to open repositories. Nested folders resolve to their Git root and duplicate roots are merged. Missing repositories remain removable and show a recoverable error.

## Import projects

Use **Import** at the bottom of the project rail, **File → Import Projects…**, or the command palette to find local Git projects from Herdr, Codex, and Claude Code. Select the projects to add; repositories already in Diffy are marked **Already added**. Unavailable directories are hidden. Results appear once per checkout in one list, ordered by the latest tool activity; non-Git folders and repositories in system temporary directories are hidden too. Temporary repositories can still be opened manually, and already-added projects are preserved. Older sessions without a stored working directory are skipped.

Discovery runs when you open Import or press Refresh and shows “Pulling latest projects from Herdr, Codex, and Claude Code…” while scanning. Select projects and press **Import** to add them; discovery never adds projects on its own. Removed and unselected projects stay unselected until you explicitly choose them or open them again. Imported repositories remain in Diffy when their Herdr workspace closes or agent history disappears. Importing preserves the project you are viewing.

Discovery reads directory metadata only. It does not run agents, resume sessions, read terminal output, or access credentials. It honors `HERDR_SOCKET_PATH`, `XDG_CONFIG_HOME`, `CODEX_HOME`, and `CLAUDE_CONFIG_DIR`; otherwise it uses each tool's standard local directories. Herdr's saved sessions provide an offline fallback. Recency uses session metadata modification times across all three sources, including saved metadata for live Herdr sessions; projects without a timestamp appear last.

`--demo` opens sample projects without reading or writing saved preferences. Demo staging is disabled.

## GitHub pull requests

Open a local repository with a GitHub.com remote, then choose **Pull requests** beside the project name. **Worktrees** returns to the local checkout with its selection, scope, and reading position preserved. The PR list includes all open pull requests and drafts, most recently updated first. Tabs show the PR number and shortened title; hover for details. The **…** menu searches loaded titles and numbers, offers **Load more** after each page of 100, and lets you choose a remote. Diffy prefers `origin`; with several remotes and no `origin`, choose one from that menu.

Install [GitHub CLI](https://cli.github.com/) and run `gh auth login --hostname github.com` first. Diffy uses that login for Octocrab's GitHub API requests, including private repositories you can access. Credentials and patches are never written to preferences. Login, permission, connection, and rate-limit errors offer a retry; **Open in GitHub** opens the selected PR in your browser.

PR review is read-only: it doesn't check out branches, stage files, or change your local index. Local file watchers keep working independently. **⌘R** refreshes the loaded PR pages and selected diff; remote updates aren't polled automatically. The existing split/unified viewer, file search, and copying work with the patch supplied by GitHub. Extra source context beyond that patch is unavailable. Patches over 16 MiB or parsed previews over 32 MiB show a message to open the PR in GitHub; previews share the existing bounded cache. GitHub Enterprise hosts, review comments, approvals, merging, and native sign-in aren't included.

## Terminal command

In the installed macOS app, choose **File → Install terminal command…**, or search for **Install terminal command** in the command palette. This installs `~/.local/bin/diffy` for your user. Existing commands from other tools are preserved.

If your terminal cannot find it, add this line to your shell configuration (usually `~/.zshrc`) and open a new terminal:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

```sh
diffy .
diffy /path/to/repository /path/to/another/repository
diffy --check .
diffy --version
```

`diffy .` adds or selects the repository in the existing Diffy window and brings it forward. A closed window is recreated; a stopped app is launched. Relative paths are resolved in your terminal, and nested folders resolve to the repository root. `diffy` without paths opens your saved projects. Reinstall the command if you move `Diffy.app`; normal updates at the same location keep it working.

CLI installation supports all three desktop targets. Linux installs `~/.local/bin/diffy`, pointing to the outer AppImage when packaged. Windows copies a managed console launcher to its local app-data directory and shows instructions for adding the directory to your user Path. The Windows installer includes `diffy-cli.exe` beside the GUI executable. Development binaries accept repository paths directly through `cargo run`. Windows/Linux invocations forward requests to the existing app through a bounded per-user local socket; demo mode stays isolated.

## Keyboard shortcuts

Use **Ctrl** instead of **⌘** on Windows and Linux. Menu and footer hints adapt to the platform.

| Shortcut | Action |
|---|---|
| ⌘P | Search projects and commands |
| ⌘O | Open repositories |
| ⌘⇧↓ / ⌘⇧↑ | Next / previous project |
| ⌘→ / ⌘← | Next / previous worktree (wraps within the project) |
| ⌘↓ / ⌘↑ | Next / previous file |
| ⌘ + vertical scroll | Move between files |
| ⌘F | Find a changed file |
| ⌘⇧S | Toggle Staged / Changes |
| ⌘S | Stage file in Changes; unstage file in Staged |
| ⌘R | Refresh repository |
| ⌥↓ / ⌥↑ | Next / previous change |
| ⌘Enter | Focus the diff |
| ⌘C | Copy selected source |
| Arrow keys | Navigate the focused file list, palette, or diff |
| ⌘Q | Quit |

In the palette, Enter executes; Escape clears a nonempty query, then closes. Clicking outside dismisses it.

Each project remembers its scope; each scope retains its selection and viewport during the session. When staging removes the selected file, Diffy selects the next remaining file, with a previous-file fallback at the end. Background operations retain user navigation where possible and block duplicate writes.

Unmerged files remain readable with their conflict markers. Resolve and stage them outside Diffy; bulk staging skips them while other files stay usable.

Git writes affect the selected file's index entries only. Unstaging preserves working-tree contents, including on unborn branches. Rename actions include both paths; copy actions affect the destination only. Errors retain the last good preview.

Preferences live at `~/Library/Application Support/Diffy/state.json` on macOS, `$XDG_CONFIG_HOME/diffy/state.json` (default `~/.config/diffy/state.json`) on Linux, and `%LOCALAPPDATA%/hitSlop/Diffy/config/state.json` on Windows and contain paths and viewer settings, never patches or source contents. Imported project images live beside it in `project-icons/`, named by their SHA-256 contents so matching filenames cannot overwrite other icons. Back up the folder to keep both preferences and images. Existing image references remain readable; selecting an image again copies it into managed storage. Tests inject temporary storage; `DIFFY_STATE_PATH` can override the path for isolated development sessions and also relocates the icon folder.

## Build a macOS app

```sh
./scripts/bundle-macos.sh
open dist/Diffy.app
```

The default bundle uses a development build; `--release` builds an optimized app. When the project's Developer ID certificate is available in the login keychain, local bundles use it so macOS can remember folder permissions across rebuilds. Other machines fall back to ad-hoc signing, which may prompt again after rebuilding. `DIFFY_CODESIGN_IDENTITY` selects another installed signer (or `-` for ad-hoc); `DIFFY_CODESIGN_KEYCHAIN` overrides the keychain path. These bundles are local development builds, without notarization or release publishing.

## Verify

```sh
cargo test --locked
cargo clippy --locked --features ui-tests --all-targets -- -D warnings
cargo run --locked -- --check
cargo run --locked -- --check /path/to/repository
cargo test --locked --features ui-tests --test ui
```

Git tests run in disposable repositories and cover independent scopes, index preservation during reads, literal filenames, renames, deletions, copies, unborn HEAD, binary additions, and index-lock failures. Preference tests cover serialized background saves, latest-state flushing, atomic saves, restoration, invalid state, and removal without changing repository files. Native UI tests exercise the actual offscreen Metal renderer, palette commands, hover actions, staging shortcuts, reduced motion, restoration, empty states, large-file scrolling, and automatic edit/index detection; PNG previews go to `target/previews`.

Repositories refresh automatically after file and Git metadata changes, with a short debounce. Linked worktree metadata is watched too; ⌘R remains available. Background edits preserve the visible source-line anchor when it still exists, and unrelated file changes retain the current preview. Diff panes are bounded, and overflow scrollbars stay visible. A narrow GPUI Component patch fixes offscreen row estimates for large-file scrolling; see [vendor/README.md](vendor/README.md).

File lists refresh from Git metadata. Only the active file’s full patch loads in the background; inactive previews release source contents while retaining a reading anchor. Parsed previews and highlighting share bounded caches with a combined 64 MiB estimated budget. Remote machines, advanced diff engines, and exact scroll-position restoration across launches remain later work.

The read-only gix metadata backend ships behind an internal rollout switch. Git remains the default. To try it:

```sh
DIFFY_GIT_BACKEND=gix cargo run --locked -- /path/to/repository
DIFFY_GIT_BACKEND=gix cargo run --locked -- --check /path/to/repository
```

`--check` reports which backend actually supplied the listing. Unset the variable or set `DIFFY_GIT_BACKEND=git` to use Git throughout. Native read errors and unsupported cases automatically fall back to Git: conflicts, submodules, sparse checkout, intent-to-add entries, inherited repository/config environment overrides, embedded repositories, uncertain rename matches, and changed files with content-conversion attributes or enabled `core.autocrlf`. Both backends use the same cache revision keys and watch directories. Branch labels, source previews, ignore checks for watcher events, and staging/unstaging continue to use Git. The gix status reader never writes suggested index updates. See [the performance report](docs/performance.md) for compatibility coverage and rollout criteria. Full-client and SSH feasibility findings are in [Git studies](docs/git-studies.md).

Built with [GPUI Kit](https://gpui-kit.com/) and [Commit Mono](https://commitmono.com/). See [plan.md](plan.md) and [third-party notices](THIRD_PARTY_NOTICES.md).

## macOS release

Diffy uses the bundle identifier `com.hitslop.diffy`. The macOS beta targets Apple Silicon Macs running macOS 13 or later. Windows x86-64 NSIS and Linux x86-64 AppImage alphas use the same release version. Linux initially targets Ubuntu 24.04+ with a working Vulkan driver; other distributions and ARM targets are not yet verified. Source code is MIT licensed; dependency and font licenses remain in effect.

Existing preferences are migrated from `~/Library/Application Support/Differ/state.json` only when no Diffy preferences exist. The original file is retained. `DIFFY_STATE_PATH` takes precedence over the legacy `DIFFER_STATE_PATH` override; explicit overrides skip migration.

Packaged builds check for updates quietly after startup. **Check for Updates…** is also available in the Diffy menu and ⌘P command palette. Updates download and verify only after clicking **Update and Restart**. Git writes finish before installation; projects and preferences survive the restart. Development binaries cannot install updates. Windows installs verified NSIS updates and relaunches; Linux replaces and relaunches the outer AppImage. Windows publisher signing is not configured for the alpha, separately from updater payload signatures. Updates require a writable installation location.

See [RELEASING.md](RELEASING.md) for packaging, local signing, key backups, draft releases, and update verification.

## Performance checks

```sh
cargo bench --locked --bench performance
DIFFY_BENCH_REPORT=1 cargo bench --locked --bench performance
DIFFY_TRACE_PATH="$PWD/target/diffy-trace.json" cargo run --locked -- /path/to/repository
```

The benchmark compares eager loading, Git metadata refresh, selected-file parsing, cached navigation, and the complete gix metadata adapter on disposable repositories. `DIFFY_BENCH_ONLY=metadata` (or `eager`, `selected_preview`, `cached_navigation`, `gix_metadata`, `gix_status_prototype`) isolates a case for memory measurements. The gix adapter benchmark fails if it falls back to Git; the earlier filename-only prototype remains a separate comparison. Traces are opt in, stay local, and contain timing spans without paths or source contents. Open them in Perfetto. gix is a runtime dependency; Criterion remains development-only.

## Windows and Linux packages

Native build and package checks run in `.github/workflows/desktop.yml`. See [RELEASING.md](RELEASING.md) for NSIS/AppImage packaging, updater signatures, release assembly, and validation. `directories` supplies native settings locations; `interprocess` and an OS file lock coordinate terminal requests. Windows/Linux are alpha until broader hardware testing is available.
