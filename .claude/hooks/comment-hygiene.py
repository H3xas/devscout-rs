#!/usr/bin/env python3
"""comment-hygiene: keep comments and commit messages carrying a "why", not a
transcript of how they were written.

Three ways to run it:

  1. Hook mode (default, no args). Reads a single Claude Code hook JSON
     payload from stdin. For an Edit or Write tool call, scans the new
     comment text; for a Bash `git commit`, scans the commit message. On a
     match it prints a deny decision to stdout, a one-line reason to
     stderr, and exits 2. Otherwise it exits 0 without printing anything.
     Never raises: any parse problem is treated as "nothing to flag".

  2. `--scan [paths...]` walks git-tracked files (default: src tests
     fixtures docs tools .github), applies the same comment-line checks,
     and prints one `path:line: <class>: <line>` per hit. Exit 1 if any
     hit was found, 0 otherwise. Paths under tests/data/ are skipped (that
     is where this script's own test fixtures live).

  3. `--scan-commits <git-log-range>` applies the commit-message checks to
     every commit message in the range. Exit 1 if any hit was found.

Python 3 standard library only -- this has to run unmodified on macOS and
Ubuntu CI runners with nothing extra installed.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

DEFAULT_SCAN_PATHS = ["src", "tests", "fixtures", "docs", "tools", ".github"]

# Standards / spec prefixes that happen to look like TICKET-123 but are not
# tracker ids (SHA-256, UTF-8, HTTP-01, ...).
STANDARDS_ALLOWLIST = {
    "UTF", "ISO", "SHA", "RFC", "IEEE", "CVE", "IEC", "ECMA", "ASCII",
    "TLS", "SSL", "AES", "RSA", "CRC", "HTTP", "ES", "X", "MD",
}

HASH_COMMENT_EXTENSIONS = {"sh", "py", "yml", "yaml", "toml"}

# A line is "a comment line" if it opens with one of the C-style markers, or
# (only for the extensions above) with '#'.
COMMENT_LINE_RE = re.compile(r"^\s*(?://!|///|//|/\*|\*)")
HASH_LINE_RE = re.compile(r"^\s*#")

# ---- the five comment-content classes ------------------------------------

CASE_NOTE_RE = re.compile(r"^\s*(//|#|\*|/\*)\s*Case\s+[A-Za-z0-9-]+\s*:")
SOURCE_CITATION_RE = re.compile(
    r"\b(src|tests|fixtures|tools)/[A-Za-z0-9_./-]+\.(rs|cs|ts|js|py)\s*:\s*[0-9]+"
)
TRACKER_ID_RE = re.compile(r"\b([A-Z]{2,5})-[0-9]{1,6}\b")
TOOL_NAME_RE = re.compile(r"\b(Claude|Anthropic|ChatGPT|GPT|Copilot|Sonnet|Opus|Fable|Haiku)\b")
TEST_NARRATION_RE = re.compile(r"\bprobes?\b|\bexercises the\b")

# ---- the commit-message classes ------------------------------------------

COMMIT_TRAILER_RE = re.compile(r"\b(Co-Authored-By|Claude-Session|Generated with)\b")


def classify_comment_line(line):
    """Return a human-readable class name for the first matching class in a
    comment line, or None if the line is clean."""
    if CASE_NOTE_RE.match(line):
        return "narrative case note"
    if SOURCE_CITATION_RE.search(line):
        return "source-line citation"
    for m in TRACKER_ID_RE.finditer(line):
        if m.group(1) not in STANDARDS_ALLOWLIST:
            return "tracker id"
    if TOOL_NAME_RE.search(line):
        return "tool or model name"
    if TEST_NARRATION_RE.search(line):
        return "test-intent narration"
    return None


def classify_commit_line(line):
    """Same idea as classify_comment_line, for a line of commit-message (or
    git-commit command) text."""
    if COMMIT_TRAILER_RE.search(line):
        return "attribution trailer"
    for m in TRACKER_ID_RE.finditer(line):
        if m.group(1) not in STANDARDS_ALLOWLIST:
            return "tracker id"
    if TOOL_NAME_RE.search(line):
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


def find_comment_hit(text, file_path):
    """Scan `text` (an Edit new_string or a Write content) for the first
    comment line that trips one of the comment-content classes."""
    ext = file_extension(file_path)
    for line_no, line in enumerate(text.splitlines(), start=1):
        if not is_comment_line(line, ext):
            continue
        cls = classify_comment_line(line)
        if cls:
            return {"class": cls, "line_no": line_no, "line": line}
    return None


def find_commit_hit(command_text):
    """Scan a `git commit` Bash command (message text, wherever it lives in
    the command -- after -m or inside a heredoc) for a banned pattern."""
    for line_no, line in enumerate(command_text.splitlines(), start=1):
        cls = classify_commit_line(line)
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

    try:
        return _hook_mode_inner(data)
    except Exception:
        return 0


def _hook_mode_inner(data):
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
        hit = find_comment_hit(text, file_path)
        if hit:
            emit_deny(event_name, hit)
            return 2
        return 0

    if tool_name == "Bash":
        command = tool_input.get("command", "")
        if not isinstance(command, str) or "git commit" not in command:
            return 0
        hit = find_commit_hit(command)
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


def scan_mode(paths):
    hits = []
    for path in tracked_files(paths):
        if path.startswith("tests/data/"):
            continue
        p = Path(path)
        try:
            content = p.read_text(encoding="utf-8")
        except Exception:
            continue
        ext = file_extension(path)
        for line_no, line in enumerate(content.splitlines(), start=1):
            if not is_comment_line(line, ext):
                continue
            cls = classify_comment_line(line)
            if cls:
                hits.append(f"{path}:{line_no}: {cls}: {line.strip()}")

    for hit in hits:
        print(hit)
    return 1 if hits else 0


def scan_commits_mode(commit_range):
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
        cls = classify_commit_line(line)
        if cls:
            hits.append(f"commit-log:{line_no}: {cls}: {line.strip()}")

    for hit in hits:
        print(hit)
    return 1 if hits else 0


def main(argv):
    if argv and argv[0] == "--scan":
        paths = argv[1:] if len(argv) > 1 else DEFAULT_SCAN_PATHS
        return scan_mode(paths)

    if argv and argv[0] == "--scan-commits":
        if len(argv) < 2:
            print("usage: comment-hygiene.py --scan-commits <range>", file=sys.stderr)
            return 2
        return scan_commits_mode(argv[1])

    return hook_mode()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
