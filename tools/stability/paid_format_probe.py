#!/usr/bin/env python3
"""Exercise paid-format circuits with zero-fee Chutney relays and no payments."""
import http.server, json, pathlib, re, signal, socket, subprocess, threading, time
from soak import Origin, ROOT, read_reply

OUT = ROOT / '.runtime/circuit-audit/paid-format'
OUT.mkdir(parents=True, exist_ok=True)
OUT.chmod(0o700)

class Wallet(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path not in ('/getinfo', '/getbalance'):
            self.send_error(403)
            return
        value = ({'nodeId': '02' + '11' * 32, 'channels': []} if self.path == '/getinfo'
                 else {'balanceSat': 0, 'feeCreditSat': 0})
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def do_POST(self):
        self.send_error(403, 'Payments are disabled in this test')
    def log_message(self, *args):
        pass

def control(command):
    with socket.create_connection(('127.0.0.1', 9996), timeout=4) as s:
        f = s.makefile('rb')
        s.sendall(b'AUTHENTICATE "password1234_"\r\n')
        assert f.readline().strip() == b'250 OK'
        s.sendall((command + '\r\n').encode())
        return read_reply(f)

wallet_server = http.server.ThreadingHTTPServer(('127.0.0.1', 19741), Wallet)
origin = http.server.ThreadingHTTPServer(('127.0.0.1', 18884), Origin)
origin.out = OUT
origin.daemon_threads = True
for server in (wallet_server, origin):
    threading.Thread(target=server.serve_forever, daemon=True).start()
text = pathlib.Path.home().joinpath('.local/share/eltor/torrc').read_text()
# Never inherit a real wallet from the app configuration.
text = re.sub(r'^\s*PaymentLightningNodeConfig\b.*$', '', text, flags=re.M)
text = re.sub(r'^DataDirectory .*$', f'DataDirectory {OUT}/data', text, flags=re.M)
text = re.sub(r'^SocksPort .*$', 'SocksPort 127.0.0.1:18068', text, flags=re.M)
text = re.sub(r'^ControlPort .*$', 'ControlPort 127.0.0.1:9996', text, flags=re.M)
text = re.sub(r'^Log (notice|info|debug) file .*$',
              lambda m: f'Log {m[1]} file {OUT}/{m[1]}.log', text, flags=re.M)
fingerprint = (ROOT/'chutney/net/nodes/004r/fingerprint').read_text().split()[-1]
text = re.sub(r'^ExitNodes .*$', 'ExitNodes ' + fingerprint, text, flags=re.M)
text = re.sub(r'^PaymentCircuitMaxFee .*$', '', text, flags=re.M)
text += ('\nPaymentCircuitMaxFee 1\nPaymentLightningNodeConfig type=phoenixd '
         'url=http://127.0.0.1:19741 password=test-only default=true\n')
(OUT/'torrc').write_text(text)
(OUT/'torrc').chmod(0o600)
with (OUT/'daemon.log').open('wb') as log:
    process = subprocess.Popen([str(ROOT/'eltor-app/backend/bin/eltord'), 'client',
                                '-f', str(OUT/'torrc'), '-p', 'password1234_'],
                               cwd=OUT, stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(180):
            time.sleep(1)
            if process.poll() is not None:
                raise RuntimeError('Daemon exited: ' + str(process.returncode))
            content=(OUT/'daemon.log').read_text(errors='replace')
            if 'Bootstrapping 100%' in content and 'Manual stream attachment enabled' in content:
                break
        else:
            raise RuntimeError('Paid-format circuits did not become usable within 180 seconds')
        requests=[]
        closed=None
        started=time.monotonic()
        while time.monotonic()-started < 100:
            # Lose one circuit while the owned monitor remains active.
            if closed is None and time.monotonic()-started > 25:
                rows=control('GETINFO circuit-status').splitlines()
                ids=re.findall(r'Created paid Circuit with ID: (\d+)', (OUT/'daemon.log').read_text())
                target=next((r.split()[0] for r in rows if ' BUILT ' in r and r.split()[0] in ids),None)
                if target:
                    closed={'ts':time.time(),'circuit':target,'reply':control('CLOSECIRCUIT '+target)}
            result=subprocess.run(['curl','--silent','--show-error','--max-time','15',
                                   '--socks5-hostname','127.0.0.1:18068',
                                   'http://127.0.0.1:18884/bytes?size=65536&id=paid-format',
                                   '-o','/dev/null','-w','%{http_code}'],capture_output=True,text=True)
            requests.append({'ts':time.time(),'exit':result.returncode,
                             'http':result.stdout,'error':result.stderr})
            time.sleep(2)
        report={'paid_format_circuits_built':True,'payments_permitted':False,
                'elapsed_s':time.monotonic()-started,'requests':requests,'closed':closed,
                'control_end':control('GETCONF __LeaveStreamsUnattached'),
                'monitor_starts':(OUT/'daemon.log').read_text().count('Manual stream attachment enabled')}
        (OUT/'result.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report),flush=True)
        assert all(r['exit']==0 and r['http']=='200' for r in requests)
        assert closed and closed['reply']=='250 OK'
        assert report['monitor_starts']==1
    finally:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        wallet_server.shutdown()
        origin.shutdown()
