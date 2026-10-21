#!/usr/bin/env python3
"""Measure real Firefox requests through the running desktop app SOCKS router.

Requires a local geckodriver WebDriver session with an isolated SOCKS5 profile.
No fault injection or payments are performed. Raw results remain local.
"""
import argparse
import asyncio
import base64
import collections
import hashlib
import http.server
import json
import math
import pathlib
import statistics
import threading
import time
import urllib.request

import soak

DASHBOARD = r'''<!doctype html><meta charset="utf-8"><title>El Tor Firefox connectivity soak</title>
<style>body{font:18px system-ui;max-width:900px;margin:50px auto;background:#101827;color:#e4edf7}pre{white-space:pre-wrap}h1{color:#78e4bf}</style>
<h1>El Tor · Firefox SOCKS5 test</h1><p>Firefox → app router :18048 → embedded client → Chutney relays → test origin</p>
<pre id="stats">Waiting to start…</pre><script>
window.records=[];window.testState={started:null,deadline:null,active:0,done:false};
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const hex=b=>Array.from(new Uint8Array(b),x=>x.toString(16).padStart(2,'0')).join('');
const hashes=new Map();
async function expected(size){if(!hashes.has(size)){let b=new Uint8Array(size);for(let i=0;i<size;i++)b[i]=i%256;hashes.set(size,hex(await crypto.subtle.digest('SHA-256',b)));}return hashes.get(size);}
async function request(size,delay,kind){
 const id=crypto.randomUUID(),start=Date.now()/1000,t0=performance.now();
 const row={id,kind,start,expected_bytes:size,actual_bytes:0,ok:false};
 const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),delay?90000:20000);
 window.testState.active++;
 try{let r=await fetch(`/bytes?size=${size}&delay_ms=${delay}&id=${id}`,{cache:'no-store',signal:controller.signal});row.http_code=r.status;let b=await r.arrayBuffer();row.actual_bytes=b.byteLength;row.sha256=hex(await crypto.subtle.digest('SHA-256',b));row.ok=r.status===200&&b.byteLength===size&&row.sha256===await expected(size);if(!row.ok)row.error='HTTP, size or hash mismatch';}
 catch(e){row.error=e.name+': '+e.message;}
 finally{clearTimeout(timer);row.time_total=(performance.now()-t0)/1000;row.end=Date.now()/1000;window.records.push(row);window.testState.active--;}
}
window.begin=async seconds=>{
 window.testState.started=Date.now()/1000;window.testState.deadline=window.testState.started+seconds;
 const sizes=[16384,262144,262144,1048576];
 const workers=sizes.map(async(size)=>{while(Date.now()/1000<window.testState.deadline){let t=performance.now();await request(size,0,'repeated');await sleep(Math.max(0,1500-(performance.now()-t)));}});
 workers.push((async()=>{while(Date.now()/1000<window.testState.deadline){let t=performance.now();await request(8388608,250,'paced');await sleep(Math.max(0,60000-(performance.now()-t)));}})());
 await Promise.all(workers);window.testState.done=true;
};
setInterval(()=>{let a=window.records,bytes=a.reduce((s,r)=>s+r.actual_bytes,0),bad=a.filter(r=>!r.ok).length;document.querySelector('#stats').textContent=JSON.stringify({elapsed_s:window.testState.started?Math.floor(Date.now()/1000-window.testState.started):0,requests:a.length,failures:bad,verified_MiB:Math.round(bytes/1048576),inflight:window.testState.active,done:window.testState.done},null,2);},1000);
</script>'''


class Origin(soak.Origin):
    def do_GET(self):
        if self.path.startswith('/bytes?'):
            return super().do_GET()
        data = DASHBOARD.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(data)


class Driver:
    def __init__(self, url, session):
        self.url = url.rstrip('/') + '/session/' + session
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def call(self, method, path, value=None):
        data = json.dumps(value).encode() if value is not None else None
        req = urllib.request.Request(self.url + path, data=data, method=method,
                                     headers={'Content-Type': 'application/json'})
        with self.opener.open(req, timeout=100) as response:
            result = json.load(response)['value']
        if isinstance(result, dict) and 'error' in result:
            raise RuntimeError(str(result))
        return result

    def script(self, script, *args):
        return self.call('POST', '/execute/sync', {'script': script, 'args': list(args)})


def summarize(rows):
    good = [r for r in rows if r.get('ok')]
    times = sorted(r['time_total'] for r in good)
    return {'requests': len(rows), 'failures': len(rows) - len(good),
            'failure_pct': (len(rows) - len(good)) * 100 / len(rows) if rows else None,
            'verified_bytes': sum(r['actual_bytes'] for r in good),
            'median_s': statistics.median(times) if times else None,
            'p95_s': times[max(0, math.ceil(len(times) * .95) - 1)] if times else None}


