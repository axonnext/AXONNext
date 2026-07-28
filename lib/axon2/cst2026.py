"""Lossless concrete syntax tree for AXON 2026.

The CST deliberately owns every source byte.  It is independent from the
semantic parser so editors can retain malformed input and diagnostics.
"""
from bisect import bisect_right
from dataclasses import dataclass, field, replace
from .v2026 import Axon2026Error, Options, loads2026

@dataclass(frozen=True)
class Span:
    """A byte-addressed source span.

Half-open ``[start, end)`` UTF-8 byte offsets, plus the 1-based line and
column of *start*.
    """
    start: int
    end: int
    line: int
    column: int

@dataclass(frozen=True)
class CstToken:
    """One lexical token, owning its exact source text.

Trivia (whitespace and comments) is attached to the neighbouring non-trivia
token as leading or trailing, so no byte is orphaned.
    """
    kind: str
    text: str
    span: Span
    trivia: bool = False
    leading_trivia: tuple = ()
    trailing_trivia: tuple = ()

@dataclass(frozen=True)
class CstDiagnostic:
    """A recoverable syntax problem.

Carries the span it covers, the tokens that were expected there, and any
directly applicable fixes.
    """
    category: str
    message: str
    span: Span
    expected: tuple[str, ...] = ()
    fixes: tuple['CstFix', ...] = ()

@dataclass(frozen=True)
class CstFix:
    """A byte-addressed, directly applicable editor repair."""
    title: str
    start: int
    end: int
    replacement: str

@dataclass
class CstNode:
    """A grammar-production node in the CST.

Children are tokens or further nodes; ``semantic_value`` links back to the
parsed value when this node produced one.
    """
    kind: str
    span: Span
    children: list = field(default_factory=list)
    semantic_value: object = None

    def tokens(self):
        """Yield every token beneath this node, in source order."""
        stack = list(reversed(self.children))
        while stack:
            child = stack.pop()
            if isinstance(child, CstToken):
                yield child
            else:
                stack.extend(reversed(child.children))

@dataclass(frozen=True)
class CstSemanticTrace:
    """A semantic value linked to its exact source span and nearest CST node."""
    span: Span
    value: object
    node: CstNode

@dataclass
class CstDocument:
    """A losslessly parsed document.

Concatenating the token texts reproduces the source exactly, so comments,
whitespace, and malformed regions all survive a parse/render round trip.
    """
    source: str
    tokens: list[CstToken]
    root: CstNode | None = None
    values: list = field(default_factory=list)
    diagnostics: list[CstDiagnostic] = field(default_factory=list)
    semantic_nodes: list[CstNode] = field(default_factory=list)
    semantic_traces: list[CstSemanticTrace] = field(default_factory=list)

    def render(self):
        """Reproduce the source text exactly by concatenating every token."""
        return ''.join((token.text for token in self.tokens))

    def bytes(self):
        """Return the rendered source as UTF-8 bytes."""
        return self.render().encode('utf-8')

    def token_at(self, byte_offset):
        """Return the token covering *byte_offset*, or ``None`` if there is none."""
        for token in self.tokens:
            if token.span.start <= byte_offset < token.span.end:
                return token
        return None

    def replace(self, start, end, replacement, *, options=None):
        """Return a re-parsed document with ``[start, end)`` replaced by *replacement*.

Both bounds must be UTF-8 scalar boundaries inside the document.
        """
        if not isinstance(start, int) or not isinstance(end, int):
            raise TypeError('CST edit bounds must be integers')
        if not isinstance(replacement, str):
            raise TypeError('CST replacement must be Unicode text')
        data = self.bytes()
        if start < 0 or end < start or end > len(data):
            raise ValueError('CST edit has invalid byte bounds')
        _require_utf8_boundary(data, start, 'CST edit start')
        _require_utf8_boundary(data, end, 'CST edit end')
        try:
            replacement_bytes = replacement.encode('utf-8')
        except UnicodeEncodeError as exc:
            raise ValueError('CST replacement is not scalar Unicode') from exc
        updated = data[:start] + replacement_bytes + data[end:]
        return parse_cst2026(updated.decode('utf-8'), options=options)

    def non_trivia(self):
        """Return the tokens that are neither whitespace nor comments."""
        return [token for token in self.tokens if not token.trivia]

