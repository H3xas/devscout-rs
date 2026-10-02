"""A small Rust lexer: reference-site classes, `#[test]` functions and `mod` declarations.

Positions follow LSIF: 0-based lines and 0-based UTF-16 columns.
"""

import bisect
from collections import namedtuple

Token = namedtuple("Token", "kind text line col end_line end_col")

KEYWORDS = frozenset(
    "as async await break const continue crate dyn else enum extern false fn for if impl in "
    "let loop match mod move mut pub ref return self Self static struct super trait true type "
    "union unsafe use where while".split()
)

# Tokens after which an uppercase name followed by `{` opens a block, not a struct literal.
_NOT_LITERAL_BEFORE = frozenset(
    ["impl", "for", "struct", "enum", "union", "trait", "mod", "fn", "match", "if", "while",
     "in", "where", "->", "type", "else", "loop", "unsafe", "dyn", "=>"]
)
PRIMITIVES = frozenset(
    "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64".split()
)
_TYPE_BEFORE = frozenset([":", "->", "impl", "dyn", "as", "&", "+", "where", "<", "*"])
_PUNCT2 = ("::", "->", "=>", "..", "==", "!=", "<=", ">=", "&&", "||")

CONTEXTS = ("plain", "macro argument", "macro_rules body", "attribute", "use tree")
SHAPES = ("path call", "method call", "path qualifier", "struct literal", "type position",
          "macro name", "other value reference", "operator", "literal", "lifetime", "whitespace")
_OTHER_SHAPES = {"punct": "operator", "lit": "literal", "lifetime": "lifetime"}


def _utf16_len(text):
    return sum(2 if ord(c) > 0xFFFF else 1 for c in text)


class _Cursor:
    def __init__(self, source):
        self.src = source
        self.i = 0
        self.line = 0
        self.col = 0

    def peek(self, k=0):
        j = self.i + k
        return self.src[j] if j < len(self.src) else ""

    def advance(self, n=1):
        for _ in range(n):
            if self.i >= len(self.src):
                return
            c = self.src[self.i]
            self.i += 1
            if c == "\n":
                self.line += 1
                self.col = 0
            else:
                self.col += 2 if ord(c) > 0xFFFF else 1


def _skip_block_comment(cur):
    depth = 0
    while cur.i < len(cur.src):
        if cur.peek() == "/" and cur.peek(1) == "*":
            depth += 1
            cur.advance(2)
        elif cur.peek() == "*" and cur.peek(1) == "/":
            depth -= 1
            cur.advance(2)
            if depth == 0:
                return
        else:
            cur.advance()


def _skip_quoted(cur, quote):
    cur.advance()
    while cur.i < len(cur.src) and cur.peek() != quote:
        if cur.peek() == "\\":
            cur.advance()
        cur.advance()
    cur.advance()


def _raw_string_len(src, i):
    """Length of a raw string starting at `src[i]` (`r`, `br` or `cr` prefix), else 0."""
    j = i
    if src[j] in "bc" and j + 1 < len(src) and src[j + 1] == "r":
        j += 1
    if src[j] != "r":
        return 0
    j += 1
    hashes = 0
    while j < len(src) and src[j] == "#":
        hashes += 1
        j += 1
    if j >= len(src) or src[j] != '"':
        return 0
    close = '"' + "#" * hashes
    end = src.find(close, j + 1)
    return (len(src) if end < 0 else end + len(close)) - i


def _char_literal_len(src, i):
    """Length of a char literal starting at the quote `src[i]`, or 0 for a lifetime or label."""
    if i + 1 >= len(src):
        return 0
    if src[i + 1] == "\\":
        end = src.find("'", i + 2)
        if src[i + 2:i + 3] == "'":
            end = src.find("'", i + 3)
        return 0 if end < 0 else end + 1 - i
    if i + 2 < len(src) and src[i + 2] == "'":
        return 3
    return 0


def _is_ident_start(c):
    return c == "_" or c.isalpha()


def _is_ident_char(c):
    return c == "_" or c.isalnum()


