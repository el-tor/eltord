#!/usr/bin/env python3
"""Exercise C payment-control error paths in disposable, network-disabled processes."""
import json, pathlib, signal, socket, subprocess, time, tempfile, shutil
ROOT=pathlib.Path(__file__).resolve().parents[3]
OUT=ROOT/'.runtime/circuit-audit/c-control'/('run-'+str(time.time_ns()))
OUT.mkdir(parents=True,exist_ok=True)

def run(kind,embedded=False):
    name=('embedded-' if embedded else 'standalone-')+kind
    artifact=OUT/name;artifact.mkdir(exist_ok=True)
    base=pathlib.Path(tempfile.mkdtemp(prefix='eltor-c-probe-'));base.chmod(0o700)
    cfg=base/'torrc';ctrl=base/'control'
    cfg.write_text(f'DataDirectory {base}/data\nSocksPort 0\nControlSocket {ctrl}\n'
                   'CookieAuthentication 1\nDisableNetwork 1\nRunAsDaemon 0\n'
                   'Log notice stdout\n')
    binary=ROOT/('eltor-app/backend/bin/eltord' if embedded else 'tor/src/app/tor')
    args=[str(binary),*(['client'] if embedded else []),'-f',str(cfg)]
    result={'name':name,'start':time.time()}
    with (base/'process.log').open('wb') as log:
        p=subprocess.Popen(args,stdout=log,stderr=subprocess.STDOUT,cwd=base)
        try:
            for _ in range(100):
                if ctrl.exists():break
                if p.poll() is not None:raise RuntimeError('Process exited before control socket')
                time.sleep(.05)
            cookie=(base/'data/control_auth_cookie').read_bytes().hex()
            with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as s:
                s.settimeout(3);s.connect(str(ctrl));f=s.makefile('rb')
                s.sendall(f'AUTHENTICATE {cookie}\r\n'.encode());result['auth']=f.readline().decode().strip()
                command={'empty-body':b'+EXTENDPAIDCIRCUIT 0\r\n.\r\n',
                         'unknown-circuit':b'+EXTENDPAIDCIRCUIT 4294967295\r\ninvalid\r\n.\r\n',
                         'invalid-node':b'+EXTENDPAIDCIRCUIT 0\r\ninvalid\r\n.\r\n'}[kind]
                s.sendall(command);result['reply']=f.readline().decode(errors='replace').strip()
                s.sendall(b'GETINFO version\r\n');result['alive_reply']=f.readline().decode(errors='replace').strip()
                time.sleep(.15)
                result['process_exit_after_probe']=p.poll()
        except Exception as e:
            time.sleep(.15);result['error']=type(e).__name__+': '+str(e);result['process_exit_after_probe']=p.poll()
        finally:
            if p.poll() is None:
                p.send_signal(signal.SIGTERM)
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:p.kill();p.wait()
    shutil.copy2(base/'process.log', artifact/'process.log')
    shutil.rmtree(base)
    return result

results=[]
for embedded in (False,True):
    for kind in ('empty-body','unknown-circuit','invalid-node'):
        r=run(kind,embedded);results.append(r);print(json.dumps(r),flush=True)
(OUT/'results.json').write_text(json.dumps(results,indent=2)+'\n')
