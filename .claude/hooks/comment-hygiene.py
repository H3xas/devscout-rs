#!/usr/bin/env python3
"""comment-hygiene: keep comments and commit messages carrying a "why", not a
transcript of how they were written, and keep roadmap decomposition
vocabulary (`Unit A3 item 4`, bare `item 7`) out of shipped comments.

This is the canonical copy. Public repos vendor a byte-identical copy at
`.claude/hooks/comment-hygiene.py`; `tools/check-hygiene-sync.sh` (next to
this file) compares a vendored copy's bytes against this one, and
`--selfcheck` lets a vendored copy verify nobody edited its body locally
without refreshing HYGIENE_SHA256.

Five ways to run it:

  1. Hook mode (default, no args). Reads a single Claude Code hook JSON
     payload from stdin. For an Edit or Write tool call, scans the new
     comment text -- but only when the target path is one `--scan` would
     itself read (see in_scan_scope), so the hook and the gate refuse the
     same set; for a Bash `git commit`, scans the commit message. On a
     match it prints a deny decision to stdout, a one-line reason to
     stderr, and exits 2. Otherwise it exits 0 without printing anything.
     Never raises: any parse problem is treated as "nothing to flag".

  2. `--scan [paths...]` walks git-tracked files (default: every tracked
     file, i.e. the full tree) and applies the comment-line checks to code
     files only (.rs .cs .ts .tsx .js .py .sh .yml .yaml .toml -- never
     .md), printing one `path:line: <class>: <line>` per hit. Exit 1 if
     any hit was found, 0 otherwise. The scanner's own file and anything
     under a `tests/data/` directory (this script's own test fixtures, or
     a vendoring repo's equivalent) are always skipped, regardless of
     which paths were passed. Explicit paths narrow the scan the same way
     the old fixed-path default used to.

  3. `--scan-commits <git-log-range>` applies the commit-message checks to
     every commit message in the range. Exit 1 if any hit was found.

  4. `--require-signoff <git-log-range>` checks that every commit in the
     range carries a `Signed-off-by:` trailer, printing one
     `<sha>: missing Signed-off-by trailer` line per commit that lacks it.
     Exit 1 if any commit is missing one, 0 otherwise.

  5. `--selfcheck` recomputes this file's own sha256 (with the recorded
     HYGIENE_SHA256 value normalized out first, so the hash does not need
     to hash itself) and compares it against the embedded HYGIENE_SHA256.
     Exit 1 and print a reason on drift; exit 0 and print the version and
     hash otherwise.

A repo may drop a `.claude/hooks/comment-hygiene.allow` file (one prefix per
line, `#` comments allowed) next to its vendored copy to exempt tracker-id
look-alike prefixes particular to that repo -- e.g. a two-letter test-case id
API whose call sites read like id of a bracketed pair of dash-numbered
strings, which is not a tracker id. This never touches the plan-label,
case-note, source-citation, tool-name or test-narration classes; it only
widens the tracker-id STANDARDS_ALLOWLIST for that one repo, so the sha
check can keep every copy's body identical.

Python 3 standard library only -- this has to run unmodified on macOS and
Ubuntu CI runners with nothing extra installed.
"""

import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

HYGIENE_VERSION = "1.2.0"
# Frozen by hashing this file with this value normalized to 64 zeros first;
# --selfcheck redoes that normalization, so editing the body without
# refreshing this constant is exactly the drift --selfcheck exists to catch.
HYGIENE_SHA256 = "4fad4465a36f608499c554cd0d8db1ab42285174d873832d7a3a0df04bee22f5"

# `--scan` with no explicit paths now walks every git-tracked file (see
# should_ignore_for_scan for what still gets skipped). Kept as a name, not a
# literal [], so a caller can see at a glance that "no paths" means "no
# path filter" rather than "no files".
DEFAULT_SCAN_PATHS = []

