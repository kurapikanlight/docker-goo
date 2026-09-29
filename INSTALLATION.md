# Docker-Goo installation

Docker-Goo is distributed as a precompiled Linux executable. Normal users do **not** need Rust, Cargo, Git, GCC, Make, or the source repository.

## Quick install

With `curl`:

```bash
curl -fsSL https://raw.githubusercontent.com/kurapikanlight/docker-goo/master/install.sh | sh
```

Without `curl`, use `wget`:

```bash
wget -qO- https://raw.githubusercontent.com/kurapikanlight/docker-goo/master/install.sh | sh
```

Then run:

```bash
docker-goo
```

> The `curl` command naturally requires `curl` to already exist so it can download the installer. The `wget` form is provided for systems where only `wget` is available.

## What the installer does

```text
install.sh
   │
   ├── verify Linux
   ├── detect CPU architecture
   ├── identify libc/environment for diagnostics
   ├── detect Docker CLI / Engine / Compose
   │
   ├── Docker works
   │      └── leave the existing installation untouched
   │
   └── Docker missing
          └── detect an available package environment
                 └── install/configure Docker only when it is safe to do so
   │
   ├── select the matching Docker-Goo release asset
   ├── download the executable + SHA256SUMS
   ├── verify the SHA-256 checksum
   ├── install the executable in PATH
   └── run docker-goo --check
```

The script is intentionally conservative: it installs Docker-Goo without deleting Docker data, pruning resources, replacing an already-working Docker installation, or forcing a Docker context.

## Release binaries

Tagged releases publish these Linux assets:

| Architecture | Release asset | Rust target |
| --- | --- | --- |
| x86_64 / amd64 | `docker-goo-linux-x86_64` | `x86_64-unknown-linux-musl` |
| aarch64 / arm64 | `docker-goo-linux-aarch64` | `aarch64-unknown-linux-musl` |
| armv7 / armv7l | `docker-goo-linux-armv7` | `armv7-unknown-linux-musleabihf` |

They are built against musl to reduce distribution-specific libc requirements. This also allows the same release family to cover glibc distributions and Alpine-style musl environments.

Every release also contains `SHA256SUMS`. The installer refuses to install a downloaded binary if its checksum does not match the published checksum.

### Unsupported CPU architectures

An unknown distribution is **not** treated as an unsupported system merely because its name is unfamiliar.

If the CPU architecture has no published binary, the installer uses a source-build fallback only when Rust, Cargo, Git, and the required build environment already exist. Otherwise it stops with manual build instructions rather than silently installing a toolchain.

## Docker detection

The installer checks more than the presence of `/usr/bin/docker`.

It distinguishes between:

- Docker CLI missing
- Docker CLI present
- Docker Engine reachable
- Docker Engine unreachable
- likely socket permission failure
- Docker Compose plugin available or missing

Existing Docker configuration is left alone. Docker-Goo already understands standard sockets, rootless Docker, `DOCKER_HOST`, `DOCKER_CONTEXT`, Docker contexts, and explicit host/context options.

Useful diagnostics:

```bash
docker version
docker info
docker compose version
docker context show
docker-goo --check
```

## Package-manager handling

When Docker is genuinely missing, the installer can recognize these package environments:

- `apt-get`
- `dnf`
- `yum`
- `pacman`
- `zypper`
- `apk`
- `xbps-install`
- `emerge`
- `nix`

This is capability-based rather than a hard distribution whitelist. For example, an unknown Linux distribution with a working Docker Engine can install Docker-Goo normally.

Package availability still depends on the repositories configured on the machine. If the native package manager cannot provide Docker safely, the installer reports that and continues with Docker-Goo installation rather than rewriting repositories or Docker configuration.

### Immutable and declarative systems

On NixOS and other declarative/immutable environments, enabling a Docker daemon is normally a system configuration decision. The installer does not rewrite the host configuration automatically.

If Docker is already configured and reachable, Docker-Goo works normally.

## Service managers

The installer does not assume systemd.

When it installs Docker itself, it can recognize:

- systemd
- OpenRC
- runit

It only attempts to start Docker after the installer itself installed the engine. If Docker was already installed, the script does not restart or reconfigure its service.

## Docker permissions

The installer does **not** automatically run:

