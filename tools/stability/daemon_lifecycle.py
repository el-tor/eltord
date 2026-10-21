#!/usr/bin/env python3
"""Check that a network-disabled embedded Tor shutdown also stops eltord.

The disposable client has no wallet configuration and never contacts relays.
Use --allow-survivor only to record the previous behavior before applying a fix.
"""
import argparse
import json
import os
import pathlib
import signal
import socket
import subprocess
import time

from soak import read_reply


def control(port, cookie, command):
    with socket.create_connection(('127.0.0.1', port), timeout=2) as conn:
        reader = conn.makefile('rb')
        conn.sendall(('AUTHENTICATE ' + cookie.read_bytes().hex() + '\r\n').encode())
        assert reader.readline().strip() == b'250 OK'
        conn.sendall((command + '\r\n').encode())
        return read_reply(reader)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=pathlib.Path)
    parser.add_argument('--out', type=pathlib.Path, required=True)
    parser.add_argument('--allow-survivor', action='store_true')
    args = parser.parse_args()
    binary = args.binary.resolve()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o700)
    results = []
    for name, delay, method in [('startup-sigterm', 0, 'signal'),
                                 ('active-sigterm', 6, 'signal'),
                                 ('active-control-shutdown', 6, 'control')]:
        case = out / name
        case.mkdir()
        with socket.socket() as reservation:
            reservation.bind(('127.0.0.1', 0))
            port = reservation.getsockname()[1]
        cookie = case / 'data/control_auth_cookie'
        config = case / 'torrc'
        config.write_text(f'DataDirectory {case / "data"}\nSocksPort 0\n'
                          f'ControlPort 127.0.0.1:{port}\nCookieAuthentication 1\n'
                          'DisableNetwork 1\nClientOnly 1\nAddress 127.0.0.1\n')
        environment = dict(os.environ)
        environment.pop('ARGS', None)
        with (case / 'daemon.log').open('wb') as log:
            process = subprocess.Popen([str(binary), 'client', '-f', str(config)],
                                       cwd=case, env=environment, stdout=log,
                                       stderr=subprocess.STDOUT)
            try:
                for _ in range(200):
                    if process.poll() is not None:
                        raise RuntimeError('Daemon exited before the control socket opened')
                    try:
                        if control(port, cookie, 'GETINFO version').startswith('250'):
                            break
                    except (OSError, AssertionError):
                        pass
                    time.sleep(.05)
                else:
                    raise RuntimeError('Control socket did not become ready')
                time.sleep(delay)
                started = time.monotonic()
                if method == 'signal':
                    process.send_signal(signal.SIGTERM)
                else:
                    assert control(port, cookie, 'SIGNAL SHUTDOWN') == '250 OK'
                try:
                    exit_code = process.wait(timeout=3)
                    stopped = True
                except subprocess.TimeoutExpired:
                    exit_code, stopped = None, False
                record = {'case': name, 'process_exited': stopped, 'exit_code': exit_code,
                          'shutdown_elapsed_s': time.monotonic() - started}
                results.append(record)
                print(json.dumps(record), flush=True)
            finally:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
    (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    if not args.allow_survivor and not all(r['process_exited'] and r['exit_code'] == 0 for r in results):
        raise SystemExit('Embedded Tor shutdown left eltord running or exited unsuccessfully')


if __name__ == '__main__':
    main()