def parse_cst2026(source, *, options=None):
    """Parse *source* into a lossless concrete syntax tree.

Every byte is retained, syntax problems are reported as diagnostics rather
than raised, and semantic values are attached to the nodes that produced
them. Resource limits come from *options*.
    """
    if not isinstance(source, str):
        raise TypeError('CST source must be Unicode text')
    options = options or Options()
    try:
        source_bytes = source.encode('utf-8')
    except UnicodeEncodeError as exc:
        prefix = source[:exc.start]
        raise Axon2026Error('invalid-unicode', 'CST source contains a non-scalar Unicode value', len(prefix.encode('utf-8')), prefix.count('\n') + 1, len(prefix.rsplit('\n', 1)[-1]) + 1) from exc
    if len(source_bytes) > options.max_input:
        raise Axon2026Error('resource-limit-exceeded', 'CST input is too large')
    tokens = _attach_trivia(_tokenize(source, options))
    source_end = len(source_bytes)
    root, diagnostics = _build_production_tree(tokens, source_end, options)
    diagnostics.extend(_recover_token_errors(tokens, source_end, options))
    values = []
    trace_events = []
    try:
        values = loads2026(source, options=options, _trace=trace_events)
    except Axon2026Error as error:
        start = error.offset
        terminal_recovery = error.category == 'unexpected-end' and any((item.category == error.category and item.span.end == source_end for item in diagnostics))
        if not terminal_recovery and (not any((item.span.start == start and item.category == error.category for item in diagnostics))):
            diagnostics.append(_semantic_diagnostic(error, source, tokens))
    root.semantic_value = tuple(values)
    slots = [child for child in root.children if isinstance(child, CstNode) and child.kind == 'stream-value']
    candidates = [node for node in slots if not (node.children and isinstance(node.children[0], CstNode) and (node.children[0].kind == 'discard-operand'))]
    semantic_nodes = candidates[-len(values):] if values else []
    for node, value in zip(semantic_nodes, values):
        node.semantic_value = value
    semantic_traces = _attach_semantic_traces(root, source, tokens, trace_events)
    diagnostics = _dedupe_diagnostics(diagnostics)
    diagnostics.sort(key=lambda item: (item.span.start, item.span.end, item.category))
    document = CstDocument(source, tokens, root, values, diagnostics, semantic_nodes, semantic_traces)
    if document.render() != source:
        raise AssertionError('CST tokenizer failed to own every source byte')
    return document

def _build_production_tree(tokens, source_end, options):
    root_span = Span(0, source_end, 1, 1)
    root = CstNode('document', root_span, [])
    stack = [(root, None)]
    diagnostics = []
    depth_exceeded = False
    openers = {'{': ('}', 'brace'), '[': (']', 'bracket'), '(': (')', 'tuple')}
    closers = {'}', ']', ')'}
    for token in tokens:
        text = token.text
        current = stack[-1][0]
        if text in openers and token.kind == 'punctuation':
            expected, kind = openers[text]
            node = CstNode(kind, token.span, [token])
            current.children.append(node)
            stack.append((node, expected))
            if len(stack) - 1 > options.max_depth and (not depth_exceeded):
                depth_exceeded = True
                diagnostics.append(CstDiagnostic('resource-limit-exceeded', 'maximum CST nesting depth exceeded', token.span))
            continue
        if text in closers and token.kind == 'punctuation':
            if len(stack) == 1:
                current.children.append(token)
                diagnostics.append(CstDiagnostic('unexpected-token', 'unmatched closing delimiter', token.span, ('value',), (CstFix('Remove unmatched delimiter', token.span.start, token.span.end, ''),)))
                continue
            node, expected = stack[-1]
            node.children.append(token)
            node.span = Span(node.span.start, token.span.end, node.span.line, node.span.column)
            if text != expected:
                diagnostics.append(CstDiagnostic('unexpected-token', 'mismatched delimiter: expected %s' % expected, token.span, (expected,), (CstFix('Replace with %s' % expected, token.span.start, token.span.end, expected),)))
            stack.pop()
            continue
        if token.trivia or token.kind == 'punctuation':
            current.children.append(token)
        else:
            production = {'name': 'name', 'quoted-name': 'name', 'string': 'string-literal', 'raw-string': 'string-literal', 'binary': 'binary-literal', 'temporal': 'temporal-literal', 'atom': 'scalar-literal', 'constant': 'constant-reference', 'anchor': 'anchor-prefix', 'reference': 'reference-prefix', 'discard': 'discard-prefix'}.get(token.kind, 'lexical-value')
            current.children.append(CstNode(production, token.span, [token]))
    while len(stack) > 1:
        node, expected = stack.pop()
        diagnostics.append(CstDiagnostic('unexpected-end', 'unterminated %s; expected %s' % (node.kind, expected), Span(source_end, source_end, node.span.line, node.span.column), (expected,), (CstFix('Insert missing %s' % expected, source_end, source_end, expected),)))
        node.span = Span(node.span.start, source_end, node.span.line, node.span.column)
    if not depth_exceeded:
        _refine_production_tree(root, options)
    return (root, diagnostics)