# Code files only -- scan mode never looks at prose (.md), regardless of
# which paths it is pointed at.
SCANNABLE_EXTENSIONS = {"rs", "cs", "ts", "tsx", "js", "py", "sh", "yml", "yaml", "toml"}

# Standards / spec prefixes that happen to look like TICKET-123 but are not
# tracker ids (SHA-256, UTF-8, HTTP-01, ...). Extend per repo with a
# `.claude/hooks/comment-hygiene.allow` file, not by editing this set.
STANDARDS_ALLOWLIST = {
    "UTF", "ISO", "SHA", "RFC", "IEEE", "CVE", "IEC", "ECMA", "ASCII",
    "TLS", "SSL", "AES", "RSA", "CRC", "HTTP", "ES", "X", "MD",
}

HASH_COMMENT_EXTENSIONS = {"sh", "py", "yml", "yaml", "toml"}

# A line is "a comment line" if it opens with one of the C-style markers, or
# (only for the extensions above) with '#'.
COMMENT_LINE_RE = re.compile(r"^\s*(?://!|///|//|/\*|\*)")
HASH_LINE_RE = re.compile(r"^\s*#")

# ---- the six comment-content classes --------------------------------------

CASE_NOTE_RE = re.compile(r"^\s*(//|#|\*|/\*)\s*Case\s+[A-Za-z0-9-]+\s*:")
SOURCE_CITATION_RE = re.compile(
    r"\b(src|tests|fixtures|tools)/[A-Za-z0-9_./-]+\.(rs|cs|ts|js|py)\s*:\s*[0-9]+"
)
TRACKER_ID_RE = re.compile(r"\b([A-Z]{2,5})-[0-9]{1,6}\b")
TOOL_NAME_RE = re.compile(r"\b(Claude|Anthropic|ChatGPT|GPT|Copilot|Sonnet|Opus|Fable|Haiku)\b")
# devscout is a tool built to work with the Claude Code host product, so that
# exact two-word phrase is allowed; the tool name alone, or any other name
# above, stays banned.
CLAUDE_CODE_RE = re.compile(r"\bClaude Code\b")
TEST_NARRATION_RE = re.compile(r"\bprobes?\b|\bexercises the\b")

# Roadmap decomposition vocabulary ("Unit A3 item 4", "item 7") does not
# belong in a shipped comment: it names a plan, not an invariant, and rots
# the moment the plan is renumbered. Deliberately narrow to the Unit/item
# family rather than a generic step|wave|phase|unit|item ban: step N names
# devscout's own resolver ladder ("step 1.5", "step 0b", "step 3") and is
# domain vocabulary, not a plan label, so it must not match here.
PLAN_LABEL_RE = re.compile(
    r"\bUnit [A-Z][0-9]+\b"          # Unit <L><N>, e.g. "Unit A3"
    r"|\b[A-Z][0-9]+ item [0-9]+\b"  # <L><N> item <N>, e.g. "A3 item 4"
    r"|\bitem [0-9]+\b"              # bare item <N>, e.g. "item 7"
)

# ---- the commit-message classes -------------------------------------------

COMMIT_TRAILER_RE = re.compile(r"\b(Co-Authored-By|Claude-Session|Generated with)\b")
SIGNOFF_TRAILER_RE = re.compile(r"^Signed-off-by:\s*\S", re.MULTILINE)


def has_tool_name(line):
    """True if `line` names a banned tool/model, ignoring any occurrence of
    the allowed phrase "Claude Code"."""
    return bool(TOOL_NAME_RE.search(CLAUDE_CODE_RE.sub("", line)))


def is_under_fixtures(path):
    return "fixtures" in Path(path).parts


def is_under_test_fixtures(path):
    """True for any path with a tests/data/ component, at any depth -- this
    is where a scanner's own hook-mode fixtures live, and stays out of scan
    mode so their realistic-looking violations do not self-trip the scan."""
    parts = Path(path).parts
    return any(parts[i] == "tests" and parts[i + 1] == "data" for i in range(len(parts) - 1))


