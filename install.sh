#!/bin/sh

# Docker-Goo universal Linux installer.
# Normal installations use precompiled GitHub Release binaries.

set -u

REPO="kurapikanlight/docker-goo"
REPO_URL="https://github.com/$REPO"
RELEASE_BASE="$REPO_URL/releases/latest/download"
BINARY_NAME="docker-goo"
TMP_DIR=""
INSTALL_PATH=""
PACKAGE_MANAGER=""
DOCKER_INSTALLED_BY_US=0

ok()   { printf '✓ %s\n' "$*"; }
info() { printf '→ %s\n' "$*"; }
warn() { printf '! %s\n' "$*" >&2; }
fail() { printf '✗ %s\n' "$*" >&2; exit 1; }

cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
trap cleanup EXIT HUP INT TERM

have() { command -v "$1" >/dev/null 2>&1; }

run_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif have sudo; then
        sudo "$@"
    else
        return 127
    fi
}

fetch() {
    url=$1
    dest=$2

    if have curl; then
        curl -fL --retry 3 --retry-delay 1 --connect-timeout 15 -o "$dest" "$url"
    elif have wget; then
        wget -q --https-only --tries=3 -O "$dest" "$url"
    else
        fail "Neither curl nor wget is available. Install one and run the installer again."
    fi
}

sha256_file() {
    file=$1
    if have sha256sum; then
        sha256sum "$file" | awk '{print $1}'
    elif have shasum; then
        shasum -a 256 "$file" | awk '{print $1}'
    else
        fail "No SHA-256 tool found. Install coreutils (sha256sum) and try again."
    fi
}

detect_package_manager() {
    for pm in apt-get dnf yum pacman zypper apk xbps-install emerge nix; do
        if have "$pm"; then
            PACKAGE_MANAGER=$pm
            return 0
        fi
    done
    return 1
}

install_docker_native() {
    [ -n "$PACKAGE_MANAGER" ] || return 1

    info "Docker CLI/Engine not found; trying the native package environment ($PACKAGE_MANAGER)..."

    case "$PACKAGE_MANAGER" in
        apt-get)
            run_root apt-get update || return 1
            run_root apt-get install -y docker.io || return 1
            ;;
        dnf)
            if ! run_root dnf install -y moby-engine; then
                run_root dnf install -y docker-ce || return 1
            fi
            ;;
        yum)
            if ! run_root yum install -y docker-ce; then
                run_root yum install -y moby-engine || return 1
            fi
            ;;
        pacman)
            run_root pacman -S --needed --noconfirm docker || return 1
            ;;
        zypper)
            run_root zypper --non-interactive install docker || return 1
            ;;
        apk)
            run_root apk add docker || return 1
            ;;
        xbps-install)
            run_root xbps-install -Sy docker || return 1
            ;;
        emerge)
            run_root emerge --noreplace app-containers/docker || return 1
            ;;
        nix)
            warn "Nix was detected. Docker service configuration is declarative on NixOS, so the installer will not modify it automatically."
            return 1
            ;;
        *)
            return 1
            ;;
    esac

    DOCKER_INSTALLED_BY_US=1
    return 0
}

install_compose_native() {
    [ -n "$PACKAGE_MANAGER" ] || return 1

    info "Docker Compose plugin not found; trying the native package environment..."

    case "$PACKAGE_MANAGER" in
        apt-get)
            run_root apt-get update || return 1
            if run_root apt-get install -y docker-compose-v2; then return 0; fi
            if run_root apt-get install -y docker-compose-plugin; then return 0; fi
            run_root apt-get install -y docker-compose
            ;;
        dnf)
            if run_root dnf install -y docker-compose-plugin; then return 0; fi
            run_root dnf install -y docker-compose
            ;;
        yum)
            if run_root yum install -y docker-compose-plugin; then return 0; fi
            run_root yum install -y docker-compose
            ;;
        pacman)
            run_root pacman -S --needed --noconfirm docker-compose
            ;;
        zypper)
            run_root zypper --non-interactive install docker-compose
            ;;
        apk)
            if run_root apk add docker-cli-compose; then return 0; fi
            run_root apk add docker-compose
            ;;
        xbps-install)
            run_root xbps-install -Sy docker-compose
            ;;
        emerge)
            run_root emerge --noreplace app-containers/docker-compose
            ;;
        nix)
            return 1
            ;;
        *)
            return 1
            ;;
    esac
}