def _recover_token_errors(tokens, source_end, options):
    """Collect independent local repairs without discarding any source byte.

    The semantic parser remains the authority for validity.  This pass is
    intentionally conservative: it only reports token arrangements whose
    repair is unambiguous and can therefore coexist with later diagnostics.
    """
    diagnostics = []
    structural = [token for token in tokens if not token.trivia]
    openers = {'{', '[', '('}
    closers = {'}', ']', ')'}
    prefixes = {'anchor': ('anchor label',), 'reference': ('reference label',), 'constant': ('constant name',), 'discard': ('value',)}
    for index, token in enumerate(structural):
        previous = structural[index - 1] if index else None
        following = structural[index + 1] if index + 1 < len(structural) else None
        if token.text == ',' and (previous is None or previous.text in openers or previous.text == ','):
            diagnostics.append(CstDiagnostic('unexpected-token', 'comma does not have a preceding item', token.span, ('value',), (CstFix('Remove comma', token.span.start, token.span.end, ''),)))
        if token.kind in prefixes and (following is None or following.text in closers or following.text == ','):
            expected = prefixes[token.kind]
            diagnostics.append(CstDiagnostic('unexpected-end' if following is None else 'unexpected-token', '%s requires %s' % (token.text, expected[0]), Span(token.span.end, token.span.end, token.span.line, token.span.column + len(token.text)), expected))
        if token.text == ':' and (following is None or following.text in closers or following.text == ','):
            diagnostics.append(CstDiagnostic('unexpected-end' if following is None else 'unexpected-token', "pair requires a value after ':'", Span(token.span.end, token.span.end, token.span.line, token.span.column + 1), ('value',), (CstFix('Insert null value', token.span.end, token.span.end, 'null'),)))
        if token.kind in ('string', 'quoted-name'):
            delimiter = token.text[:1]
            if delimiter and (not _quoted_token_closed(token.text, delimiter)):
                diagnostics.append(CstDiagnostic('unexpected-end', 'unterminated quoted token', token.span, (delimiter,), (CstFix('Insert closing %s' % delimiter, token.span.end, token.span.end, delimiter),)))
        if token.kind == 'raw-string':
            opener = token.text.split('"', 1)[0]
            closer = '"' + '#' * opener.count('#')
            if not token.text.endswith(closer):
                diagnostics.append(CstDiagnostic('unexpected-end', 'unterminated raw string', token.span, (closer,), (CstFix('Insert raw-string terminator', token.span.end, token.span.end, closer),)))
        if token.kind == 'binary' and (not (options.legacy_binary or options.compatibility)) and (not (len(token.text) >= 2 and token.text.endswith('|'))):
            diagnostics.append(CstDiagnostic('unexpected-end', 'binary literal requires a closing pipe', Span(source_end, source_end, token.span.line, token.span.column), ('|',), (CstFix('Insert closing pipe', source_end, source_end, '|'),)))
        if token.kind == 'temporal' and '[' in token.text and (not token.text.endswith(']')):
            diagnostics.append(CstDiagnostic('unexpected-end', "named time-zone suffix requires ']'", token.span, (']',), (CstFix('Insert closing ]', token.span.end, token.span.end, ']'),)))
    return diagnostics

def _quoted_token_closed(text, delimiter):
    if len(text) < 2 or not text.endswith(delimiter):
        return False
    backslashes = 0
    index = len(text) - 2
    while index >= 0 and text[index] == '\\':
        backslashes += 1
        index -= 1
    return backslashes % 2 == 0

