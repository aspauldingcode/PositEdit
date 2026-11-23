"""JSON-lines worker. Designed for a supervised non-UI child process."""
from __future__ import annotations
import argparse, json, sys
from .protocol import CompletionRequest, ProtocolError, MAX_BYTES
from .engine import CompletionEngine
from .backend import LocalInfillBackend

def main(argv=None) -> int:
    parser=argparse.ArgumentParser(description='Local-only code Tab completion worker')
    parser.add_argument('--local-llama', action='store_true', help='Enable local llama.cpp /infill backend')
    parser.add_argument('--endpoint',default='http://127.0.0.1:8137/infill')
    args=parser.parse_args(argv)
    backend=LocalInfillBackend(args.endpoint) if args.local_llama else None
    engine=CompletionEngine(backend=backend)
    for raw in sys.stdin.buffer:
        if len(raw)>MAX_BYTES:
            print(json.dumps({'protocol':1,'status':'error','error':'oversized request'}),flush=True)
            continue
        try:
            data=json.loads(raw)
            if not isinstance(data,dict): raise ProtocolError('expected object')
            op=data.get('op')
            if op=='complete':
                req=CompletionRequest.from_dict(data)
                reply=engine.complete(req)
            elif op=='set_enabled':
                if type(data.get('enabled')) is not bool: raise ProtocolError('enabled must be boolean')
                engine.enabled=data['enabled']
                reply={'protocol':1,'status':'ok','enabled':engine.enabled}
            elif op=='cancel':
                if not isinstance(data.get('document_id'),str) or type(data.get('revision')) is not int or data['revision']<0:
                    raise ProtocolError('invalid cancel fields')
                engine.set_revision(data['document_id'],data['revision'])
                reply={'protocol':1,'status':'ok'}
            elif op=='health':
                reply={'protocol':1,'status':'ok','enabled':engine.enabled,'backend':'llama.cpp-local' if backend else 'lexical-only','network_policy':'loopback-only' if backend else 'no-network'}
            else: raise ProtocolError('unsupported operation')
        except (ProtocolError, json.JSONDecodeError, TypeError, ValueError) as e:
            reply={'protocol':1,'status':'error','error':str(e)[:512]}
        print(json.dumps(reply,ensure_ascii=False),flush=True)
    return 0

if __name__=='__main__': raise SystemExit(main())
