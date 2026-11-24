"""No-network-by-default completion controller with revision and response guards."""
from __future__ import annotations
import time
from collections import OrderedDict
from .protocol import CompletionRequest, MAX_OUTPUT
from .fallback import complete as lexical_complete
from .backend import LocalInfillBackend, ModelUnavailable

class CompletionEngine:
    def __init__(self, backend: LocalInfillBackend | None=None, enabled: bool=True,
                 max_cache: int=128):
        self.backend=backend
        self.enabled=enabled
        self.revisions: dict[str,int]={}
        self.cache: OrderedDict[tuple,tuple[str,str]] = OrderedDict()
        self.max_cache=max_cache

    def set_revision(self, document_id: str, revision: int) -> None:
        old=self.revisions.get(document_id,-1)
        if revision>old: self.revisions[document_id]=revision

    @staticmethod
    def sanitize(candidate: str, suffix: str) -> str:
        if not isinstance(candidate,str): return ''
        if candidate.startswith('```') or '\x00' in candidate: return ''
        candidate=candidate[:MAX_OUTPUT]
        candidate=''.join(ch for ch in candidate if ch=='\n' or ch=='\t' or (ord(ch)>=32 and ch not in '\x7f'))
        if not candidate.strip(): return ''
        # Never insert the existing suffix verbatim by accident.
        if suffix and candidate.startswith(suffix[:min(len(suffix),32)]): return ''
        return candidate

    def complete(self, req: CompletionRequest) -> dict:
        t0=time.monotonic()
        known=self.revisions.get(req.document_id,-1)
        if req.revision < known:
            return self._result(req,'stale','','none',t0)
        self.set_revision(req.document_id,req.revision)
        if not self.enabled:
            return self._result(req,'disabled','','none',t0)
        key=(req.language,req.prefix,req.suffix,req.context,req.max_tokens)
        if key in self.cache:
            candidate,source=self.cache[key]
            self.cache.move_to_end(key)
            return self._result(req,'ok' if candidate else 'empty',candidate,'cache:'+source,t0)
        candidate=''
        source='lexical'
        if self.backend is not None:
            try:
                candidate=self.sanitize(self.backend.complete(req),req.suffix)
                source='model'
            except ModelUnavailable:
                source='lexical'
        if not candidate:
            candidate=self.sanitize(lexical_complete(req),req.suffix)
            source='lexical'
        self.cache[key]=(candidate,source)
        self.cache.move_to_end(key)
        while len(self.cache)>self.max_cache:
            self.cache.popitem(last=False)
        # In an actual asynchronous host, revise before accepting a response.
        if req.revision<self.revisions.get(req.document_id,-1):
            return self._result(req,'stale','','none',t0)
        return self._result(req,'ok' if candidate else 'empty',candidate,source,t0)

    @staticmethod
    def _result(req,status,suggestion,source,t0):
        return {'protocol':1,'id':req.id,'document_id':req.document_id,
                'revision':req.revision,'status':status,'suggestion':suggestion,
                'source':source,'elapsed_ms':round((time.monotonic()-t0)*1000,3)}