def tokenize(source):
    """Tokens with LSIF positions; comments dropped, literals kept as one `lit` token."""
    cur = _Cursor(source)
    out = []
    src = source
    while cur.i < len(src):
        c = cur.peek()
        if c.isspace():
            cur.advance()
            continue
        line, col, start = cur.line, cur.col, cur.i
        if c == "/" and cur.peek(1) == "/":
            while cur.i < len(src) and cur.peek() != "\n":
                cur.advance()
            continue
        if c == "/" and cur.peek(1) == "*":
            _skip_block_comment(cur)
            continue
        kind = None
        raw = _raw_string_len(src, cur.i) if c in "rbc" else 0
        if raw:
            cur.advance(raw)
            kind = "lit"
        elif c in "bc" and cur.peek(1) == '"':
            cur.advance()
            _skip_quoted(cur, '"')
            kind = "lit"
        elif c == "b" and cur.peek(1) == "'":
            cur.advance()
            n = _char_literal_len(src, cur.i)
            cur.advance(n or 1)
            kind = "lit"
        elif c == '"':
            _skip_quoted(cur, '"')
            kind = "lit"
        elif c == "'":
            n = _char_literal_len(src, cur.i)
            if n:
                cur.advance(n)
                kind = "lit"
            else:
                cur.advance()
                while cur.i < len(src) and _is_ident_char(cur.peek()):
                    cur.advance()
                kind = "lifetime"
        elif c == "r" and cur.peek(1) == "#" and _is_ident_start(cur.peek(2)):
            cur.advance(2)
            while cur.i < len(src) and _is_ident_char(cur.peek()):
                cur.advance()
            out.append(Token("ident", src[start + 2:cur.i], line, col, cur.line, cur.col))
            continue
        elif _is_ident_start(c):
            while cur.i < len(src) and _is_ident_char(cur.peek()):
                cur.advance()
            kind = "ident"
        elif c.isdigit():
            while cur.i < len(src) and _is_ident_char(cur.peek()):
                cur.advance()
            if cur.peek() == "." and cur.peek(1).isdigit():
                cur.advance()
                while cur.i < len(src) and _is_ident_char(cur.peek()):
                    cur.advance()
            kind = "lit"
        else:
            two = src[cur.i:cur.i + 2]
            cur.advance(2 if two in _PUNCT2 else 1)
            kind = "punct"
        out.append(Token(kind, src[start:cur.i], line, col, cur.line, cur.col))
    return out


_OPEN = {"(": ")", "[": "]", "{": "}"}


def contexts(tokens):
    """The context of every token: one of `CONTEXTS`."""
    result = ["plain"] * len(tokens)
    frames = []  # (closing char, frame kind)
    use_depth = None
    i = 0
    n = len(tokens)
    while i < n:
        t = tokens[i]
        kinds = {k for _, k in frames}
        if "attribute" in kinds:
            ctx = "attribute"
        elif "macro_rules" in kinds:
            ctx = "macro_rules body"
        elif "macro" in kinds:
            ctx = "macro argument"
        elif use_depth is not None:
            ctx = "use tree"
        else:
            ctx = "plain"
        result[i] = ctx
        text = t.text
        if t.kind == "punct" and text == "#":
            j = i + 1
            if j < n and tokens[j].text == "!":
                j += 1
            if j < n and tokens[j].text == "[":
                for k in range(i + 1, j + 1):
                    result[k] = "attribute" if ctx == "plain" else ctx
                frames.append(("]", "attribute"))
                i = j + 1
                continue
        if t.kind == "ident" and i + 2 < n and tokens[i + 1].text == "!" and tokens[i + 1].kind == "punct":
            nxt = tokens[i + 2]
            if text == "macro_rules" and nxt.kind == "ident" and i + 3 < n and tokens[i + 3].text in _OPEN:
                result[i + 1] = result[i + 2] = ctx
                result[i + 3] = ctx
                frames.append((_OPEN[tokens[i + 3].text], "macro_rules"))
                i += 4
                continue
            if nxt.text in _OPEN:
                result[i + 1] = result[i + 2] = ctx
                frames.append((_OPEN[nxt.text], "macro"))
                i += 3
                continue
        if t.kind == "ident" and text == "use" and use_depth is None and not frames_kind(frames, "macro_rules"):
            use_depth = len(frames)
        elif t.kind == "punct" and text == ";" and use_depth is not None and len(frames) == use_depth:
            use_depth = None
        if t.kind == "punct" and text in _OPEN:
            frames.append((_OPEN[text], "group"))
        elif t.kind == "punct" and text in (")", "]", "}"):
            if frames and frames[-1][0] == text:
                frames.pop()
            if use_depth is not None and len(frames) < use_depth:
                use_depth = None
        i += 1
    return result


def frames_kind(frames, kind):
    return any(k == kind for _, k in frames)


def _skip_turbofish(tokens, j):
    """Index after a `::<...>` starting at `j`, or `j` when none starts there."""
    n = len(tokens)
    if not (j + 1 < n and tokens[j].text == "::" and tokens[j + 1].text == "<"):
        return j
    depth = 0
    k = j + 1
    while k < n:
        text = tokens[k].text
        if text == "<":
            depth += 1
        elif text == ">":
            depth -= 1
            if depth == 0:
                return k + 1
        elif text == "->":
            pass
        elif text in (";", "{", "}"):
            return j
        k += 1
    return j


