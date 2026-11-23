"""Local-only llama.cpp /infill transport. No remote URLs are accepted."""
from __future__ import annotations
import json
import os
from urllib.parse import urlsplit
from urllib.request import Request, build_opener, ProxyHandler
from .protocol import CompletionRequest

class ModelUnavailable(RuntimeError):
    pass

class LocalInfillBackend:
    def __init__(self, endpoint: str='http://127.0.0.1:8137/infill', timeout: float=0.6):
        parsed = urlsplit(endpoint)
        if parsed.scheme != 'http' or parsed.hostname not in ('127.0.0.1','localhost','::1') or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path != '/infill':
            raise ValueError('Only local HTTP /infill endpoints are allowed')
        if not parsed.port or parsed.port < 1024: raise ValueError('Use a nonprivileged loopback port')
        self.endpoint = endpoint
        # Never honor http_proxy/HTTPS_PROXY for the local model connection.
        self.opener = build_opener(ProxyHandler({}))
        self.timeout = min(max(timeout,0.05),2.0)

    def complete(self, req: CompletionRequest) -> str:
        body = {
            'input_prefix': req.prefix[-12000:],
            'input_suffix': req.suffix[:6000],
            'input_extra': [{'filename': f'local-{i}.txt', 'text': s} for i,s in enumerate(req.context)],
            'n_predict': req.max_tokens,
            'temperature': 0.1,
            'cache_prompt': True,
            'stream': False,
        }
        payload=json.dumps(body).encode('utf-8')
        try:
            with self.opener.open(Request(self.endpoint, data=payload,
                                 headers={'Content-Type':'application/json'}, method='POST'),
                         timeout=self.timeout) as response:
                if response.status != 200: raise ModelUnavailable(f'local model returned {response.status}')
                raw=response.read(64*1024 + 1)
            if len(raw)>64*1024: raise ModelUnavailable('oversized model response')
            obj=json.loads(raw)
            if not isinstance(obj,dict) or not isinstance(obj.get('content'),str):
                raise ModelUnavailable('invalid local model response')
            return obj['content']
        except (TimeoutError, OSError, ValueError, KeyError) as e:
            raise ModelUnavailable(str(e)) from e