def _semantic_diagnostic(error, source, tokens):
    start = error.offset
    expected = ()
    fixes = ()
    message = str(error)
    lower = message.lower()
    if 'expected a value' in lower or 'requires a pair' in lower:
        expected = ('value',)
    elif 'expected anchor label' in lower:
        expected = ('anchor label',)
    elif 'expected a name' in lower:
        expected = ('name',)
    elif 'closing pipe' in lower or 'binary literal' in lower:
        expected = ('|',)
        fixes = (CstFix('Insert closing pipe', start, start, '|'),)
    elif 'unterminated quoted name' in lower:
        expected = ("'",)
        fixes = (CstFix('Insert closing quote', start, start, "'"),)
    elif 'unterminated string' in lower:
        expected = ('"',)
        fixes = (CstFix('Insert closing quote', start, start, '"'),)
    elif 'unterminated named time-zone' in lower:
        expected = (']',)
        fixes = (CstFix('Insert closing ]', start, start, ']'),)
    elif 'consecutive commas' in lower:
        token = next((item for item in tokens if item.span.start <= start < item.span.end), None)
        if token is not None:
            expected = ('value',)
            fixes = (CstFix('Remove comma', token.span.start, token.span.end, ''),)
    return CstDiagnostic(error.category, message, Span(start, start, error.line, error.column), expected, fixes)

def _dedupe_diagnostics(diagnostics):
    result = []
    seen = set()
    for item in diagnostics:
        key = (item.category, item.span.start, item.span.end, item.expected)
        if key in seen:
            continue
        seen.add(key)
        result.append(item)
    return result

def _node_span(children):
    first = next((child for child in children if isinstance(child, (CstNode, CstToken))))
    last = next((child for child in reversed(children) if isinstance(child, (CstNode, CstToken))))
    return Span(first.span.start, last.span.end, first.span.line, first.span.column)

def _next_structural(children, start):
    for index in range(start, len(children)):
        child = children[index]
        if isinstance(child, CstToken) and child.trivia:
            continue
        return index
    return None

def _refine_production_tree(root, options):
    """Add grammar-production nodes without changing leaf-token ownership."""

    def refine(node):
        for child in list(node.children):
            if isinstance(child, CstNode) and child.kind in ('brace', 'bracket', 'tuple'):
                refine(child)
        source_children = node.children
        children = []
        cursor = 0
        while cursor < len(source_children):
            child = source_children[cursor]
            next_index = _next_structural(source_children, cursor + 1)
            next_child = source_children[next_index] if next_index is not None else None
            if isinstance(child, CstNode) and child.kind == 'name' and isinstance(next_child, CstNode) and (next_child.kind in ('brace', 'bracket', 'tuple')) and ((options.compatibility or options.legacy_binding) or not any((isinstance(item, CstToken) and ('\n' in item.text or '\r' in item.text) for item in source_children[cursor + 1:next_index]))):
                segment = source_children[cursor:next_index + 1]
                child.kind = 'node-name'
                body = CstNode('body', next_child.span, [next_child])
                segment[-1] = body
                children.append(CstNode('node', _node_span(segment), segment))
                cursor = next_index + 1
            else:
                children.append(child)
                cursor += 1
        output = []
        index = 0
        while index < len(children):
            child = children[index]
            if not isinstance(child, CstNode):
                output.append(child)
                index += 1
                continue
            next_index = _next_structural(children, index + 1)
            next_child = children[next_index] if next_index is not None else None
            if child.kind in ('name', 'string-literal'):
                colon_index = next_index
                colon = next_child
                value_index = _next_structural(children, colon_index + 1) if colon_index is not None else None
                if isinstance(colon, CstToken) and colon.text == ':' and (value_index is not None):
                    segment = children[index:value_index + 1]
                    output.append(CstNode('pair', _node_span(segment), segment))
                    index = value_index + 1
                    continue
            if child.kind == 'discard-prefix' and next_index is not None:
                segment = children[index:next_index + 1]
                output.append(CstNode('discard-operand', _node_span(segment), segment))
                index = next_index + 1
                continue
            if child.kind in ('name', 'string-literal', 'binary-literal', 'temporal-literal', 'scalar-literal', 'constant-reference', 'anchor-prefix', 'reference-prefix', 'lexical-value'):
                output.append(CstNode('atom', child.span, [child]))
            else:
                output.append(child)
            index += 1
        node.children = output
    refine(root)
    structural = [child for child in root.children if isinstance(child, CstNode)]
    if structural and any((child.kind == 'pair' for child in structural)) and all((child.kind in ('pair', 'discard-operand') for child in structural)):
        document = CstNode('attribute-document', _node_span(root.children), root.children)
        root.children = [CstNode('stream-value', document.span, [document])]
        return
    wrapped = []
    for child in root.children:
        if isinstance(child, CstNode):
            wrapped.append(CstNode('stream-value', child.span, [child]))
        else:
            wrapped.append(child)
    root.children = wrapped

