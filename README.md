# Docker-Goo

A Linux-first Rust TUI for your existing Docker Engine. No account, cloud, telemetry, or bundled daemon.

The interface uses Docker-blue surfaces, white text and purple accents (`#8711C7`). The all-blue whale icon precedes the redesigned wordmark: **docker stays blue**, while only **Goo** cycles blue/purple from right to left. Large logos use terminal pixels; the drawer uses a compact icon and readable wordmark. Home credits Kafeyn; About includes the project description and GitHub link.

Color roles live in src/tui/theme.rs. Explicit RGB backgrounds replace inherited terminal colors. Terminal-wide opacity can still be controlled by your terminal emulator. Set DOCKER_GOO_STATIC=1 to pause the logo gradient. NO_COLOR requests a monochrome display; unset it to see the blue/purple theme. The font itself is selected in your terminal settings.

Rendered previews with labeled sample data are in previews/.

## Install

Requirements: Rust **1.98+**, a C compiler/linker, and access to a Docker Engine. Build on your own distribution; no GNOME, KDE, X11, Wayland or desktop-specific libraries are required.

```bash
cd ~/Projects/docker-goo
cargo install --path . --locked
export PATH="$HOME/.cargo/bin:$PATH"
docker-goo
```

Keep `~/.cargo/bin` in your shell's PATH to launch from any directory. Development: `cargo run --locked`. The package and installed executable are both named `docker-goo`.

## First milestone

- Full-terminal launch screen with a large original cell-rendered logo and a left-aligned, vertically centered section menu.
- Opening a section keeps a fixed left drawer and compact logo; the right pane shows that section's resources.
- Docker detection, daemon summary, offline status and automatic retries.
- All containers: running/stopped state, image and published ports.
- Start, stop, restart, pause, resume and confirmed deletion of stopped containers.
- Live logs and CPU / memory / network totals for the selected container.
- Images, volumes, networks and discovered Compose projects.
- JSON inspect for each resource type.
- Interactive container shell using the Docker exec API, including terminal resize.
- Keyboard navigation, mouse selection, filtering, Docker console, responsive layouts and NO_COLOR support.
- Container creation, published-web-port picker, terminal choices and About.

The application clears its alternate terminal screen on entry and uses the full terminal. Normal exit restores the previous terminal view. It does not erase your shell history. Standard ANSI/UTF-8 terminals are supported; no Nerd Font is needed. At least 50 columns × 15 rows are required; 100 × 30 or larger is recommended. Small panes hide the detail panel; enlarge the terminal to see it below the resource table.

## Controls

| Key | Action |
| --- | --- |
| Up/Down or j/k | Select section on home/in drawer; select resource in content |
| Enter / Right | Open a section from home or drawer |
| Tab / Left | Focus drawer from content |
| Tab / Shift+Tab | Move between sections while drawer/home is focused |
| c / i / v / t / o | Containers / Images / Volumes / Networks / Compose (1–5 also work) |
| h / Home | Return to launch screen |
| Click logo | Return to launch screen |
| / | Filter resources by name, ID, image or state |
| $ | Docker command console with contextual help |
| n | New container form; prefilled from the selected image |
| w | Choose a published web port and open your default browser |
| a | About / Kafeyn / clickable GitHub button |
| Shift+T | Theme picker: arrows preview, Enter saves, Esc reverts |
| s / x / r | Start / stop / restart container |
| p / u | Pause / resume container |
| d | Confirm deletion of a stopped container |
| b | Container terminal: auto, Bash, sh, or a custom executable |
| Ctrl+D | Detach shell and return to Docker-Goo |
| Enter / e | Inspect selected resource from content |
| PgUp / PgDn | Scroll logs or inspect |
| l / End | Open Logs and follow latest output |
| g | Open Stats |
| f / F5 | Refresh; restart selected streams |
| Esc | Cancel input/dialog; clear filter; focus drawer |
| ? | Help |
| q / Ctrl+C | Quit (Ctrl+C inside a shell goes to that shell) |

The `$` console accepts explicit Docker commands on the selected engine and normal host commands in the displayed local working directory. Output streams live and retains prior results. Enter runs the line when suggestions are hidden; Ctrl+S toggles them. Ctrl+C cancels, Esc closes, and Ctrl+O opens a native host shell. See **Console update** below for editing, scrolling, native interactive Docker sessions and file completion. The console needs the Docker CLI for Docker operations; API resource controls do not.

Create with `n`: first choose an existing local image using arrows + Enter or a mouse click. Type to filter; Ctrl+R refreshes the list; Ctrl+N lets you enter a new image to pull. The next form contains image, optional name/command, ports, environment, mounts, and network. Click the Image field or press Enter there to reopen the picker. Tab/Shift+Tab or arrows select fields. Enter advances on other fields; Enter on **CREATE AND START** submits. Ctrl+P switches Service/Shell presets. Shell enables an interactive TTY and defaults to `/bin/sh`. Missing images are pulled automatically. Port examples: `8080:80,8443:443` (localhost by default) or `0.0.0.0:8080:80`. Environment and mount entries use semicolons. Example mounts: `my-data:/data;/home/me/project:/app:ro`. The form supports IPv4 port bindings; use the console for advanced flags.

