"""Editor/LSP-facing helpers built on the AXON 2026 lossless CST.

The module is transport-neutral: an LSP server can translate these plain
dictionaries directly to protocol diagnostics, symbols, hover data, and edits.
"""
from .cst2026 import format_cst2026, parse_cst2026
from .v2026 import AxonSet, AxonTuple, Node

def diagnostics2026(source, *, options=None):
    """Return the CST diagnostics for *source* as LSP-shaped dictionaries.

Spans are UTF-8 byte offsets; ``line``/``character`` are 0-based for LSP.
Each entry carries any directly applicable fixes.
    """
    document = parse_cst2026(source, options=options)
    return [{'code': item.category, 'message': item.message, 'start_byte': item.span.start, 'end_byte': item.span.end, 'line': item.span.line - 1, 'character': item.span.column - 1, 'expected': list(item.expected), 'fixes': [{'title': fix.title, 'start_byte': fix.start, 'end_byte': fix.end, 'new_text': fix.replacement} for fix in item.fixes]} for item in document.diagnostics]

def hover2026(source, byte_offset, *, options=None):
    """Return the token at *byte_offset* as a hover payload.

Returns ``None`` when the offset falls outside every token.
    """
    token = parse_cst2026(source, options=options).token_at(byte_offset)
    if token is None:
        return None
    return {'kind': token.kind, 'text': token.text, 'start_byte': token.span.start, 'end_byte': token.span.end, 'line': token.span.line - 1, 'character': token.span.column - 1, 'trivia': token.trivia}

def formatting_edits2026(source, *, indent=2, options=None):
    """Return the whole-document edit that formats *source*.

Returns an empty list when the source is already formatted.
    """
    formatted = format_cst2026(source, indent=indent, options=options)
    if formatted == source:
        return []
    return [{'start_byte': 0, 'end_byte': len(source.encode('utf-8')), 'new_text': formatted}]

def document_symbols2026(source, *, options=None):
    """Return the node symbols in *source* with their structural paths.

Returns an empty list when the document has diagnostics, and visits each
shared value once so a cyclic graph terminates.
    """
    document = parse_cst2026(source, options=options)
    if document.diagnostics:
        return []
    symbols = []
    visited = set()

    def visit(value, path):
        if isinstance(value, (Node, list, tuple, AxonTuple, AxonSet, dict)):
            ident = id(value)
            if ident in visited:
                return
            visited.add(ident)
        if isinstance(value, Node):
            symbols.append({'name': value.name, 'kind': 'node', 'path': path})
            for key, child in value.attributes.items():
                visit(child, path + ('@', key))
            for index, child in enumerate(value.children):
                visit(child, path + (index,))
        elif isinstance(value, (list, tuple, AxonTuple)):
            for index, child in enumerate(value):
                visit(child, path + (index,))
        elif isinstance(value, AxonSet):
            for index, child in enumerate(value.items):
                visit(child, path + (index,))
        elif isinstance(value, dict):
            for key, child in value.items():
                visit(child, path + (key,))
    for index, value in enumerate(document.values):
        visit(value, (index,))
    return symbols
__all__ = ['diagnostics2026', 'hover2026', 'formatting_edits2026', 'document_symbols2026']