start_docker_service_if_needed() {
    [ "$DOCKER_INSTALLED_BY_US" -eq 1 ] || return 0

    if have systemctl && [ -d /run/systemd/system ]; then
        info "Starting Docker with systemd..."
        run_root systemctl enable --now docker || return 1
    elif have rc-service; then
        info "Starting Docker with OpenRC..."
        if have rc-update; then
            run_root rc-update add docker default || return 1
        fi
        run_root rc-service docker start || return 1
    elif have sv; then
        info "Trying to start Docker with runit..."
        run_root sv up docker || return 1
    else
        warn "Docker was installed, but no supported service manager was detected. Start the Docker daemon using your system's service mechanism."
        return 1
    fi
}

install_release_binary() {
    asset=$1
    binary_file="$TMP_DIR/$asset"
    checksum_file="$TMP_DIR/SHA256SUMS"

    info "Downloading Docker-Goo..."
    fetch "$RELEASE_BASE/$asset" "$binary_file" || fail "Could not download $asset from the latest GitHub Release."
    fetch "$RELEASE_BASE/SHA256SUMS" "$checksum_file" || fail "Could not download release checksums."

    expected=$(awk -v name="$asset" '$2 == name || $2 == "*" name {print $1; exit}' "$checksum_file")
    [ -n "$expected" ] || fail "The release checksum file does not contain $asset."

    actual=$(sha256_file "$binary_file")
    [ "$actual" = "$expected" ] || fail "Checksum verification failed for $asset."
    ok "SHA-256 checksum verified"

    chmod 0755 "$binary_file"

    if [ "$(id -u)" -eq 0 ] || [ -w /usr/local/bin ]; then
        target="/usr/local/bin/$BINARY_NAME"
        if have install; then
            install -m 0755 "$binary_file" "$target" || fail "Could not install to $target."
        else
            cp "$binary_file" "$target" && chmod 0755 "$target" || fail "Could not install to $target."
        fi
    elif have sudo && [ -d /usr/local/bin ]; then
        target="/usr/local/bin/$BINARY_NAME"
        if have install; then
            run_root install -m 0755 "$binary_file" "$target" || fail "Could not install to $target."
        else
            run_root cp "$binary_file" "$target" && run_root chmod 0755 "$target" || fail "Could not install to $target."
        fi
    else
        mkdir -p "$HOME/.local/bin" || fail "Could not create $HOME/.local/bin."
        target="$HOME/.local/bin/$BINARY_NAME"
        if have install; then
            install -m 0755 "$binary_file" "$target" || fail "Could not install to $target."
        else
            cp "$binary_file" "$target" && chmod 0755 "$target" || fail "Could not install to $target."
        fi
    fi

    INSTALL_PATH=$target
    ok "Installed Docker-Goo to $INSTALL_PATH"
}

source_fallback() {
    warn "No precompiled Docker-Goo release is published for architecture: $1"

    if have cargo && have rustc && have git; then
        info "Rust and Git are already available; building Docker-Goo from source as a fallback..."
        cargo install --git "$REPO_URL.git" --locked --force docker-goo || fail "Source-build fallback failed."
        cargo_home=${CARGO_HOME:-"$HOME/.cargo"}
        INSTALL_PATH="$cargo_home/bin/$BINARY_NAME"
        ok "Built and installed Docker-Goo from source"
        return 0
    fi

    fail "No release binary exists for this architecture. Install Rust 1.98+, Cargo, Git, and build tools, then use the manual source instructions in INSTALLATION.md."
}