def find_repo_root():
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            check=True,
            capture_output=True,
            text=True,
        )
        root = out.stdout.strip()
        return root or "."
    except Exception:
        return "."


def load_allow_prefixes(repo_root):
    """Read `.claude/hooks/comment-hygiene.allow` (one prefix per line, `#`
    comments allowed) if the repo has one, else the empty set."""
    allow_path = Path(repo_root) / ".claude" / "hooks" / "comment-hygiene.allow"
    if not allow_path.is_file():
        return set()
    try:
        content = allow_path.read_text(encoding="utf-8")
    except Exception:
        return set()
    prefixes = set()
    for line in content.splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            prefixes.add(line.upper())
    return prefixes


def effective_allowlist(repo_root):
    return STANDARDS_ALLOWLIST | load_allow_prefixes(repo_root)


def classify_comment_line(line, is_fixture_path, allowlist):
    """Return a human-readable class name for the first matching class in a
    comment line, or None if the line is clean. `is_fixture_path` scopes the
    test-intent-narration class to fixture sources, where "probe" is a
    narrated intent rather than resolver vocabulary. `allowlist` is the
    tracker-id STANDARDS_ALLOWLIST widened by any repo `.allow` file."""
    if CASE_NOTE_RE.match(line):
        return "narrative case note"
    if SOURCE_CITATION_RE.search(line):
        return "source-line citation"
    for m in TRACKER_ID_RE.finditer(line):
        if m.group(1) not in allowlist:
            return "tracker id"
    if has_tool_name(line):
        return "tool or model name"
    if is_fixture_path and TEST_NARRATION_RE.search(line):
        return "test-intent narration"
    if PLAN_LABEL_RE.search(line):
        return "plan label"
    return None


def classify_commit_line(line, allowlist):
    """Same idea as classify_comment_line, for a line of commit-message (or
    git-commit command) text."""
    if COMMIT_TRAILER_RE.search(line):
        return "attribution trailer"
    for m in TRACKER_ID_RE.finditer(line):
        if m.group(1) not in allowlist:
            return "tracker id"
    if has_tool_name(line):
        return "tool or model name"
    return None


def file_extension(file_path):
    if not file_path:
        return ""
    return Path(file_path).suffix.lstrip(".").lower()


def is_comment_line(line, ext):
    if COMMENT_LINE_RE.match(line):
        return True
    if ext in HASH_COMMENT_EXTENSIONS and HASH_LINE_RE.match(line):
        return True
    return False


def trim(text, limit=120):
    text = text.strip()
    return text[:limit]


def in_scan_scope(file_path):
    """True if `--scan` would read `file_path`, so hook mode can refuse exactly
    what the gate refuses and nothing more. A hook stricter than the gate stops
    work that would have passed, and it reads as a policy verdict rather than
    the false positive it is -- which teaches people to switch it off.

    No tracked-ness test here. Prose is already out of scope by extension
    (SCANNABLE_EXTENSIONS never includes .md), so tracked-ness added nothing
    for the prose case it was meant to cover. Its only real effect was
    exempting untracked source files -- and a brand-new .rs file is exactly
    the case that must stay in scope: it is about to become tracked, and an
    editor-time refusal is the earliest point anyone can catch it, before it
    ever reaches `git add`."""
    if not file_path:
        return False
    if should_ignore_for_scan(file_path, self_file_path()):
        return False
    return file_extension(file_path) in SCANNABLE_EXTENSIONS


def find_comment_hit(text, file_path, allowlist):
    """Scan `text` (an Edit new_string or a Write content) for the first
    comment line that trips one of the comment-content classes. A path outside
    the gate's scope is never read at all."""
    if not in_scan_scope(file_path):
        return None
    ext = file_extension(file_path)
    is_fixture_path = is_under_fixtures(file_path) if file_path else False
    for line_no, line in enumerate(text.splitlines(), start=1):
        if not is_comment_line(line, ext):
            continue
        cls = classify_comment_line(line, is_fixture_path, allowlist)
        if cls:
            return {"class": cls, "line_no": line_no, "line": line}
    return None


