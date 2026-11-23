"""Versioned, bounded JSON-Lines protocol for local-only Tab completion."""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Any

LANGUAGES = frozenset({'python','rust','c','cpp','zig','nix','java','kotlin','swift','typescript','javascript','text'})
MAX_BYTES = 128 * 1024
MAX_PREFIX = 16000
MAX_SUFFIX = 8000
MAX_CONTEXT = 8
MAX_CONTEXT_CHARS = 2000
MAX_OUTPUT = 2048

class ProtocolError(ValueError):
    pass

@dataclass(frozen=True)
class CompletionRequest:
    id: str
    document_id: str
    revision: int
    language: str
    prefix: str
    suffix: str
    context: tuple[str, ...] = field(default_factory=tuple)
    max_tokens: int = 48

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> 'CompletionRequest':
        if not isinstance(data, dict): raise ProtocolError('request must be an object')
        if data.get('op') != 'complete': raise ProtocolError('unsupported operation')
        for key in ('id', 'document_id', 'prefix', 'suffix'):
            if not isinstance(data.get(key), str): raise ProtocolError(f'{key} must be a string')
        if not data['id'] or len(data['id']) > 128: raise ProtocolError('invalid id')
        if not data['document_id'] or len(data['document_id']) > 1024: raise ProtocolError('invalid document_id')
        revision = data.get('revision')
        if type(revision) is not int or revision < 0 or revision > 2**63-1:
            raise ProtocolError('invalid document revision')
        language = data.get('language', 'text')
        if not isinstance(language, str) or len(language) > 64: raise ProtocolError('invalid language')
        language = language.lower()
        if language not in LANGUAGES: language = 'text'
        if len(data['prefix']) > MAX_PREFIX or len(data['suffix']) > MAX_SUFFIX:
            raise ProtocolError('prefix/suffix exceed bounded context')
        raw_context = data.get('context', [])
        if not isinstance(raw_context, list) or len(raw_context) > MAX_CONTEXT:
            raise ProtocolError('invalid context list')
        if any(not isinstance(x, str) or len(x)>MAX_CONTEXT_CHARS for x in raw_context):
            raise ProtocolError('invalid context entry')
        tokens = data.get('max_tokens', 48)
        if type(tokens) is not int or tokens < 1 or tokens > 128:
            raise ProtocolError('max_tokens must be 1..128')
        return cls(data['id'], data['document_id'], revision, language,
                   data['prefix'], data['suffix'], tuple(raw_context), tokens)