printf '%s\n\n' "Docker-Goo Installer"

[ "$(uname -s 2>/dev/null || printf unknown)" = "Linux" ] || fail "Docker-Goo's installer currently supports Linux only."
ok "Linux detected"

machine=$(uname -m 2>/dev/null || printf unknown)
case "$machine" in
    x86_64|amd64) release_arch="x86_64" ;;
    aarch64|arm64) release_arch="aarch64" ;;
    armv7|armv7l) release_arch="armv7" ;;
    *) release_arch="" ;;
esac
ok "Architecture: $machine"

libc="unknown"
if getconf GNU_LIBC_VERSION >/dev/null 2>&1; then
    libc=$(getconf GNU_LIBC_VERSION 2>/dev/null || printf glibc)
elif have ldd; then
    ldd_first=$(ldd --version 2>&1 | sed -n '1p')
    case "$ldd_first" in
        *musl*|*Musl*) libc="musl" ;;
        *GLIBC*|*glibc*|*GNU*) libc="glibc" ;;
    esac
fi
ok "libc/environment: $libc"

if detect_package_manager; then
    :
fi

if have docker; then
    ok "Docker CLI detected"
else
    if install_docker_native && have docker; then
        ok "Docker installed"
        if ! start_docker_service_if_needed; then
            warn "Docker service could not be started automatically. Docker-Goo will still be installed."
        fi
    else
        warn "Docker is not installed and could not be installed safely with the detected package environment."
        warn "Docker-Goo will still be installed; configure a local or remote Docker Engine before using it."
    fi
fi

if have docker; then
    if docker compose version >/dev/null 2>&1; then
        ok "Docker Compose detected"
    elif install_compose_native >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
        ok "Docker Compose installed"
    else
        warn "Docker Compose plugin was not detected. Core Docker-Goo features can still work, but Compose features will be unavailable."
    fi

    docker_info_output=$(docker info 2>&1)
    docker_info_status=$?
    if [ "$docker_info_status" -eq 0 ]; then
        ok "Docker Engine reachable"
    else
        case "$docker_info_output" in
            *[Pp]ermission*[Dd]enied*|*permission*denied*)
                warn "Docker Engine exists, but the current user cannot access it."
                warn "Do not add yourself to the docker group blindly: that group grants root-level Docker control. Rootless Docker is also supported."
                ;;
            *)
                warn "Docker CLI is present, but the selected Docker Engine is not reachable right now."
                warn "Check the daemon, DOCKER_HOST/DOCKER_CONTEXT, rootless socket, or current Docker context."
                ;;
        esac
    fi
else
    warn "Docker CLI is unavailable; engine and Compose checks were skipped."
fi

TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/docker-goo.XXXXXX") || fail "Could not create a temporary directory."

if [ -n "$release_arch" ]; then
    install_release_binary "docker-goo-linux-$release_arch"
else
    source_fallback "$machine"
fi

if ! "$INSTALL_PATH" --help >/dev/null 2>&1; then
    fail "Docker-Goo was copied to $INSTALL_PATH but could not start. This is an installation/runtime compatibility failure."
fi
ok "Installation verified"

printf '\n'
if "$INSTALL_PATH" --check; then
    ok "Docker-Goo can reach the configured Docker Engine"
else
    warn "Docker-Goo is installed correctly, but its Docker Engine check failed."
    warn "Run 'docker info' and '$INSTALL_PATH --check' to diagnose the selected engine/context/permissions."
fi

printf '\nRun:\n\n'
if command -v "$BINARY_NAME" >/dev/null 2>&1; then
    printf '  docker-goo\n'
else
    printf '  %s\n\n' "$INSTALL_PATH"
    warn "$INSTALL_PATH is not currently in PATH. Add its directory to PATH, then run: docker-goo"
fi
