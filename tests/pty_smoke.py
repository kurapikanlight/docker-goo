#!/usr/bin/env python3
"""Check real terminal entry/navigation/exit, without a Docker daemon.
Requires the optional test dependency: pip install pyte
Usage: python3 tests/pty_smoke.py target/debug/docker-goo
"""
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time
import pyte

screen = pyte.Screen(110, 30)
parser = pyte.ByteStream(screen)
binary = os.path.abspath(sys.argv[1])
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 110, 0, 0))
before = termios.tcgetattr(slave)
env = dict(os.environ, TERM="xterm-256color")
process = subprocess.Popen([binary, "--host", "unix:///tmp/docker-goo-test-missing.sock"], stdin=slave, stdout=slave, stderr=slave, env=env)

def read_until(needle, timeout=5):
    data = b""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            chunk = os.read(master, 65536)
            data += chunk
            parser.feed(chunk)
            found = needle in data if needle.startswith(b"\x1b") else needle.decode() in "\n".join(screen.display)
            if found:
                return data
    raise AssertionError(f"Missing {needle!r}; received {data[-300:]!r}")

try:
    entry = read_until(b"DOCKER-GOO")
    assert b"\x1b[?1049h" in entry, "Must enter alternate screen"
    assert b"\x1b[2J" in entry, "Must clear the full screen"
    os.write(master,b"T")
    read_until(b"Classic / Royal accents")
    os.write(master,b"3")
    read_until(b"Daylight / White")
    os.write(master,b"\x1b")
    time.sleep(0.1)
    os.write(master, b"i")
    read_until(b"/images")
    os.write(master, b"h")
    read_until(b"YOUR ENGINE")
    os.write(master, b"c")
    read_until(b"/containers")
    os.write(master, b"a")
    read_until(b"kurapikanlight")
    os.write(master, b"\x1b")
    time.sleep(0.1)
    os.write(master, b"n")
    read_until(b"Choose image")
    os.write(master,b"\x0e")
    read_until(b"New container")
    os.write(master, b"\x10")
    read_until(b"Shell / TTY")
    os.write(master, b"\x1b")
    time.sleep(0.1)
    os.write(master, b"$")
    read_until(b"Host:")
    os.write(master,b"docker pu")
    read_until(b"docker pull")
    os.write(master,b"\r")
    read_until(b"$ docker pull")
    os.write(master,b"ubuntu:24.04")
    read_until(b"$ docker pull ubuntu:24.04")
    os.write(master, b"\x1b")
    time.sleep(0.1)
    os.write(master,b"o")
    read_until(b"/compose")
    os.write(master,b"y")
    read_until(b"Run existing YAML")
    os.write(master,b"/tmp/no-such-compose.yaml\r")
    read_until(b"Choose an existing Compose YAML file")
    os.write(master,b"\x1b")
    time.sleep(0.1)
    os.write(master, b"q")
    read_until(b"\x1b[?1049l")
    assert process.wait(timeout=5) == 0
    assert termios.tcgetattr(slave) == before, "Terminal mode must be restored"
    print("PASS: full-screen entry, home, drawer, sections, quit and terminal restoration")
finally:
    if process.poll() is None:
        process.kill()
        process.wait()
    os.close(master)
    os.close(slave)
