#!/usr/bin/env python3
"""Reproduce SOCKS framing failures against the running router and direct client."""
import json, pathlib, socket, struct, time

OUT=pathlib.Path(__file__).resolve().parents[3]/'.runtime/circuit-audit/protocol-probes.jsonl'

def exact(s,n):
    result=b''
    while len(result)<n:
        part=s.recv(n-len(result))
        if not part:raise EOFError(f'EOF after {len(result)}/{n} bytes')
        result+=part
    return result

def probe(port,kind):
    started=time.time()
    result={'start':started,'port':port,'case':kind}
    try:
        with socket.create_connection(('127.0.0.1',port),timeout=4) as s:
            s.settimeout(4)
            greeting=b'\x05\x01\x00'
            if kind=='split-greeting':
                s.sendall(greeting[:1]);time.sleep(.15);s.sendall(greeting[1:])
            else:s.sendall(greeting)
            assert exact(s,2)==b'\x05\x00'
            if kind=='short-valid-domain':
                # RFC1928 allows a one-byte hostname; a.b is three bytes.
                domain=b'a.b'
                request=b'\x05\x01\x00\x03'+bytes([len(domain)])+domain+struct.pack('!H',18880)
            else:
                request=b'\x05\x01\x00\x01'+socket.inet_aton('127.0.0.1')+struct.pack('!H',18880)
            if kind=='split-request':
                s.sendall(request[:4]);time.sleep(.15);s.sendall(request[4:])
            elif kind=='bytewise-request':
                for b in request:s.sendall(bytes([b]));time.sleep(.02)
            else:s.sendall(request)
            header=exact(s,4)
            result['reply_status']=header[1]
            assert header[0]==5
            if header[1]!=0:
                result['ok']=False
                result['error']='SOCKS server returned status '+str(header[1])
                return result
            atyp=header[3]
            exact(s,{1:4,4:16}.get(atyp,0) if atyp!=3 else exact(s,1)[0]);exact(s,2)
            s.sendall(b'GET /bytes?size=1024&id=protocol-probe HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n')
            data=b''
            while True:
                part=s.recv(4096)
                if not part:break
                data+=part
            result['ok']=b'200 OK' in data and len(data.split(b'\r\n\r\n',1)[-1])==1024
    except Exception as e:
        result['ok']=False;result['error']=type(e).__name__+': '+str(e)
    finally:result['elapsed']=time.time()-started
    return result

OUT.parent.mkdir(parents=True,exist_ok=True)
with OUT.open('w') as f:
    for case in ('complete','split-greeting','split-request','bytewise-request'):
        for port in (18048,18058,9060):
            for repeat in range(3):
                result=probe(port,case);result['repeat']=repeat
                f.write(json.dumps(result)+'\n');f.flush();print(json.dumps(result),flush=True)