async def comparison_traffic(out, deadline):
    async def worker(route, number):
        while time.time() < deadline:
            start = time.monotonic()
            await soak.request(out, 'firefox-parallel', route,
                               262144 if number else 1048576)
            await asyncio.sleep(max(0, 1.5 - (time.monotonic() - start)))
    await asyncio.gather(*(worker(route, n) for route in ('direct', 'client', 'router', 'chutney')
                           for n in range(2)))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', type=pathlib.Path, required=True)
    parser.add_argument('--seconds', type=int, default=1200)
    parser.add_argument('--webdriver', default='http://127.0.0.1:4445')
    parser.add_argument('--session', required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    out.chmod(0o700)
    for name in ('logs', 'tmp'):
        (out / name).mkdir(exist_ok=True)
    soak.STOP.clear()
    servers = []
    # A distinct browser origin port makes Firefox traffic identifiable in
    # Tor STREAM events independently of the curl comparison workers.
    for port in (18880, 18881):
        server = http.server.ThreadingHTTPServer(('127.0.0.1', port), Origin)
        server.daemon_threads = True
        server.out = out
        threading.Thread(target=server.serve_forever, daemon=True).start()
        servers.append(server)
    for node in [None, *sorted((soak.ROOT / 'chutney/net/nodes').glob('*'))]:
        if node is not None and not (node / 'control').exists():
            continue
        name = node.name if node else 'app-client'
        threading.Thread(target=soak.events, args=(out, name, node), daemon=True).start()
        sources = [('app-daemon', pathlib.Path.home() / '.local/share/eltor/eltor.log')] if node is None else []
        base = node if node else pathlib.Path.home() / '.local/share/eltor/tor_data/chutney-client'
        sources += [(name + '-' + level, base / (level + '.log')) for level in ('notice', 'info')]
        for label, path in sources:
            if path.exists():
                threading.Thread(target=soak.log_follow, args=(out, path, label + '.log'), daemon=True).start()
    driver = Driver(args.webdriver, args.session)
    driver.call('POST', '/timeouts', {'pageLoad': 20000, 'script': 10000})
    driver.call('POST', '/url', {'url': 'http://127.0.0.1:18881/'})
    assert driver.script('return typeof window.begin') == 'function'
    main_tab = driver.call('GET', '/window')
    external_tab = driver.call('POST', '/window/new', {'type': 'tab'})['handle']
    driver.call('POST', '/window', {'handle': main_tab})
    soak.snapshot(out, 'start')
    driver.script('window.begin(arguments[0]);return true', args.seconds)
    state = driver.script('return window.testState')
    started, deadline = state['started'], state['deadline']
    (out / 'timing.json').write_text(json.dumps({'start': started, 'deadline': deadline,
                                               'requested_seconds': args.seconds}, indent=2))
    comparison = threading.Thread(target=lambda: asyncio.run(comparison_traffic(out, deadline)))
    comparison.start()
    print(json.dumps({'event': 'started', 'start': started, 'deadline': deadline}), flush=True)
    offset, next_external, next_snapshot = 0, started, started
    browser_rows, external_rows = [], []
    try:
        while True:
            result = driver.script('return {state:window.testState,rows:window.records.slice(arguments[0])}', offset)
            for row in result['rows']:
                soak.emit(out / 'firefox-requests.jsonl', row)
                browser_rows.append(row)
            offset += len(result['rows'])
            if time.time() >= next_snapshot:
                soak.snapshot(out, 'firefox-soak')
                soak.emit(out / 'progress.jsonl', {**summarize(browser_rows), 'elapsed_s': time.time() - started,
                                                  'active': result['state']['active']})
                next_snapshot += 10
            if result['state']['done']:
                break
            if time.time() >= next_external and time.time() < deadline:
                record = {'start': time.time(), 'url': 'https://example.com/'}
                try:
                    driver.call('POST', '/window', {'handle': external_tab})
                    driver.call('POST', '/url', {'url': 'https://example.com/?eltor-soak=' + str(int(time.time()))})
                    record['title'] = driver.call('GET', '/title')
                    record['ok'] = record['title'] == 'Example Domain'
                except Exception as e:
                    record.update(ok=False, error=str(e))
                finally:
                    record['time_total'] = time.time() - record['start']
                    record['actual_bytes'] = 0
                    soak.emit(out / 'firefox-pages.jsonl', record)
                    external_rows.append(record)
                    driver.call('POST', '/window', {'handle': main_tab})
                next_external += 60
            if time.time() > deadline + 120:
                raise RuntimeError('Firefox did not finish within the drain allowance')
            time.sleep(5)
        comparison.join(timeout=45)
        if comparison.is_alive():
            raise RuntimeError('Comparison traffic did not finish')
        soak.snapshot(out, 'end')
        png = driver.call('GET', '/screenshot')
        (out / 'firefox-final.png').write_bytes(base64.b64decode(png))
    finally:
        soak.STOP.set()
        for server in servers:
            server.shutdown()
    rows = [json.loads(line) for line in (out / 'requests.jsonl').read_text().splitlines()]
    origin = {json.loads(line)['id'] for line in (out / 'origin.jsonl').read_text().splitlines()}
    groups = collections.defaultdict(list)
    for row in browser_rows:
        groups['firefox/' + row['kind']].append(row)
    for row in rows:
        groups['curl/' + row['route']].append(row)
    report = {'elapsed_s': time.time() - started, 'traffic_window_s': deadline - started,
              'browser': summarize(browser_rows), 'public_pages': summarize(external_rows),
              'groups': {key: summarize(group) for key, group in groups.items()},
              'browser_origin_matches': sum(row['id'] in origin for row in browser_rows),
              'curl_origin_matches': sum(row['id'] in origin for row in rows)}
    (out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
