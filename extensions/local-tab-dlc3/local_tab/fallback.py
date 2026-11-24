"""Purely local no-model fallback: identifier completion, never chat."""
import re
from collections import Counter
from .protocol import CompletionRequest

_WORD = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
_TRAILING = re.compile(r'([A-Za-z_][A-Za-z0-9_]*)$')


def complete(req: CompletionRequest) -> str:
    match = _TRAILING.search(req.prefix)
    if not match: return ''
    fragment = match.group(1)
    if len(fragment) < 2: return ''
    # Prefer symbols from nearest prefix context and explicit local snippets.
    window = req.prefix[:-len(fragment)][-8000:]
    texts = [window, *req.context]
    counts: Counter[str] = Counter()
    order: dict[str, int] = {}
    for text in texts:
        for word in _WORD.findall(text):
            if word != fragment and word.startswith(fragment) and len(word) > len(fragment):
                counts[word] += 1
                order.setdefault(word, len(order))
    if not counts: return ''
    ranked = sorted(counts, key=lambda x:(-counts[x],order[x],len(x)))
    return ranked[0][len(fragment):]
