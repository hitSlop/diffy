# Third-party notices

Diffy uses GPUI Kit 0.7.3, developed by Longbridge, under the Apache License 2.0. Its application setup and background repository-loading patterns were informed by the GPUI Kit `tig` example.

- GPUI Kit: https://github.com/longbridge/gpui-kit
- Tig example: https://github.com/longbridge/gpui-kit/tree/main/examples/tig
- GPUI, developed by Zed Industries: https://github.com/zed-industries/zed
- Apache License 2.0: https://www.apache.org/licenses/LICENSE-2.0

GPUI Kit's default assets include Lucide/Isocons icons. Dependency source packages retain their own license and notice files. Cargo.lock records the exact dependency versions used by this application.

The reference repositories under `_docs` are research inputs and are not bundled with Diffy. No Difftastic, Livediff, OmniDiff, or Herdr code is included in the app.

Commit Mono 1.143 Regular (400) and Medium (500), designed by Eigil Nikolajsen, are bundled from the unmodified upstream OTF files. Fonts are licensed under the SIL Open Font License 1.1; the complete license is in [assets/fonts/LICENSE](assets/fonts/LICENSE).

- Commit Mono: https://commitmono.com/
- Font source: https://github.com/eigilnikolajsen/commit-mono/tree/main/src/fonts/fontlab

The `_docs/commit-mono-main` reference copy is not bundled; the two font files and their license are copied into `assets/fonts` and embedded in the executable.

Diffy vendors GPUI Component 0.7.3 with narrowly documented scroll-range, live reading-position, full-width addition, split-width, and scrollbar change-marker changes. The Apache-2.0 license is retained at `vendor/gpui-component/LICENSE-APACHE`; changes are described in [vendor/README.md](vendor/README.md).

Diffy uses nucleo-matcher 0.3.1 (MPL-2.0) to filter the changed-file list. The license text ships with the packaged dependency inventory.

- nucleo-matcher: https://github.com/helix-editor/nucleo
- Mozilla Public License 2.0: https://www.mozilla.org/en-US/MPL/2.0/

Diffy uses gix 0.89.0 (MIT/Apache-2.0) for the opt-in read-only repository metadata backend. Its dependency inventory and available upstream license texts ship with the packaged app.

- gix: https://github.com/GitoxideLabs/gitoxide

Diffy uses Octocrab 0.54.5 (MIT/Apache-2.0) for read-only GitHub pull request access, with Tokio for asynchronous networking. The packaged dependency inventory includes their licenses.

- Octocrab: https://github.com/XAMPPRocky/octocrab
- Tokio: https://github.com/tokio-rs/tokio

Diffy uses cargo-packager (MIT/Apache-2.0) for packaging and cargo-packager-updater (MIT/Apache-2.0) for signed updates. The updater is pinned to revision `0a3ea3a089363b7e77dfca238be5bbcf3f4d75f6` for HTTPS enforcement.

- Source: https://github.com/crabnebula-dev/cargo-packager

Packaged apps include an exact dependency inventory and available upstream license, copyright, and notice texts under `Contents/Resources/licenses`. Commit Mono and the vendored GPUI Component license are also included separately.