`w` lists published TCP ports; select a web service and press Enter. `s` switches HTTP/HTTPS. Requires `xdg-open` and a configured browser. TCP mappings may serve other protocols; no automatic HTTP probing is performed. Remote loopback bindings need a tunnel and are omitted.

`b` offers auto (Bash with sh fallback), Bash, sh, or a typed executable such as `spark-shell`. The executable must exist inside the image. Running containers use exec. Stopped containers with an existing interactive TTY are started and attached to their main process; other stopped containers explain how to create a new Shell-preset container.

Container actions operate on the selected **ID**, not a row number. Delete never forces removal and never removes volumes. Inspect may show environment values supplied to Docker. Logs keep a bounded 1,000-line history; metrics stream only for the selected running container. CPU may exceed 100% on multicore systems. Memory excludes inactive file cache when available. Network values are cumulative receive/transmit bytes, not rates.

## Connection selection

1. `--host` (explicit override)
2. `--context`, then `DOCKER_CONTEXT`
3. `DOCKER_HOST`
4. Current context in `$DOCKER_CONFIG/config.json` (default `~/.docker/config.json`)
5. Rootless socket: `$XDG_RUNTIME_DIR/docker.sock` or `/run/user/<uid>/docker.sock`
6. `/var/run/docker.sock`

An explicitly selected `default` context means `/var/run/docker.sock`. Explicit endpoints and contexts never silently fall back to another daemon. The API version is negotiated before listing resources.

```bash
docker-goo --check
docker-goo --context rootless
docker-goo --host unix:///run/user/1000/docker.sock
docker-goo --shell /bin/bash
```

`--check` prints a daemon/resource summary without starting the TUI. No Docker CLI is required for API-backed controls; the optional `$` console requires it. Unix sockets, HTTP/TCP, certificate-authenticated HTTPS, and SSH endpoints are implemented. TLS uses `DOCKER_TLS_VERIFY` / `DOCKER_CERT_PATH` or context certificates; contexts that disable certificate verification are rejected.

SSH uses your installed `ssh` client, configured key/agent authentication and known_hosts, plus `docker system dial-stdio` on the remote host. Authenticate with SSH beforehand; password prompts are not shown inside the TUI. Remote SSH/Docker failures currently surface as connection errors. The exec shell defaults to `auto` (Bash then sh); images without either need a custom executable. Use `exit` to end a shell process; detaching can leave it running inside the container.

## Architecture

- `src/config/`: CLI arguments and Docker endpoint/context discovery.
- `src/core/`: UI-independent resource models and asynchronous `Engine` contract.
- `src/core/docker/`: Bollard API adapter, metrics, SSH transport and API fixture tests.
- `src/tui/`: navigation, background jobs, Ratatui layouts and shell terminal handoff.
- `src/main.rs`: wiring and headless check.

Docker work runs asynchronously. Requests never execute typed command-bar text in a host shell. The UI imports the `Engine` contract, not Bollard. Other backends can later implement the same resource operations.

## Validation

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Tests cover Docker HTTP over a Unix socket fixture, resource mapping, non-forced deletion, log decoding, CPU calculations, metrics, context loading, shell key encoding, and layouts. These tests do not replace validation with a real Docker Engine. Real Docker daemon, SSH/TLS interoperability and the distribution matrix still need testing on target machines.

## Next tasks

See [TASKS.md](TASKS.md). Compose supports running an existing YAML and monitoring its containers. The create/run form is implemented. Dedicated image management, prune, builds, and additional Compose lifecycle controls remain later milestones; installed CLI commands are available through `$`.

## Compose files

Open `o` Compose, then `y` (or `n`) to enter the path to an **existing** `.yaml` / `.yml`. Press Enter to validate and run it. Docker-Goo does not write or modify YAML. The installed Docker Compose plugin validates with `config --format json`, then runs `up -d` on the selected Docker endpoint. Relative paths and `.env` use Compose's normal rules. Execution has a five-minute limit; on failure or timeout inspect Docker's current state because containers may already have been created.

After success, the project containers appear in the regular container dashboard with logs, live stats, web access and shell controls. In Compose, select any detected project and press Enter to monitor only that project's containers. `c` returns to all containers; `o` returns to projects. Membership uses Docker's Compose project label, not name matching, and refreshes every three seconds.

Ctrl+D is intercepted by Docker-Goo to detach from a container terminal. It is not sent as EOF to the process. Type `exit` to close the shell; a detached exec process may continue running.

## Themes and command completion