```bash
sudo usermod -aG docker "$USER"
```

Membership in the `docker` group effectively grants root-level control through the Docker daemon. Choose that model deliberately, or use Docker rootless mode when appropriate.

If you intentionally use the Docker group, a new login session may be required before the membership is active.

A permission problem is reported as a Docker Engine access problem, not as a broken Docker-Goo installation.

## Docker contexts and rootless Docker

Docker-Goo's connection selection remains application-controlled. The installer does not force `default`, replace `DOCKER_HOST`, or rewrite rootless settings.

Examples:

```bash
docker-goo --context my-context
docker-goo --host unix:///var/run/docker.sock
docker-goo --host "unix:///run/user/$(id -u)/docker.sock"
```

## Installation destination

The installer prefers:

```text
/usr/local/bin/docker-goo
```

If a system-wide install is unavailable, it falls back to:

```text
~/.local/bin/docker-goo
```

The script does not silently edit `.bashrc`, `.zshrc`, or another shell configuration file. If `~/.local/bin` is not currently in `PATH`, it tells you explicitly.

## Updating

For a binary installation, rerun the installer:

```bash
curl -fsSL https://raw.githubusercontent.com/kurapikanlight/docker-goo/master/install.sh | sh
```

This replaces only the Docker-Goo executable. It does **not** remove or reset:

- Docker Engine
- containers
- images
- volumes
- networks
- contexts
- Compose files

### Source-build update

If you installed from a cloned repository:

```bash
git pull --ff-only
cargo install --path . --locked --force
```

## Uninstalling

### Binary installation

Remove only the Docker-Goo executable from the location used during installation:

```bash
sudo rm -f /usr/local/bin/docker-goo
```

or, for a user-local install:

```bash
rm -f ~/.local/bin/docker-goo
```

### Cargo installation

```bash
cargo uninstall docker-goo
```

Uninstalling Docker-Goo never requires uninstalling Docker.

## Manual installation / Build from source

The current project requires Rust **1.98 or newer**.

### 1. Docker Engine and Compose

Install Docker Engine using your distribution's normal mechanism and verify:

```bash
docker info
docker compose version
```

### 2. Build tools

**Ubuntu / Debian / Linux Mint**

```bash
sudo apt update
sudo apt install -y build-essential curl git
```

**Fedora**

```bash
sudo dnf install gcc make curl git
```

**Arch Linux**

```bash
sudo pacman -S --needed base-devel curl git
```

**openSUSE**

```bash
sudo zypper install gcc make curl git
```

**NixOS**

```bash
nix-shell -p rustup gcc git pkg-config
rustup default stable
```

### 3. Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then verify:

```bash
rustc --version
cargo --version
```

### 4. Build and install Docker-Goo

```bash
git clone https://github.com/kurapikanlight/docker-goo.git
cd docker-goo
cargo install --path . --locked
```

Cargo installs to `~/.cargo/bin`. If needed, load the Rust environment for the current Bash/Zsh session:

```bash
source "$HOME/.cargo/env"
```

## Troubleshooting

| Problem | What to check |
| --- | --- |
| `docker-goo: command not found` | Check `/usr/local/bin` or `~/.local/bin` and your `PATH` |
| Checksum failure | Do not bypass it; retry and verify the GitHub Release assets |
| No binary for the CPU | Use the source-build fallback with Rust 1.98+ |
| Docker Engine unreachable | Run `docker info`, inspect daemon/context/rootless settings, then `docker-goo --check` |
| Docker permission denied | Fix Docker socket access deliberately; do not assume Docker-Goo itself is broken |
| Compose unavailable | Run `docker compose version` and install the Compose plugin for your environment |
| Docker service will not start | Use the service mechanism appropriate to the host |
| Declarative/immutable host | Configure Docker through the host's normal system configuration |

## Installer safety guarantees

The installer does not automatically:

- prune Docker resources
- delete containers, images, volumes, or networks
- overwrite Docker daemon configuration
- reinstall a working Docker installation
- force systemd
- force a Docker context
- replace rootless Docker settings
- require Docker Desktop
- require an account
- add telemetry or cloud dependencies
- silently modify shell startup files
- add the user to the `docker` group

The temporary installer directory is removed on normal exit, interruption, or termination.