def _attach_trivia(tokens):
    """Attach trivia references according to the normative CST rule."""
    leading = {index: [] for index, token in enumerate(tokens) if not token.trivia}
    trailing = {index: [] for index, token in enumerate(tokens) if not token.trivia}
    non_trivia = [index for index, token in enumerate(tokens) if not token.trivia]
    boundaries = [-1] + non_trivia + [len(tokens)]
    for position in range(len(boundaries) - 1):
        previous = boundaries[position]
        following = boundaries[position + 1]
        run = list(range(previous + 1, following))
        if not run:
            continue
        if previous < 0:
            if following < len(tokens):
                leading[following].extend((tokens[index] for index in run))
            continue
        if following >= len(tokens):
            trailing[previous].extend((tokens[index] for index in run))
            continue
        previous_end_line = tokens[previous].span.line + tokens[previous].text.count('\n')
        same_line_comment = None
        for offset, token_index in enumerate(run):
            token = tokens[token_index]
            if token.kind == 'comment' and token.span.line == previous_end_line:
                same_line_comment = offset
                break
        if same_line_comment is None:
            leading[following].extend((tokens[index] for index in run))
        else:
            split = same_line_comment + 1
            trailing[previous].extend((tokens[index] for index in run[:split]))
            leading[following].extend((tokens[index] for index in run[split:]))
    result = list(tokens)
    for index in non_trivia:
        result[index] = replace(tokens[index], leading_trivia=tuple(leading[index]), trailing_trivia=tuple(trailing[index]))
    return result

def migrate_indented2026(source, *, options=None):
    """Rewrite historical indentation-bound node bodies as Core brace bodies.

    Comments, blank lines, spelling, and the indentation inside each migrated
    body are retained.  Only the opening/closing braces required to remove
    indentation-sensitive semantics are inserted.
    """
    if not isinstance(source, str):
        raise TypeError('migration source must be Unicode text')
    parse_cst2026(source, options=options)
    lines = source.splitlines(keepends=True)
    significant = []
    for index, line in enumerate(lines):
        content = line.lstrip(' \t')
        if content.strip() and (not content.startswith('#')):
            significant.append((index, _indent_width(line), content.rstrip('\r\n')))
    next_significant = {significant[pos][0]: significant[pos + 1] if pos + 1 < len(significant) else None for pos in range(len(significant))}
    output = []
    stack = []
    for index, line in enumerate(lines):
        stripped = line.lstrip(' \t')
        if stripped.strip() and (not stripped.startswith('#')):
            current_indent = _indent_width(line)
            while stack and current_indent <= stack[-1]:
                output.append(' ' * stack.pop() + '}\n')
            following = next_significant.get(index)
            code = stripped.rstrip('\r\n')
            if _indented_node_name(code) and following is not None and (following[1] > current_indent):
                newline = '\r\n' if line.endswith('\r\n') else '\n'
                output.append(line[:len(line) - len(stripped)] + code + '{' + newline)
                stack.append(current_indent)
                continue
        output.append(line)
    while stack:
        output.append(' ' * stack.pop() + '}\n')
    result = ''.join(output)
    loads2026(result, options=options)
    return result

