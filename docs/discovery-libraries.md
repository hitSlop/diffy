# Project discovery library spike

2026-10-13. Source packages fetched from crates.io and inspected locally.

## Selected: agent-sessions 0.2.2 (MIT)

- `Roots::from_env_for` isolates provider configuration errors and supports custom roots.
- `discover` includes Codex active/archived files, excludes subagents by default, skips symlinks, and reports per-file errors.
- `read_from` with `EventKinds::META` skips message/tool/usage projection and enforces explicit byte budgets.
- Stop after the first cwd. This is intentionally an incomplete metadata prefix, not a complete conversation snapshot; call `finish` without draining.
- Direct dependencies are serde, serde_json, memchr, and chrono. Pin 0.2.2 because 0.3 adds storage/format dependencies unnecessary for project discovery.
- Tests exercise headers, Claude records, huge unread tails, malformed records, Unicode, archived sessions, caching, and a thousand-file fixture.
- Small adapters still read Claude project indexes and offline Herdr workspace state.
- A bounded first-line adapter handles older Codex headers without a typed envelope. Headers without a cwd are normal skips; a stored cwd is recovered without inspecting conversation messages. The local corpus had 500 such older headers, all previously misreported as failures.
- Claude uses a 4 MiB metadata prefix and 2 MiB line limit because file-history snapshots can precede its first cwd record. This recovers both oversized prefixes in the local corpus; scanning stops as soon as cwd is available.

## Rejected: herdr-sdk 0.0.1 (MIT)

Inspected `src/lib.rs` and `src/events.rs`.

- Public Client and Events constructors select their endpoint exclusively from HERDR_SOCKET_PATH.
- Socket path is private; no explicit per-session constructor.
- Unconditional `std::os::unix::net::UnixStream` prevents Windows compilation.
- Requests and subscriptions use unbounded blocking read_line without deadlines or cancellation controls.
- EventKind lacks workspace-created/worktree lifecycle subscriptions needed for project discovery.

## Rejected: herdr-plugin 0.1.8 (MIT OR Apache-2.0)

Inspected `src/runtime.rs` and `src/socket.rs`.

- Socket clients depend on a RuntimeHandle initialized by the plugin runtime.
- `RuntimeHandle::set_socket_path` is crate-private.
- Request IO uses spawn_blocking and unbounded read_line. Timing out the async wrapper does not stop its blocking read.
- A standalone discovery service would need runtime/environment workarounds and transport changes, outweighing the benefit of its typed plugin framework.

## Herdr fallback

Small read-only transport adapter using the same platform forms as Reviewr: UnixStream on Unix, interprocess named pipes on Windows. Explicit endpoint, nonblocking IO, bounded response, total deadline, cancellation checks, and request-id validation. Calls session.snapshot only. Unknown fields are ignored; terminal output and agent state are not used. Each explicit import scan requests a snapshot; there is no background source watcher or automatic import. Saved session modification times provide activity ordering for live results when available; polling never counts as new activity.

The discovery subsystem cross-compiles for x86_64-pc-windows-gnu. This does not constitute a Windows GUI runtime test.
