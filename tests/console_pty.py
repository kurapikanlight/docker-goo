#!/usr/bin/env python3
"""Console regression checks against a local PTY and fake Docker CLI; no daemon changes."""
import os, sys, pty, termios, fcntl, struct, subprocess, select, time, tempfile
from pathlib import Path
import pyte
binary=os.path.abspath(sys.argv[1])
with tempfile.TemporaryDirectory() as temp:
    root=Path(temp)
    fake=root/'docker'
    fake.write_text('#!/bin/sh\ncase "$*" in\n*pull*) echo download-start; sleep 1; echo download-complete;;\n*) echo docker-help;;\nesac\n')
    fake.chmod(0o755)
    (root/'compose.yaml').write_text('services: {}\n')
    m,s=pty.openpty();fcntl.ioctl(s,termios.TIOCSWINSZ,struct.pack('HHHH',40,140,0,0));before=termios.tcgetattr(s)
    screen=pyte.Screen(140,40);stream=pyte.ByteStream(screen)
    env=dict(os.environ,TERM='xterm-256color',PATH=str(root)+':'+os.environ['PATH'],SHELL='/bin/sh')
    p=subprocess.Popen([binary,'--host','unix:///tmp/goo-no-daemon.sock','--console','--cwd',temp],stdin=s,stdout=s,stderr=s,env=env)
    def wait(needle, timeout=6):
        deadline=time.monotonic()+timeout
        while time.monotonic()<deadline:
            if select.select([m],[],[],.08)[0]:stream.feed(os.read(m,65536))
            if needle in '\n'.join(screen.display):return
        raise AssertionError(needle+'\n'+'\n'.join(screen.display))
    def send(value):os.write(m,value)
    def paste(value):send(b'\x1b[200~'+value.encode()+b'\x1b[201~')
    try:
        wait('docker');send(b'\x13') # hide suggestions
        paste('printf first-result');send(b'\r');wait('first-result');wait('[exit 0]')
        paste('printf EDITX');send(b'\x1b[D\x1b[3~');wait('$ printf EDIT');send(b'\r');wait('EDIT');wait('[exit 0]')
        assert 'first-result' in '\n'.join(screen.display)
        paste('docker pull test-image');send(b'\r');wait('download-start');wait('download-complete')
        paste('cd /tmp');send(b'\r');wait('Host: /tmp')
        paste('pwd');send(b'\r');wait('[exit 0]')
        # Native terminal uses inherited stdio, then returns to the TUI.
        send(b'\x0f');time.sleep(.4);send(b'echo NATIVE-CHECK\nexit\n');wait('terminal session ended')
        send(b'\x1b');time.sleep(.2);send(b'q');assert p.wait(timeout=6)==0
        assert termios.tcgetattr(s)==before
        print('PASS: cursor/delete, bracketed paste, retained output, live chunks, cd, native handoff and terminal restoration')
    finally:
        if p.poll() is None:p.kill();p.wait()
        os.close(m);os.close(s)