def find_commit_hit(command_text, allowlist):
    """Scan a `git commit` Bash command (message text, wherever it lives in
    the command -- after -m or inside a heredoc) for a banned pattern."""
    for line_no, line in enumerate(command_text.splitlines(), start=1):
        cls = classify_commit_line(line, allowlist)
        if cls:
            return {"class": cls, "line_no": line_no, "line": line}
    return None


def build_reason(hit):
    return (
        f"comment-hygiene: {hit['class']} at line {hit['line_no']}: "
        f"{trim(hit['line'])}. Put test intent in the test or the fixture "
        f"README; write the why, not the what."
    )


def emit_deny(event_name, hit):
    reason = build_reason(hit)
    payload = {
        "hookSpecificOutput": {
            "hookEventName": event_name or "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }
    print(json.dumps(payload))
    print(reason, file=sys.stderr)


# ---- hook mode -------------------------------------------------------------


def hook_mode():
    try:
        raw = sys.stdin.read()
        data = json.loads(raw)
    except Exception:
        return 0

    allowlist = effective_allowlist(find_repo_root())

    try:
        return _hook_mode_inner(data, allowlist)
    except Exception:
        return 0


def _hook_mode_inner(data, allowlist):
    if not isinstance(data, dict):
        return 0

    tool_name = data.get("tool_name")
    tool_input = data.get("tool_input") or {}
    event_name = data.get("hook_event_name")

    if not isinstance(tool_input, dict):
        return 0

    if tool_name in ("Edit", "Write"):
        file_path = tool_input.get("file_path", "")
        if tool_name == "Edit":
            text = tool_input.get("new_string", "")
        else:
            text = tool_input.get("content", "")
        if not isinstance(text, str) or not text:
            return 0
        hit = find_comment_hit(text, file_path, allowlist)
        if hit:
            emit_deny(event_name, hit)
            return 2
        return 0

    if tool_name == "Bash":
        command = tool_input.get("command", "")
        if not isinstance(command, str) or "git commit" not in command:
            return 0
        hit = find_commit_hit(command, allowlist)
        if hit:
            emit_deny(event_name, hit)
            return 2
        return 0

    return 0


# ---- scan mode --------------------------------------------------------------


def tracked_files(paths):
    try:
        out = subprocess.run(
            ["git", "ls-files", "--"] + list(paths),
            check=True,
            capture_output=True,
            text=True,
        )
    except Exception as exc:
        print(f"comment-hygiene: git ls-files failed: {exc}", file=sys.stderr)
        return []
    return [p for p in out.stdout.splitlines() if p]


def self_file_path():
    try:
        return Path(__file__).resolve()
    except Exception:
        return None


def should_ignore_for_scan(path, self_path):
    """The always-skipped part of a full-tree scan: this script's own file
    (wherever it is vendored, its docstrings and regex literals carry
    example violations on purpose) and any tests/data/ fixture corpus."""
    if self_path is not None:
        try:
            if Path(path).resolve() == self_path:
                return True
        except OSError:
            pass
    return is_under_test_fixtures(path)


def scan_mode(paths):
    repo_root = find_repo_root()
    allowlist = effective_allowlist(repo_root)
    self_path = self_file_path()

    hits = []
    for path in tracked_files(paths):
        if should_ignore_for_scan(path, self_path):
            continue
        ext = file_extension(path)
        if ext not in SCANNABLE_EXTENSIONS:
            continue
        p = Path(path)
        try:
            content = p.read_text(encoding="utf-8")
        except Exception:
            continue
        is_fixture_path = is_under_fixtures(path)
        for line_no, line in enumerate(content.splitlines(), start=1):
            if not is_comment_line(line, ext):
                continue
            cls = classify_comment_line(line, is_fixture_path, allowlist)
            if cls:
                hits.append(f"{path}:{line_no}: {cls}: {line.strip()}")

    for hit in hits:
        print(hit)
    return 1 if hits else 0


def scan_commits_mode(commit_range):
    allowlist = effective_allowlist(find_repo_root())
    try:
        out = subprocess.run(
            ["git", "log", "--format=%B", commit_range],
            check=True,
            capture_output=True,
            text=True,
        )
    except Exception as exc:
        print(f"comment-hygiene: git log failed: {exc}", file=sys.stderr)
        return 1

    hits = []
    for line_no, line in enumerate(out.stdout.splitlines(), start=1):
        cls = classify_commit_line(line, allowlist)
        if cls:
            hits.append(f"commit-log:{line_no}: {cls}: {line.strip()}")

    for hit in hits:
        print(hit)
    return 1 if hits else 0


def require_signoff_mode(commit_range):
    """Check every commit in `commit_range` carries a Signed-off-by trailer.
    Unlike scan_commits_mode this needs commit boundaries, not just lines, so
    it walks one NUL/STX-delimited record per commit rather than the raw
    line stream."""
    try:
        out = subprocess.run(
            ["git", "log", "--format=%H%x00%B%x02", commit_range],
            check=True,
            capture_output=True,
            text=True,
        )
    except Exception as exc:
        print(f"comment-hygiene: git log failed: {exc}", file=sys.stderr)
        return 1

    hits = []
    for record in out.stdout.split("\x02"):
        record = record.strip("\n")
        if not record:
            continue
        sha, _, body = record.partition("\x00")
        if not SIGNOFF_TRAILER_RE.search(body):
            short_sha = sha[:12] if sha else "?"
            hits.append(f"{short_sha}: missing Signed-off-by trailer")

    for hit in hits:
        print(hit)
    return 1 if hits else 0


# ---- selfcheck mode ---------------------------------------------------------


SHA_FIELD_RE = re.compile(r'(HYGIENE_SHA256\s*=\s*")[0-9a-f]{64}(")')
SHA_PLACEHOLDER = "0" * 64


def compute_self_hash(path):
    text = path.read_text(encoding="utf-8")
    normalized = SHA_FIELD_RE.sub(r"\g<1>" + SHA_PLACEHOLDER + r"\g<2>", text)
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest()


def selfcheck_mode():
    path = self_file_path()
    if path is None:
        print("comment-hygiene: selfcheck: could not resolve this file's own path", file=sys.stderr)
        return 1
    try:
        actual = compute_self_hash(path)
    except Exception as exc:
        print(f"comment-hygiene: selfcheck: could not read {path}: {exc}", file=sys.stderr)
        return 1
    if actual != HYGIENE_SHA256:
        print(
            f"comment-hygiene: selfcheck FAILED: version {HYGIENE_VERSION} recorded "
            f"sha256 {HYGIENE_SHA256} does not match computed {actual} -- local edit "
            "without a refreshed hash, or vendoring drift",
            file=sys.stderr,
        )
        return 1
    print(f"comment-hygiene: selfcheck OK: version {HYGIENE_VERSION} sha256 {HYGIENE_SHA256}")
    return 0


def main(argv):
    if argv and argv[0] == "--scan":
        paths = argv[1:] if len(argv) > 1 else DEFAULT_SCAN_PATHS
        return scan_mode(paths)

    if argv and argv[0] == "--scan-commits":
        if len(argv) < 2:
            print("usage: comment-hygiene.py --scan-commits <range>", file=sys.stderr)
            return 2
        return scan_commits_mode(argv[1])

    if argv and argv[0] == "--require-signoff":
        if len(argv) < 2:
            print("usage: comment-hygiene.py --require-signoff <range>", file=sys.stderr)
            return 2
        return require_signoff_mode(argv[1])

    if argv and argv[0] == "--selfcheck":
        return selfcheck_mode()

    return hook_mode()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