def format_cst2026(source, *, indent=2, options=None):
    """Format Core AXON structurally while retaining every comment token."""
    if not isinstance(indent, int) or indent < 0:
        raise ValueError('indent must be a non-negative integer')
    source = migrate_indented2026(source, options=options)
    document = parse_cst2026(source, options=options)
    if document.diagnostics:
        diagnostic = document.diagnostics[0]
        raise Axon2026Error(diagnostic.category, diagnostic.message, diagnostic.span.start, diagnostic.span.line, diagnostic.span.column)
    lines = []
    current = ''
    depth = 0
    previous = ''
    previous_text = ''
    last_source_line = 1

    def ensure_indent():
        nonlocal current
        if not current:
            current = ' ' * (depth * indent)

    def flush():
        nonlocal current
        if current.strip():
            lines.append(current.rstrip())
        current = ''
    for token in document.tokens:
        if token.kind == 'whitespace':
            continue
        text = token.text
        if depth == 0 and current.strip() and (token.span.line > last_source_line) and (token.kind != 'comment'):
            flush()
        if token.kind == 'comment':
            ensure_indent()
            if current.strip():
                current += ' '
            current += text
            flush()
            previous = 'comment'
            last_source_line = token.span.line
            continue
        if text in '}])':
            flush()
            depth = max(0, depth - 1)
            ensure_indent()
            current += text
            previous = text
            continue
        ensure_indent()
        if text in '{[(':
            binds_to_name = previous == 'name' and previous_text not in ('null', 'true', 'false')
            if current.strip() and (not binds_to_name) and (previous not in ('anchor', 'reference')):
                current += ' '
            current += text
            depth += 1
            flush()
        elif text == ':':
            current = current.rstrip() + ':'
        elif text == ',':
            current = current.rstrip()
            flush()
        else:
            if current.strip() and (not current.rstrip().endswith((':', '&', '*', '$', '^'))) and (previous not in ('anchor', 'reference', 'constant', 'temporal-marker')):
                current += ' '
            current += text
        previous = token.kind if token.kind != 'punctuation' else text
        previous_text = text
        last_source_line = token.span.line
    flush()
    result = '\n'.join(lines) + ('\n' if lines else '')
    loads2026(result, options=options)
    return result

