# Docker-Goo tasks

## Milestone 1 — current implementation

- [x] 0. Rust/Cargo workspace (completed on the Fedora machine).
- [x] 1. Dependencies and separate config/core/TUI modules.
- [x] 2. Docker endpoint discovery and API connection.
- [x] 3. Full-screen home, large logo, left menu, fixed drawer and section content.
- [x] 4. Daemon state and resource lists.
- [x] 5. Container start/stop/restart/pause/resume and confirmed safe delete.
- [x] 6. Live selected-container logs and CPU/RAM/network totals.
- [x] 7. Inspect and interactive exec shell.
- [x] 8. Keyboard, mouse, search and command bar.
- [ ] 9. Validate against the user's real Fedora Docker Engine.

## Milestone 2 — one task at a time

1. **Implemented:** create/run form with image, name, command, ports, environment, bind mounts, named volumes and shell preset.
2. Image pull progress and confirmed image removal.
3. Volume/network creation and removal, with in-use errors.
4. Prune preview, scope and explicit confirmation.
5. **Implemented:** run an existing Compose YAML via installed Compose plugin and monitor project containers. Additional lifecycle controls remain.
6. Build context selection, Dockerfile options and streamed build output.
7. Linux distribution and terminal compatibility validation, then packaging.

No login, account integration, telemetry, cloud features, AI, Kubernetes, Podman, or graphical UI in these milestones.

## UI and workflow update

- [x] Blue Docker / animated purple Goo wordmark and supplied whale icon.
- [x] Alphabet shortcuts, colored keys and container states.
- [x] Web port picker, shell choices and refresh.
- [x] Docker console with contextual help.
- [x] Kafeyn credit and About / GitHub.
- [ ] Validate new create/attach/browser flows against a live Fedora engine.

- [x] Image chooser before creation, including mouse selection and filtering.
- [x] Ctrl+D terminal detach.
- [x] Clean About wordmark, inline creator credit and all-blue whale.