def shape(tokens, i):
    """The shape class (one of `SHAPES`) of the identifier token at `i`."""
    n = len(tokens)
    prev = tokens[i - 1].text if i > 0 else ""
    nxt_i = _skip_turbofish(tokens, i + 1)
    nxt = tokens[nxt_i].text if nxt_i < n else ""
    raw_next = tokens[i + 1].text if i + 1 < n else ""
    name = tokens[i].text
    if raw_next == "!" and tokens[i + 1].kind == "punct":
        return "macro name"
    if prev == ".":
        return "method call" if nxt == "(" else "other value reference"
    if nxt == "(" and prev != "fn":
        return "path call"
    if raw_next == "::":
        return "path qualifier"
    if raw_next == "{" and prev not in _NOT_LITERAL_BEFORE and name[:1].isupper():
        return "struct literal"
    if name[:1].islower() and name not in PRIMITIVES:
        return "other value reference"
    if prev in _TYPE_BEFORE or raw_next == "<":
        return "type position"
    if name[:1].isupper() and not name.isupper():
        return "type position"
    return "other value reference"


class Index:
    """Token lookup by (line, UTF-16 column) for one source file."""

    def __init__(self, source):
        self.source = source
        self.tokens = tokenize(source)
        self.contexts = contexts(self.tokens)
        self.by_pos = {(t.line, t.col): k for k, t in enumerate(self.tokens)}
        self.positions = [(t.line, t.col) for t in self.tokens]

    def site(self, line, col):
        """`(name, context, shape)` for the reference starting at the position, or None.

        An operator token (`==`, `-`, `[`, `?`, ...) resolves to a trait method in the oracle
        and gets the shape `operator`; a literal resolves to its primitive type (`literal`); a
        lifetime to its parameter (`lifetime`). A position between tokens is `whitespace`,
        with the context of the token after it.
        """
        k = self.by_pos.get((line, col))
        if k is None:
            k = bisect.bisect_left(self.positions, (line, col))
            if k >= len(self.tokens):
                return None
            return "", self.contexts[k], "whitespace"
        token = self.tokens[k]
        if token.kind == "ident":
            return token.text, self.contexts[k], shape(self.tokens, k)
        return token.text, self.contexts[k], _OTHER_SHAPES[token.kind]


def _matching(tokens, open_index):
    close = _OPEN[tokens[open_index].text]
    opener = tokens[open_index].text
    depth = 0
    for k in range(open_index, len(tokens)):
        text = tokens[k].text
        if tokens[k].kind != "punct":
            continue
        if text == opener:
            depth += 1
        elif text == close:
            depth -= 1
            if depth == 0:
                return k
    return len(tokens) - 1


def _split_top(tokens, start, end):
    """`tokens[start:end]` split at the commas outside every nested group, as index ranges."""
    parts, depth, begin = [], 0, start
    for k in range(start, end):
        token = tokens[k]
        if token.kind != "punct":
            continue
        if token.text in _OPEN:
            depth += 1
        elif token.text in (")", "]", "}"):
            depth -= 1
        elif token.text == "," and depth == 0:
            parts.append((begin, k))
            begin = k + 1
    parts.append((begin, end))
    return parts


def _attributes(tokens, start, end):
    """The attributes the body `tokens[start:end]` stands for, as `(start, end, carried)`.

    A `cfg_attr(<predicate>, <attr>, ...)` stands for its attributes, read again the same way
    and marked carried; its predicate is never evaluated. A malformed `cfg_attr` stands for
    nothing. Nesting is unwound on an explicit stack.
    """
    out = []
    stack = [(start, end, False)]
    while stack:
        lo, hi, carried = stack.pop()
        texts = [t.text for t in tokens[lo:hi]]
        if not texts or texts[0] != "cfg_attr":
            out.append((lo, hi, carried))
            continue
        if len(texts) < 3 or texts[1] != "(" or _matching(tokens, lo + 1) != hi - 1:
            continue
        parts = _split_top(tokens, lo + 2, hi - 1)
        if parts and parts[-1][0] == parts[-1][1] and len(parts) > 1:
            parts.pop()
        if len(parts) < 2 or any(a == b for a, b in parts):
            continue
        for a, b in reversed(parts[1:]):
            stack.append((a, b, True))
    out.sort()
    return out


def _attr_path(tokens, start, end):
    """The path of the attribute `tokens[start:end]` and the index after it."""
    path = []
    k = start
    if k < end and tokens[k].text == "::":
        k += 1
    while k < end and tokens[k].kind == "ident":
        path.append(tokens[k].text)
        if k + 1 < end and tokens[k + 1].text == "::":
            k += 2
        else:
            k += 1
            break
    return path, k


def _is_test_leaf(tokens, start, end):
    path, k = _attr_path(tokens, start, end)
    if not path or path[-1] != "test":
        return False
    return k == end or (tokens[k].text in _OPEN and _matching(tokens, k) == end - 1)