def _indent_width(line):
    width = 0
    for ch in line:
        if ch == ' ':
            width += 1
        elif ch == '\t':
            width = (width // 8 + 1) * 8
        else:
            break
    return width

def _indented_node_name(text):
    parts = text.split('/')
    if len(parts) > 2 or any((not part for part in parts)):
        return False
    return all((part.isidentifier() for part in parts))

def _tokenize(source, options):
    byte_offset = 0
    line = 1
    column = 1
    tokens = []
    i = 0
    n = len(source)

    def emit(kind, start, end, trivia=False):
        nonlocal byte_offset, line, column
        text = source[start:end]
        start_byte = byte_offset
        start_line = line
        start_column = column
        tokens.append(CstToken(kind, text, Span(start_byte, start_byte + len(text.encode('utf-8')), start_line, start_column), trivia))
        if len(tokens) > options.max_cst_tokens:
            raise Axon2026Error('resource-limit-exceeded', 'maximum CST token count exceeded', start_byte, start_line, start_column)
        for current in text:
            byte_offset += len(current.encode('utf-8'))
            if current == '\n':
                line += 1
                column = 1
            else:
                column += 1
    while i < n:
        start = i
        ch = source[i]
        if ch in ' \t\r\n':
            i += 1
            while i < n and source[i] in ' \t\r\n':
                i += 1
            emit('whitespace', start, i, True)
            continue
        if source.startswith('#_', i) and (options.hash_underscore_comment or options.compatibility):
            i += 2
            while i < n and source[i] not in '\r\n':
                i += 1
            emit('comment', start, i, True)
            continue
        if source.startswith('#_', i):
            i += 2
            emit('discard', start, i)
            continue
        if ch == '#':
            i += 1
            while i < n and source[i] not in '\r\n':
                i += 1
            emit('comment', start, i, True)
            continue
        if ch == 'r' and i + 1 < n and (source[i + 1] in '#"'):
            j = i + 1
            while j < n and source[j] == '#':
                j += 1
            if j < n and source[j] == '"':
                hashes = j - (i + 1)
                endmark = '"' + '#' * hashes
                stop = source.find(endmark, j + 1)
                i = n if stop < 0 else stop + len(endmark)
                emit('raw-string', start, i)
                continue
        if ch in ('"', '`', "'"):
            end = ch
            i += 1
            escaped = False
            while i < n:
                current = source[i]
                i += 1
                if escaped:
                    escaped = False
                elif current == '\\':
                    escaped = True
                elif current == end:
                    break
            emit('quoted-name' if ch == "'" else 'string', start, i)
            continue
        if ch == '|':
            i += 1
            if options.legacy_binary or options.compatibility:
                while i < n:
                    current = source[i]
                    if current == '=':
                        while i < n and source[i] == '=':
                            i += 1
                        break
                    if current in 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/' or ord(current) <= 32:
                        i += 1
                        continue
                    break
            else:
                while i < n and source[i] != '|':
                    i += 1
                if i < n:
                    i += 1
            emit('binary', start, i)
            continue
        if ch == '^':
            i += 1
            in_zone = False
            while i < n:
                current = source[i]
                if current == '[':
                    in_zone = True
                    i += 1
                    continue
                if current == ']' and in_zone:
                    i += 1
                    break
                if not in_zone and (current in ' \t\r\n' or current in ',]})#'):
                    break
                i += 1
            emit('temporal', start, i)
            continue
        if ch in '{}[]():,&*$^∅':
            i += 1
            emit({'&': 'anchor', '*': 'reference', '$': 'constant', '^': 'temporal-marker'}.get(ch, 'punctuation'), start, i)
            continue
        if ch.isdigit() or ch in '-?∞':
            i += 1
            while i < n and source[i] not in ' \t\r\n' and (source[i] not in '{}[](),#'):
                i += 1
            emit('atom', start, i)
            continue
        i += 1
        while i < n and source[i] not in ' \t\r\n' and (source[i] not in '{}[]():,&*$^#'):
            i += 1
        emit('name', start, i)
    return tokens

def _require_utf8_boundary(data, offset, label):
    try:
        data[:offset].decode('utf-8')
    except UnicodeDecodeError as exc:
        raise ValueError('%s is not a UTF-8 scalar boundary' % label) from exc

def _attach_semantic_traces(root, source, tokens, trace_events):
    if not trace_events:
        return []
    boundaries = {0: 0}
    source_positions = {0: (1, 1)}
    character_offset = 0
    for token in tokens:
        boundaries[character_offset] = token.span.start
        source_positions[character_offset] = (token.span.line, token.span.column)
        character_offset += len(token.text)
        boundaries[character_offset] = token.span.end
    newline_offsets = [index for index, character in enumerate(source) if character == '\n']
    exact = {}
    child_indexes = {}
    stack = [(root, 0)]
    while stack:
        node, depth = stack.pop()
        key = (node.span.start, node.span.end)
        previous = exact.get(key)
        if previous is None or depth > previous[1]:
            exact[key] = (node, depth)
        children = sorted((child for child in node.children if isinstance(child, CstNode)), key=lambda child: (child.span.start, child.span.end))
        child_indexes[id(node)] = ([child.span.start for child in children], children)
        stack.extend(((child, depth + 1) for child in reversed(children)))

    def nearest_owner(start, end):
        matched = exact.get((start, end))
        if matched is not None:
            return matched[0]
        current = root
        while True:
            starts, children = child_indexes[id(current)]
            index = bisect_right(starts, start) - 1
            if index < 0:
                return current
            candidate = children[index]
            if not (candidate.span.start <= start and candidate.span.end >= end):
                return current
            current = candidate

    def line_column(character):
        known = source_positions.get(character)
        if known is not None:
            return known
        line_index = bisect_right(newline_offsets, character - 1)
        previous_newline = newline_offsets[line_index - 1] if line_index else -1
        return (line_index + 1, character - previous_newline)
    traces = []
    for start_character, end_character, value in trace_events:
        start = boundaries.get(start_character)
        end = boundaries.get(end_character)
        if start is None:
            start = len(source[:start_character].encode('utf-8'))
        if end is None:
            end = len(source[:end_character].encode('utf-8'))
        node = nearest_owner(start, end)
        if node.semantic_value is None:
            node.semantic_value = value
        line, column = line_column(start_character)
        traces.append(CstSemanticTrace(Span(start, end, line, column), value, node))
    return traces
__all__ = ['Span', 'CstToken', 'CstFix', 'CstDiagnostic', 'CstNode', 'CstSemanticTrace', 'CstDocument', 'parse_cst2026', 'migrate_indented2026', 'format_cst2026']