# Validation results

Environment: Linux x86_64, Rust 1.98.1.

- `cargo build --locked`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- Eleven non-socket tests: passed (context metadata, metrics, CPU calculation, key encoding, log escape filtering, navigation state, small/normal/wide rendering).
- PTY smoke test: passed (alternate-screen entry and clearing, alphabet section keys, About, creation preset, Docker console, quit, restored terminal mode).
- The Unix-socket API fixture test could not run here: the environment rejects binding a Unix socket with `Operation not permitted`. It remains enabled for normal development machines.
- No real Docker daemon was available. Container operations, exec attachment, SSH/TLS and distro compatibility still require a real-engine smoke test.

## On Fedora

```bash
cd ~/Projects/docker-goo
cargo test --locked
docker-goo --check
docker-goo
```

Choose a disposable container to validate start/stop/restart, logs, stats, shell, pause/resume and deletion. Confirm your installed engine is the intended endpoint shown by the application.

Optional automated terminal test, after installing Python's `pyte` package:

```bash
cargo build --locked
python3 tests/pty_smoke.py target/debug/docker-goo
```

## Blue / purple redesign

Rechecked compilation, lint, layout tests and the PTY navigation test. Exported actual Ratatui buffers for home, containers and images and inspected the rendered previews. Preview data is synthetic and labeled; it is not evidence of real Docker integration. The updated backend adds create, web-port discovery and shell attachment.

## Latest workflow update

Compilation and warning-free Clippy passed. Eleven unit/layout tests passed; the explicit visual export also passed. PTY smoke checks cover alphabet navigation, About, creating a form, switching to Shell preset, `$` input, escaping dialogs, and terminal restoration. Rendered home, containers, images, About, creation, and console previews use sample data.

No live-engine validation was possible. On Fedora, additionally check nginx creation with `8080:80`, `w` opening the page, an Ubuntu Shell preset with `b`, and stopped interactive-container attachment. Test `$ docker ps -a` and contextual help with the installed CLI.

## Image selection / Compose update

Added coverage for separate image tags, preserving form fields when changing images, exact Compose project membership and Ctrl+D detach detection. PTY checks include the image chooser, manual-image route, Compose file entry and missing-file error. Actual Compose execution and container attachment still require a real Docker Engine and Compose plugin; neither was available for integration testing here.

## Theme / completion update

Checked command-only completion (including suppressing resource arguments), theme preview/cancel, completion without execution, compilation, Clippy and terminal restoration. Rendered previews include all three themes, About and the suggestion list. Image pull/build timeout increased to 10 minutes; live downloads and real browser launching still require manual verification on Fedora.

## Console upgrade checkpoint — 28 September 2026

- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- Non-socket unit/layout suite: 16 passed; renderer preview separately passed.
- PTY console regression: cursor/delete, bracketed paste, retained output, streamed
  chunks, cwd changes, native terminal handoff and terminal mode restoration passed
  using a fake Docker CLI (no real engine).
- Existing PTY navigation smoke updated for the new console/form labels.
- `tests/release_check.py` compiles; live-engine execution is pending on the user's
  machine. Its engine checks use Docker CLI and its UI checks require observations.
- No claim of a completed real Docker, SSH/TLS, rootless or distribution matrix run.
