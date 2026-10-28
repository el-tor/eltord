#!/usr/bin/env python3
"""Controlled circuit closure and an 8s exit stall; always resume the relay."""
import asyncio, http.server, json, pathlib, signal, os, threading, time
from soak import Origin, ROOT, STOP, control, emit, events, log_follow, rpc, snapshot
OUT=ROOT/'.runtime/circuit-audit/faults'
OUT.mkdir(parents=True,exist_ok=True);OUT.chmod(0o700);(OUT/'logs').mkdir(exist_ok=True)

async def fetch(label,size=4096,delay=0,deadline=15,direct=False):
    start=time.time()
    args=['curl','--silent','--show-error','--max-time',str(deadline),'--noproxy','','--output','/dev/null','--write-out','%{json}']
    args+=['--proxy',''] if direct else ['--socks5-hostname','127.0.0.1:18048']
    args+=[f'http://127.0.0.1:18882/bytes?size={size}&delay_ms={delay}&id={label}']
    proc=await asyncio.create_subprocess_exec(*args,stdout=asyncio.subprocess.PIPE,stderr=asyncio.subprocess.PIPE)
    out,err=await proc.communicate()
    try:m=json.loads(out)
    except Exception:m={}
    result={'label':label,'start':start,'exit':proc.returncode,'http':m.get('http_code'),
            'total_s':m.get('time_total'),'bytes':m.get('size_download'),'error':err.decode().strip()}
    emit(OUT/'requests.jsonl',result);print(json.dumps(result),flush=True)
    return result

async def main():
    snapshot(OUT,'before-faults')
    download=asyncio.create_task(fetch('circuit-close-active',16*1024*1024,50,60))
    await asyncio.sleep(2)
    rows=rpc('GETINFO stream-status').splitlines()
    active=next((row.split()[2] for row in rows if ' SUCCEEDED ' in row and ':18882' in row),None)
    if active and active!='0':
        emit(OUT/'faults.jsonl',{'kind':'close-circuit','circuit':active,'response':rpc('CLOSECIRCUIT '+active)})
    else:
        emit(OUT/'faults.jsonl',{'kind':'close-circuit','error':'No matching active stream'})
    await download
    for i in range(5):
        await fetch('after-circuit-close-'+str(i));await asyncio.sleep(1)
    snapshot(OUT,'after-circuit-close')
    exit_node=ROOT/'chutney/net/nodes/004r';pid=int((exit_node/'pid').read_text())
    download=asyncio.create_task(fetch('exit-stall-active',16*1024*1024,50,60))
    await asyncio.sleep(2)
    emit(OUT/'faults.jsonl',{'kind':'exit-pause-start','pid':pid})
    os.kill(pid,signal.SIGSTOP)
    try:
        # Direct succeeds while Tor traffic is stalled: isolate the network fault.
        await fetch('origin-during-exit-stall',direct=True)
        for i in range(3):await fetch('during-exit-stall-'+str(i),deadline=2)
        await asyncio.sleep(2)
    finally:
        os.kill(pid,signal.SIGCONT)
        emit(OUT/'faults.jsonl',{'kind':'exit-pause-end','pid':pid})
    await download
    for i in range(5):
        await fetch('after-exit-resume-'+str(i));await asyncio.sleep(1)
    snapshot(OUT,'after-faults')

server=http.server.ThreadingHTTPServer(('127.0.0.1',18882),Origin);server.daemon_threads=True;server.out=OUT
threading.Thread(target=server.serve_forever,daemon=True).start()
for node in [None,*sorted((ROOT/'chutney/net/nodes').glob('*'))]:
    if node is not None and not (node/'control').exists():continue
    name=node.name if node else 'app-client'
    threading.Thread(target=events,args=(OUT,name,node),daemon=True).start()
    base=node if node else pathlib.Path.home().joinpath('.local/share/eltor/tor_data/chutney-client')
    sources=[(name+'-'+level,base/(level+'.log')) for level in ('notice','info')]
    if node is None:sources += [('app-daemon',pathlib.Path.home().joinpath('.local/share/eltor/eltor.log'))]
    for label,path in sources:
        if path.exists():threading.Thread(target=log_follow,args=(OUT,path,label+'.log'),daemon=True).start()
try:asyncio.run(main())
finally:STOP.set();server.shutdown()
