#!/usr/bin/env python3
"""Reproducible public specification fixtures; not a runtime protocol implementation."""
import argparse
import hashlib
import json
import struct
from pathlib import Path
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'spec/vectors/service_codec_v1.json'
D = lambda s: s.encode('ascii') + b'\0'
L = lambda b: struct.pack('!I', len(b)) + b
H = lambda b: hashlib.sha256(b).digest()

def blob(label):
    return H(('public synthetic ' + label).encode('ascii'))

def fixture(name, fmt, values, expected, domain=None, command=None, seed_label=None):
    body = struct.pack('!' + fmt, *values)
    assert len(body) == expected, (name, len(body), expected)
    row = {'name': name, 'format_big_endian': fmt,
           'values': [v.hex() if isinstance(v, bytes) else v for v in values],
           'length': expected, 'body_hex': body.hex(), 'sha256': H(body).hex()}
    if domain:
        seed = blob((seed_label or name) + ' signing seed')
        key = Ed25519PrivateKey.from_private_bytes(seed)
        msg = D(domain) + (bytes([command]) if command is not None else b'') + L(body)
        signature = key.sign(msg)
        key.public_key().verify(signature, msg)
        row.update(public_test_seed_hex=seed.hex(),
                   public_key_hex=key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex(),
                   signature_domain=domain, signed_bytes_hex=msg.hex(), signature_hex=signature.hex())
        if command is not None:
            row['command'] = command
            padded = body + signature + bytes(498-len(body)-64)
            assert len(padded) == 498
            row['padded_relay_body_hex'] = padded.hex()
    return row

def generate():
    z = bytes(32)
    refs = [blob(x) for x in ('service','core','quote','circuit','client','operation','issuer','acquisition')]
    refs[4] = Ed25519PrivateKey.from_private_bytes(blob('control_accept_body signing seed')).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    # The identifiers are synthetic codec input, not a valid descriptor or invoice.
    request = fixture('control_accept_body', 'HBB'+'32s'*8+'IQQQQQ32s32sI',
        [1,1,1,*refs,1,100000,20000,1000,2000000100,1,blob('nonce'),blob('reply'),0],
        372, 'eltor/scq/v1/control-request')
    raw = bytes.fromhex(request['body_hex'])
    request['logical_digest_hex'] = H(D('eltor/scq/v1/control-logical')+L(raw[:296])+L(b'')).hex()
    request['delivery_digest_hex'] = H(D('eltor/scq/v1/control-delivery')+L(raw+bytes.fromhex(request['signature_hex']))).hex()
    refusal_payload = struct.pack('!HQ32s', 3, 0, z)
    response = fixture('control_refusal_body', 'HBBB'+'32s'*8+'IQ32sQ32s32sI42s',
        [1,1,5,1,*refs,1,0,bytes.fromhex(request['delivery_digest_hex']),1,
         blob('nonce'),blob('reply'),42,refusal_payload],423,'eltor/scq/v1/control-reply')
    ready = fixture('ready_body','HBB'+'32s'*5+'QQ32s',
        [1,2,0,refs[0],refs[1],refs[3],blob('attempt'),blob('challenge'),0,4,refs[4]],
        212,'eltor/scq/v1/ready-path',239,seed_label='control_accept_body')
    delegation = fixture('signer_delegation','BBHQQQ32s',
        [1,1,1,2,2000000000,2000007200,bytes.fromhex(response['public_key_hex'])],60)
    funding = fixture('funding_policy','HBBBBHHHIHBB'+'Q'*7+'II',
        [1,1,1,0,1,60,60,15,60,15,2,3,3000,200,1000,0,1000,100000,20000,3600,60],84)
    funding['catalog_ref_hex'] = H(D('eltor/scq/v1/catalog-entry')+refs[0]+struct.pack('!H',1)+L(bytes.fromhex(funding['body_hex']))).hex()
    continuation = fixture('extension_core','B32s32sIIH32sIIQQ',
        [1,blob('attempt'),blob('extension'),1,11,10,refs[1],60,3600,100000,20000],131)
    continuation['extension_digest_hex'] = H(D('eltor/scq/v1/continuation')+refs[0]+refs[1]+refs[3]+L(bytes.fromhex(continuation['body_hex']))).hex()
    status = fixture('attempt_status','BH32sIIB'+'Q'*11,
        [6,0,blob('attempt'),1,2,13,20000,0,40000,80000,10000,3000,0,0,2000,4000,1000],132)
    assert 10000 == 3000+0+0+2000+4000+1000
    refund = fixture('refund_state','32s32sBH32sQQQ',
        [blob('attempt'),blob('refund'),4,0,blob('payment hash'),1000,100,20],123)
    frames = []
    for kind, count in [(1,1),(2,2),(3,8),(4,8)]:
        # Not HPKE ciphertext: deterministic byte fixture checks slice/framing only.
        envelope = bytes((i % 251 for i in range(count*434)))
        payloads=[]
        for index in range(count):
            header=struct.pack('!BBBB32s16s12s',1,kind,index,count,z,blob('leg token')[:16],bytes(12))
            frame=header+envelope[index*434:(index+1)*434]
            assert len(header)==64 and len(frame)==498
            payloads.append(frame.hex())
        assert b''.join(bytes.fromhex(x)[64:] for x in payloads)==envelope
        frames.append({'kind':kind,'count':count,'note':'slice codec only; zero routing is appropriate only on reply-to-client legs',
                       'envelope_hex':envelope.hex(),'fragments_hex':payloads})
    return {'profile':'El Tor v1 public synthetic service codec fixtures',
        'limitations':'Serialization, digest and Ed25519 examples only. Not valid native invoices, descriptors, hs-ntor handshakes or HPKE ciphertext. Not runtime interoperability or independent crypto review.',
        'records':[request,response,ready,delegation,funding,continuation,status,refund],
        'framing':frames,
        'deferred_negative_tests':['wrong signature domain/command','nonzero padding','unknown enum',
          'oversize/truncated payload','changed logical retry bytes','wrong circuit READY',
          'duplicate/changed invoice','accounting partition overlap','expired key or deadline']}

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate()
    if args.write:
        DEST.write_text(json.dumps(data,indent=2)+'\n')
    else:
        assert json.loads(DEST.read_text()) == data, 'fixture differs; review schema changes before regeneration'
    print('Verified 8 canonical record fixtures, signature/digest examples and 4 framing examples; no runtime/HPKE validation.')
if __name__=='__main__':main()