`Shift+T` opens three themes: Classic / Royal accents (original dark navy body, layered blue panels and drawer, #002366 accents), Midnight (pure black page background), and Daylight (pure white page background with readable dark text). All three share Classic’s panels, drawer, gradients, and shortcut colors. The selection is saved under `$XDG_CONFIG_HOME/docker-goo/theme` or `~/.config/docker-goo/theme`. `NO_COLOR` still overrides colors.

In `$`, typing a Docker command prefix displays command/subcommand suggestions. Up/Down selects; Enter or Tab **inserts** the suggestion. It does not execute it. Add arguments, then Enter runs the completed command. Commands expecting an image or container also suggest matching local resources in green. Ctrl+S hides or shows suggestions; Esc closes the console. Both controls can be clicked. The output/help background follows the selected theme, with blue reserved for input and suggestions. Examples: `docker pu` and `docker volume cr`. About includes a large wordmark and a clickable `[ w ]` GitHub control; the `w` key also opens it.

The creator credit uses Baby Blue Eyes (#A1CAF1); Kafeyn uses the terminal's bold weight. Terminal fonts control the exact bold appearance. Logo rendering remains terminal-cell based.

Action buttons use Steel blue for New, Sea green for Start, Metallic Orange for Web, red for Stop (`x`), and coordinated colors for the remaining actions. Drawer shortcuts use bold bracketed letters, e.g. `[c] - Containers`.

## Console update — 28 September

Launch directly in the command console from your project directory:

```bash
cd ~/Projects/your-project
docker-goo --console
# Or choose an explicit directory:
docker-goo --console --cwd ~/Projects/your-project
```

The `$` console now displays its local working directory. `cd`, `ls`, `cp` and
other host commands run there; host commands use `/bin/sh -c`, so they can modify
host files just as commands in your regular terminal do. Use explicit `docker`
commands for operations on Docker-Goo's selected engine. Docker commands are
passed directly to the Docker CLI with the selected endpoint/TLS settings;
host-shell pipelines containing Docker use the host shell's own Docker context.

- Left/Right, Home/End, Backspace/Delete: edit at the cursor, including Unicode.
- Ctrl+A / Ctrl+E: start/end of command; Ctrl+U: delete before the cursor.
- Terminal paste works in commands, container forms, image searches and paths.
  Multiline paste becomes one editable line; pasting never executes automatically.
- Ctrl+S: show/hide suggestions. Enter/Tab inserts a visible suggestion; hide
  suggestions before Enter when you want to execute the exact existing line.
- Up/Down: select suggestions, or recall command history when suggestions are hidden.
- Wheel or PgUp/PgDn: scroll output. New commands resume following new output.
- Command output is streamed live and retained between commands with `====`
  separators (bounded to about 1 MiB per session). History holds 200 commands.
- Ctrl+C: cancel the current console process group. Docker daemon work or an
  already-started container exec can continue; inspect its state before retrying.
- Ctrl+O: open your native host shell in the displayed directory; `exit` returns.
  Directory changes in that child shell do not change the embedded console's cwd.
- `docker exec -it NAME bash` and `docker run -it ...` use the current terminal's
  native input/output and return to Docker-Goo afterward. No desktop emulator is
  assumed. In a native shell Ctrl+D is normal EOF; Docker's native attach detach
  sequence is Ctrl+P, Ctrl+Q. In the API `b` shell, Ctrl+D still detaches.
- `b` on a container opens the shell selector. Enter uses the API shell; click
  **Native terminal (-it)** or Ctrl+Enter for the native Docker CLI session.
- `docker compose -f ` suggests local YAML files; `docker build -f ` suggests
  local Dockerfiles; `cd ` suggests directories. YAML is not a docker exec argument.
- Blank input suggests **docker** first. Selected suggestions, images, form fields
  and the Create button use the drawer's blue/purple selection styling.
- Creation failures keep all form fields editable and show the error. A failed
  start may leave a created container: inspect/remove that container explicitly
  before reusing its name. No failed container is silently force-deleted.

Pull/build output is no longer buffered to completion in `$`. Streamed commands
have a 30-minute ceiling. The API new-container form still shows a working status
while pulling, rather than per-layer progress. Use `$ docker pull IMAGE` for that.
A registry error such as `nginx:apline: not found` requires correcting the tag to
`nginx:alpine`; a UI retry cannot fix an invalid registry tag.

## Run-and-return release checks

```bash
python3 tests/release_check.py
```

Python 3 and Docker CLI/Compose are required. The script creates uniquely named
fixtures, runs Docker-engine lifecycle/storage/network/Compose/build checks, and
pauses for manual UI observations in a second terminal. It never runs daemon-wide
prune. It cleans only its own fixtures by default; `--keep` retains them and writes
exact cleanup commands to the JSON report. Base images and build cache are retained.
Send back `docker-goo-report-*.json`. `--auto` skips UI questions and explicitly
marks them untested. CLI checks do not certify that the TUI works.

The earlier HTML guide predates host commands and native `-it` support; its tests
expecting those commands to be rejected are superseded by this section.
