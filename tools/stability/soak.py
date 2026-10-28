#!/usr/bin/env python3
"""Local-only El Tor transport soak with curl results and synchronized Tor events."""
import argparse, asyncio, concurrent.futures, hashlib, http.server, json, os
import pathlib, socket, subprocess, threading, time, urllib.parse, uuid

ROOT = pathlib.Path(__file__).resolve().parents[3]
BLOCK = bytes(range(256)) * 256
STOP = threading.Event()
LOCK = threading.Lock()

def now():
    return time.time_ns() / 1e9

def emit(path, record):
    with LOCK, path.open('a') as f:
        f.write(json.dumps({'ts': now(), **record}) + '\n')

def control(command, node=None):
    s = socket.socket(socket.AF_UNIX if node else socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(4)
    if node:
        s.connect(str((node / 'control').resolve()))
        auth = (node / 'control_auth_cookie').read_bytes().hex()
    else:
        s.connect(('127.0.0.1', 9992))
        auth = json.dumps(os.environ.get('APP_ELTOR_TOR_CONTROL_PASSWORD', 'password1234_'))
    f = s.makefile('rb')
    s.sendall(f'AUTHENTICATE {auth}\r\n'.encode())
    response = f.readline().decode().strip()
    if response != '250 OK':
        s.close()
        raise RuntimeError('Control authentication failed')
    s.sendall((command + '\r\n').encode())
    return s, f

def read_reply(f):
    lines = []
    data = False
    while True:
        line = f.readline().decode().strip()
        lines.append(line)
        if not line:
            break
        if data:
            if line == '.': data = False
            continue
        if len(line) >= 4 and line[:3].isdigit():
            if line[3] == '+': data = True
            elif line[3] == ' ': break
    return '\n'.join(lines)

def rpc(command):
    s, f = control(command)
    try:
        return read_reply(f)
    finally:
        f.close(); s.close()

def events(out, name, node=None):
    try:
        s, f = control('SETEVENTS CIRC CIRC_MINOR STREAM ORCONN BW STATUS_CLIENT STATUS_GENERAL', node)
        s.settimeout(2)
        # Read directly after authentication file buffer; readline timeout would poison it.
        while not STOP.is_set():
            line = f.readline()
            if not line:
                break
            emit(out / 'control-events.jsonl', {'node': name, 'event': line.decode(errors='replace').strip()})
    except Exception as e:
        emit(out / 'monitor-errors.jsonl', {'node': name, 'error': str(e)})

class Origin(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def do_GET(self):
        q = urllib.parse.parse_qs(urllib.parse.urlsplit(self.path).query)
        size = min(int(q.get('size', ['262144'])[0]), 32 * 1024 * 1024)
        delay = int(q.get('delay_ms', ['0'])[0]) / 1000
        ident = q.get('id', [''])[0]
        start = now()
        sent = 0
        try:
            self.send_response(200)
            self.send_header('Content-Length', str(size))
            self.send_header('Content-Type', 'application/octet-stream')
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            while sent < size:
                n = min(len(BLOCK), size - sent)
                self.wfile.write(BLOCK[:n]); self.wfile.flush(); sent += n
                if delay:
                    time.sleep(delay)
            error = None
        except (OSError, ConnectionError) as e:
            error = repr(e)
        emit(self.server.out / 'origin.jsonl', {'id': ident, 'start': start, 'sent': sent, 'error': error})
    def log_message(self, *args):
        pass

async def request(out, phase, route, size=262144, delay_ms=0, deadline=30):
    ident = uuid.uuid4().hex[:14]
    target = out / 'tmp' / ident
    args = ['curl', '--silent', '--show-error', '--max-time', str(deadline),
            '--noproxy', '', '--output', str(target), '--write-out', '%{json}',
            '--http1.1']
    if route != 'direct':
        args += ['--socks5-hostname', '127.0.0.1:' + {'router':'18048','client':'18058','chutney':'9060'}[route]]
    else:
        args += ['--proxy', '']
    args += [f'http://127.0.0.1:18880/bytes?size={size}&delay_ms={delay_ms}&id={ident}']
    start = now()
    p = await asyncio.create_subprocess_exec(*args, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
    stdout, stderr = await p.communicate()
    try:
        m = json.loads(stdout)
    except Exception:
        m = {}
    content = target.read_bytes() if target.exists() else b''
    target.unlink(missing_ok=True)
    expected = hashlib.sha256((BLOCK * ((size + len(BLOCK)-1)//len(BLOCK)))[:size]).hexdigest()
    good = p.returncode == 0 and m.get('http_code') == 200 and len(content) == size and hashlib.sha256(content).hexdigest() == expected
    record = {'id':ident, 'phase':phase, 'route':route, 'start':start, 'exit':p.returncode,
              'ok':good, 'expected_bytes':size, 'actual_bytes':len(content), 'error':stderr.decode().strip(),
              **{k:m.get(k) for k in ('http_code','time_total','time_starttransfer','time_connect','time_appconnect','speed_download','local_port')}}
    emit(out / 'requests.jsonl', record)
    return record

def snapshot(out, label):
    record = {'phase':label}
    for key,cmd in [('bootstrap','GETINFO status/bootstrap-phase'),('circuits','GETINFO circuit-status'),('streams','GETINFO stream-status'),('conf','GETCONF __LeaveStreamsUnattached ConfluxEnabled ExitNodes')]:
        try: record[key] = rpc(cmd)
        except Exception as e: record[key] = str(e)
    emit(out / 'snapshots.jsonl',record)

async def run(out, seconds):
    routes = ('direct','client','router','chutney')
    snapshot(out,'start')
    # Paired small / bulk serial measurements, then browser-like fan-out.
    for size in (16384, 1048576, 8388608):
        for route in routes:
            for _ in range(4):
                await request(out,'serial',route,size)
    for concurrency in (4,16,48):
        for route in routes:
            await asyncio.gather(*(request(out,f'fanout-{concurrency}',route,262144) for _ in range(concurrency)))
    # A paced 32-second transfer checks persistent streams alongside repeated requests.
    async def long_stream(route):
        return await request(out,'long-stream',route,8*1024*1024,250,deadline=90)
    long_tasks=[asyncio.create_task(long_stream(r)) for r in routes]
    end = time.monotonic() + seconds
    async def worker(route, n):
        while time.monotonic() < end:
            start = time.monotonic()
            await request(out,'soak',route,262144 if n % 2 else 1048576)
            await asyncio.sleep(max(0,1.5 - (time.monotonic()-start)))
    tasks=[asyncio.create_task(worker(r,n)) for r in routes for n in range(2)]
    while time.monotonic() < end:
        snapshot(out,'soak')
        proc = subprocess.run(['ps','-C','tor,eltord,eltor-app','-o','pid,pcpu,pmem,rss,nlwp,comm'],capture_output=True,text=True)
        emit(out/'resources.jsonl', {'processes':proc.stdout, 'loadavg':pathlib.Path('/proc/loadavg').read_text()})
        await asyncio.sleep(min(10,max(0,end-time.monotonic())))
    await asyncio.gather(*tasks,*long_tasks)
    snapshot(out,'end')

def log_follow(out, path, name):
    try:
        with path.open('rb') as f, (out/'logs'/name).open('ab') as dst:
            f.seek(0,2)
            written=0
            while not STOP.wait(.15):
                data=f.read(1024*1024)
                if data:
                    dst.write(data);dst.flush();written+=len(data)
                if written > 128*1024*1024:
                    emit(out/'monitor-errors.jsonl',{'node':name,'error':'128 MiB log capture cap reached'})
                    break
    except Exception as e:
        emit(out/'monitor-errors.jsonl',{'node':name,'error':str(e)})

def main():
    p=argparse.ArgumentParser();p.add_argument('--seconds',type=int,default=480);p.add_argument('--out',required=True)
    args=p.parse_args();out=pathlib.Path(args.out).resolve();out.mkdir(parents=True,exist_ok=True);out.chmod(0o700)
    (out/'logs').mkdir(exist_ok=True);(out/'tmp').mkdir(exist_ok=True)
    server=http.server.ThreadingHTTPServer(('127.0.0.1',18880),Origin);server.daemon_threads=True;server.out=out
    threading.Thread(target=server.serve_forever,daemon=True).start()
    for node in [None,*sorted((ROOT/'chutney/net/nodes').glob('*'))]:
        if node is not None and not (node/'control').exists():continue
        name=node.name if node else 'app-client'
        threading.Thread(target=events,args=(out,name,node),daemon=True).start()
        sources=[('app-daemon',pathlib.Path.home().joinpath('.local/share/eltor/eltor.log'))] if node is None else []
        base=node if node else pathlib.Path.home().joinpath('.local/share/eltor/tor_data/chutney-client')
        sources += [(name+'-'+level,base/(level+'.log')) for level in ('notice','info')]
        for label,path in sources:
            if path.exists():threading.Thread(target=log_follow,args=(out,path,label+'.log'),daemon=True).start()
    print('Starting local traffic matrix and',args.seconds,'second soak. Results:',out,flush=True)
    try:asyncio.run(run(out,args.seconds))
    finally:STOP.set();server.shutdown()
    rows=[json.loads(l) for l in (out/'requests.jsonl').read_text().splitlines()]
    summary={'requests':len(rows),'failures':sum(not r['ok'] for r in rows)}
    (out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary),flush=True)

if __name__=='__main__':main()