def _is_test_attr(tokens, start, end):
    """Whether the attribute body `tokens[start:end]` marks a test.

    A plain attribute marks one when it is `test` or a path ending `::test`; a `cfg_attr`
    marks one when one of its attributes does.
    """
    body = [t.text for t in tokens[start:end]]
    if body[:1] != ["cfg_attr"]:
        if "(" in body:
            body = body[:body.index("(")]
        return body == ["test"] or (len(body) >= 3 and body[-2:] == ["::", "test"])
    return any(carried and _is_test_leaf(tokens, a, b)
               for a, b, carried in _attributes(tokens, start, end))


def _literal(text):
    """The value of a plain `"..."` with no escape or a raw `r"..."` string token, else None."""
    if text.startswith('"') and text.endswith('"') and len(text) >= 2 and "\\" not in text:
        return text[1:-1]
    if text.startswith("r"):
        body = text[1:]
        hashes = len(body) - len(body.lstrip("#"))
        body = body[hashes:]
        close = '"' + "#" * hashes
        if body.startswith('"') and body.endswith(close) and len(body) >= 1 + len(close):
            return body[1:len(body) - len(close)]
    return None


def _path_attr(tokens, start, end):
    """What the attribute body `tokens[start:end]` says of a module's file.

    `None` when it names no `path`; `("path", literal)` for a plain `path = "..."`; and
    `("unread", None)` for a `path` whose value is not a plain string literal or that comes
    through `cfg_attr`.
    """
    for a, b, carried in _attributes(tokens, start, end):
        path, k = _attr_path(tokens, a, b)
        if path != ["path"]:
            continue
        if carried:
            return ("unread", None)
        if k + 2 == b and tokens[k].text == "=" and tokens[k + 1].kind == "lit":
            value = _literal(tokens[k + 1].text)
            if value is not None:
                return ("path", value)
        return ("unread", None)
    return None


def scan_items(source):
    """`#[test]` functions and `mod` declarations of one file.

    Returns `(tests, mods)`: each test is a dict with `name`, `nameLine`, `startLine`, `endLine`
    (1-based) and `modPath` (the enclosing inline modules); each mod is a dict with `name`,
    `modPath`, `inline` (whether it has a body), `path` (the literal of the first `#[path]` on
    a declaration without a body, else None) and `unread` (whether its file cannot be told: a
    `path` that is not a plain literal or comes through `cfg_attr`, or an enclosing inline
    module that carries one).
    """
    idx = Index(source)
    tokens = idx.tokens
    ctx = idx.contexts
    tests, mods = [], []
    mod_stack = []  # (closing token index, name, carries a path attribute)
    pending_test = None
    pending_path = None
    k = 0
    n = len(tokens)
    while k < n:
        while mod_stack and k > mod_stack[-1][0]:
            mod_stack.pop()
        t = tokens[k]
        if ctx[k] in ("macro argument", "macro_rules body"):
            k += 1
            continue
        if t.text == "#" and k + 1 < n and tokens[k + 1].text == "[":
            close = _matching(tokens, k + 1)
            if _is_test_attr(tokens, k + 2, close):
                pending_test = pending_test or t.line + 1
            pending_path = pending_path or _path_attr(tokens, k + 2, close)
            k = close + 1
            continue
        if t.kind == "ident" and t.text == "mod" and k + 1 < n and tokens[k + 1].kind == "ident":
            name = tokens[k + 1].text
            path = [m for _, m, _ in mod_stack]
            under_path = any(p for _, _, p in mod_stack)
            if k + 2 < n and tokens[k + 2].text == "{":
                mods.append({"name": name, "modPath": path, "inline": True, "path": None,
                             "unread": False})
                mod_stack.append((_matching(tokens, k + 2), name,
                                  under_path or pending_path is not None))
                k += 3
            else:
                literal = pending_path[1] if pending_path and not under_path else None
                unread = under_path or (pending_path is not None and literal is None)
                mods.append({"name": name, "modPath": path, "inline": False, "path": literal,
                             "unread": unread})
                k += 2
            pending_test = None
            pending_path = None
            continue
        if t.kind == "ident" and t.text == "fn" and k + 1 < n and tokens[k + 1].kind == "ident":
            body = k + 2
            while body < n and tokens[body].text not in ("{", ";"):
                body += 1
            end = _matching(tokens, body) if body < n and tokens[body].text == "{" else body
            if pending_test is not None:
                tests.append({
                    "name": tokens[k + 1].text,
                    "nameLine": tokens[k + 1].line + 1,
                    "startLine": pending_test,
                    "endLine": tokens[min(end, n - 1)].end_line + 1,
                    "modPath": [m for _, m, _ in mod_stack],
                })
            pending_test = None
            pending_path = None
            k = end + 1
            continue
        if t.kind == "punct" and t.text in (";", "{", "}"):
            pending_test = None
            pending_path = None
        k += 1
    return tests, mods
