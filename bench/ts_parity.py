"""TypeScript reference parity bench: seed rule, truth, guards, arm runner and scorer.

Standard library only. Every subcommand reads and writes files, so each step can be rerun
and every figure traced back to a stored arm output. See
docs/benchmarks/methodology.md#typescript-reference-parity for the protocol.
"""

import hashlib
import json
import os
import pwd
import re
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path

PEER_PACKAGE = "code-review-graph"
PEER_MODULE = "code_review_graph"
PEER_VERSION = "2.3.7"
# Upstream ships these; a build that lacks them is not the release this bench measures.
PEER_IDENTITY_FILES = ("skills.py", "uninstall.py")
PEER_IDENTITY_DIRS = ("eval",)
PEER_VERBS = ("build", "status", "query", "impact")
DEVSCOUT_VERBS = ("init", "refs", "tests", "impact")
ARM_VERBS = {"devscout-base": DEVSCOUT_VERBS, "devscout-head": DEVSCOUT_VERBS, "crg": PEER_VERBS}
# Pre-registered files each step reads; the step re-checks them against the manifest itself.
RUN_INPUTS = ("templates.tsv", "seeds.json")
# Exit codes each arm answers with: devscout exits 3 for a resolved seed with an empty
# answer (docs/answer-contract.md); any other code is a failed run that answers nothing.
ANSWER_EXITS = {"devscout-base": (0, 3), "devscout-head": (0, 3), "crg": (0,)}
# The peer exits 0 on its own errors, so its payload status decides; not_found is its answer
# for a name it holds no node for.
ANSWER_STATUSES = {"crg": ("ok", "not_found")}
SCORE_INPUTS = ("seeds.json", "truth-sealed.json", "truth-raw.json", "templates.tsv")
MANIFEST_DIR = Path(__file__).resolve().parent / "truth" / "typescript"
SHEBANG_RE = re.compile(r"^#!\s*(\S+)")
# Installers write this /bin/sh trampoline when the interpreter path is too long for a shebang.
TRAMPOLINE_RE = re.compile(r"^'{3}exec' '([^']+)'", re.MULTILINE)

PROVIDER_KEY_RE = re.compile(
    r"(API_?KEY|TOKEN|SECRET|PASSWORD|OPENAI|GEMINI|GOOGLE|MINIMAX|ANTHROPIC|VOYAGE|COHERE|MISTRAL)",
    re.IGNORECASE,
)
# Only these uv settings may reach the install; anything else could add an index or a cache.
ALLOWED_UV_ENV = {"UV_CACHE_DIR", "UV_NO_CONFIG", "UV_PYTHON_DOWNLOADS"}
PIN_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*==[A-Za-z0-9.+!_-]+$")
HASH_RE = re.compile(r"^--hash=sha256:[0-9a-f]{64}$")

NAME_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
WORD_RE = re.compile(r"\w+")
TS_EXT = (".ts", ".tsx")
SOURCE_EXT = (".ts", ".tsx", ".js", ".jsx")
TEST_SUFFIXES = (".spec.ts", ".spec.tsx", ".test.ts", ".test.tsx")

EXPORT_RE = re.compile(
    r"^export\s+(?:declare\s+)?(?:default\s+)?(?:abstract\s+)?(?:async\s+)?"
    r"(function\s*\*?|class|interface|type|const\s+enum|enum|namespace|module|const|let|var)\s+"
    r"([A-Za-z_$][\w$]*)(.*)$"
)
FUNCTION_INIT_RE = re.compile(
    r"^\s*(?::[^=]*)?=\s*(?:async\s+)?(?:function\b|\(|<|[A-Za-z_$][\w$]*\s*=>)"
)
CALL_INIT_RE = re.compile(r"^\s*(?::[^=]*)?=\s*[A-Za-z_$][\w$.]*\s*(?:<[^>]*>)?\s*\(")
HOOK_RE = re.compile(r"^use[A-Z]")


# ---- guards ----------------------------------------------------------------------------


def real_home():
    return os.path.realpath(pwd.getpwuid(os.getuid()).pw_dir)


def canonical(path):
    return os.path.realpath(path)


def inside(path, root):
    path, root = os.path.realpath(path), os.path.realpath(root)
    return path == root or path.startswith(root.rstrip(os.sep) + os.sep)


def env_problems(env, home_of_user):
    problems = []
    home = env.get("HOME", "")
    if not home:
        problems.append("HOME is unset")
    elif not os.path.isabs(home):
        problems.append(f"HOME is not an absolute path: {home}")
    elif inside(home, home_of_user):
        problems.append("HOME is the real home directory or inside it")
    for name in sorted(env):
        if PROVIDER_KEY_RE.search(name):
            problems.append(f"environment carries a credential-like name: {name}")
        if (name.startswith("UV_") and name not in ALLOWED_UV_ENV) or name.startswith("PIP_"):
            problems.append(f"environment carries an installer setting: {name}")
    return problems


def logical_requirements(text):
    lines, current = [], ""
    for raw in text.splitlines():
        line = "" if raw.lstrip().startswith("#") else raw.split(" #", 1)[0].strip()
        if not line:
            if current:
                lines.append(current)
                current = ""
            continue
        continued = line.endswith("\\")
        current = f"{current} {line.rstrip(chr(92)).strip()}".strip()
        if not continued:
            lines.append(current)
            current = ""
    if current:
        lines.append(current)
    return lines


def requirement_problems(text):
    problems = []
    requirements = logical_requirements(text)
    if not requirements:
        problems.append("requirements file lists nothing")
    for requirement in requirements:
        head, _, marker = requirement.partition(";")
        tokens = head.split()
        if "://" in requirement or "git+" in requirement or "@" in head or "[" in head:
            problems.append(f"not a plain index pin: {requirement}")
            continue
        if not tokens or not PIN_RE.match(tokens[0]):
            problems.append(f"not an exact == pin: {requirement}")
            continue
        options = [token for token in tokens[1:] + marker.split() if token.startswith("-")]
        if not options:
            problems.append(f"no hash for {tokens[0]}")
        for option in options:
            if not HASH_RE.match(option):
                problems.append(f"option other than a sha256 hash: {option}")
    return problems


def cache_problems(cache_dir):
    if not cache_dir:
        return ["UV_CACHE_DIR is unset"]
    path = Path(cache_dir)
    if path.exists() and any(path.iterdir()):
        return [f"UV_CACHE_DIR is not empty: {cache_dir}"]
    return []


def identity_problems(package_dir, version):
    problems = []
    root = Path(package_dir)
    for name in PEER_IDENTITY_FILES:
        if not (root / name).is_file():
            problems.append(f"installed package lacks {name}")
    for name in PEER_IDENTITY_DIRS:
        if not (root / name).is_dir():
            problems.append(f"installed package lacks {name}/")
    if version != PEER_VERSION:
        problems.append(f"installed version is {version}, expected {PEER_VERSION}")
    return problems


def entry_interpreter(head):
    """The interpreter an entry script runs, from its shebang or its /bin/sh trampoline."""
    match = SHEBANG_RE.match(head.split("\n", 1)[0])
    if not match:
        return None
    if match.group(1) == "/bin/sh":
        trampoline = TRAMPOLINE_RE.search(head)
        return trampoline.group(1) if trampoline else None
    return match.group(1)


def peer_binary_problems(binary):
    """The binary an arm run is given must be the entry script of a virtual environment whose
    installed peer passes the identity check, whatever the install step reported."""
    path = Path(canonical(binary))
    venv = path.parent.parent
    if path.name != PEER_PACKAGE or path.parent.name != "bin" or not (venv / "pyvenv.cfg").is_file():
        return [f"{binary} is not the {PEER_PACKAGE} entry script of a virtual environment"]
    try:
        head = path.read_bytes()[:1024].decode("utf-8", "replace")
    except OSError as error:
        return [f"cannot read {binary}: {error}"]
    interpreter = entry_interpreter(head)
    # The interpreter link itself resolves to the base Python, so its directory is what ties
    # the script to this environment, however the install spelled the path.
    if (interpreter is None or not os.path.isabs(interpreter)
            or not os.path.basename(interpreter).startswith("python")
            or canonical(os.path.dirname(interpreter)) != str(venv / "bin")):
        return [f"{binary} does not run the interpreter of its own virtual environment"]
    try:
        package_dir, version = installed_peer(venv)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        return [f"cannot read the installed {PEER_PACKAGE}: {error}"]
    return identity_problems(package_dir, version)


def manifest_problems(bench_dir, names):
    bench = Path(bench_dir)
    try:
        recorded = json.loads((bench / "manifest.json").read_text(encoding="utf-8"))["files"]
    except (OSError, ValueError, KeyError) as error:
        return [f"cannot read the manifest: {error}"]
    problems = []
    for name in names:
        if name not in recorded:
            problems.append(f"{name} has no manifest entry")
            continue
        try:
            actual = sha256_file(bench / name)
        except OSError as error:
            problems.append(f"cannot read {name}: {error}")
            continue
        if actual != recorded[name]:
            problems.append(f"{name} hash {actual} differs from the manifest {recorded[name]}")
    return problems


def registered_file_problems(manifest_dir, path):
    """A step that consumes a registered file re-checks it against the manifest entry that names
    the same file, whatever path it was handed."""
    manifest_dir = Path(manifest_dir)
    try:
        recorded = read_json(manifest_dir / "manifest.json")["files"]
    except (OSError, ValueError, KeyError) as error:
        return [f"cannot read the manifest: {error}"]
    target = canonical(path)
    for name, digest in recorded.items():
        if canonical(manifest_dir / name) == target:
            actual = sha256_file(target)
            return [] if actual == digest else [f"{path} hash {actual} differs from the manifest {digest}"]
    return [f"{path} has no manifest entry"]


def is_devscout_checkout(directory):
    manifest = Path(directory) / "Cargo.toml"
    try:
        return 'name = "devscout-rs"' in manifest.read_text(encoding="utf-8")
    except OSError:
        return False


def enclosing_checkout(path):
    path = Path(canonical(path))
    for directory in [path, *path.parents]:
        if is_devscout_checkout(directory):
            return directory
    return None


def place_problems(label, path, home_of_user, scratch_root=None):
    """A bench path must be absolute, outside the real home and every devscout-rs checkout,
    and, when a scratch root is given, inside it."""
    if not os.path.isabs(path):
        return [f"{label} is not an absolute path: {path}"]
    problems = []
    if scratch_root is not None and not inside(path, scratch_root):
        problems.append(f"{label} is outside the bench scratch root")
    if inside(path, home_of_user):
        problems.append(f"{label} is inside the real home directory")
    checkout = enclosing_checkout(path)
    if checkout is not None:
        problems.append(f"{label} sits inside a devscout-rs checkout: {checkout}")
    return problems


def install_path_problems(cache_dir, venv, home_of_user):
    return (place_problems("UV_CACHE_DIR", cache_dir, home_of_user)
            + place_problems("the peer virtual environment", canonical(venv), home_of_user))


def corpus_changes(corpus):
    """Every changed, untracked or ignored path of the corpus copy, one `git status` line each."""
    run = subprocess.run(["git", "-C", str(corpus), "status", "--porcelain", "--ignored", "--untracked-files=all"],
                         capture_output=True, text=True)
    if run.returncode != 0:
        return [f"git status failed: {run.stderr.strip()}"]
    return [line for line in run.stdout.splitlines() if line]


def corpus_problems(corpus, pin, head, scratch_root, home_of_user, changes=()):
    """The corpus must be a copy of the pin that no arm has indexed or edited, bound to the
    scratch root and outside the real home."""
    problems = []
    if head != pin:
        problems.append(f"corpus HEAD {head} is not the pin {pin}")
    if changes:
        problems.append(f"the corpus copy is not a pristine clone of the pin: {len(changes)} changed, "
                        f"untracked or ignored paths, first {changes[0]}")
    return problems + place_problems("the corpus", canonical(corpus), home_of_user, scratch_root)


def arm_env_problems(arm, env, scratch_root, home_of_user):
    problems = env_problems(env, home_of_user)
    problems += place_problems("the bench scratch root", scratch_root, home_of_user)
    state_keys = ["HOME"] + (["CRG_DATA_DIR"] if arm == "crg" else ["SCOUT_REGISTRY", "SCOUT_CONTENT_DB"])
    for key in state_keys:
        value = env.get(key, "")
        if not value:
            problems.append(f"{key} is unset")
        else:
            problems += place_problems(key, value, home_of_user, scratch_root)
    return problems


def template_problems(templates, manifest_sha, template_path):
    problems = []
    actual = sha256_file(template_path)
    if actual != manifest_sha:
        problems.append(f"templates hash {actual} differs from the manifest {manifest_sha}")
    for row in templates:
        allowed = ARM_VERBS.get(row["arm"])
        if allowed is None:
            problems.append(f"unknown arm {row['arm']}")
            continue
        verb = row["argv"][1] if len(row["argv"]) > 1 else ""
        if verb not in allowed:
            problems.append(f"{row['arm']} template uses a verb outside {', '.join(allowed)}: {verb}")
    return problems


def refuse(problems):
    for problem in problems:
        print(f"refused: {problem}", file=sys.stderr)
    return 1 if problems else 0


# ---- corpus reading --------------------------------------------------------------------


def is_test_path(path):
    if not path.endswith(TS_EXT):
        return False
    if path.endswith(TEST_SUFFIXES):
        return True
    return "__tests__" in path.split("/")[:-1]


def exported_declarations(path, text):
    """Top-level exported declarations, one dict per name, read from the source text."""
    found, seen = [], set()
    for line in text.splitlines():
        match = EXPORT_RE.match(line)
        if not match:
            continue
        keyword, name, rest = match.group(1), match.group(2), match.group(3)
        keyword = re.sub(r"\s+", " ", keyword).replace(" *", "").replace("*", "")
        if name in seen:
            continue
        seen.add(name)
        found.append({"name": name, "file": path, "keyword": keyword,
                      "functionLike": keyword in ("const", "let", "var") and bool(FUNCTION_INIT_RE.match(rest)),
                      "callInit": keyword in ("const", "let", "var") and bool(CALL_INIT_RE.match(rest))})
    return found


def slot_of(declaration):
    """5 hook or component, 4 interface or type, 1 function or class, None otherwise."""
    name, keyword, path = declaration["name"], declaration["keyword"], declaration["file"]
    is_function = keyword == "function" or declaration["functionLike"]
    if HOOK_RE.match(name) and is_function:
        return 5
    if path.endswith(".tsx") and name[:1].isupper() and (is_function or declaration["callInit"]):
        return 5
    if keyword in ("interface", "type"):
        return 4
    if keyword == "class" or is_function:
        return 1
    return None


def read_text(root, path):
    try:
        return (Path(root) / path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def tracked_files(corpus):
    out = subprocess.run(["git", "-C", str(corpus), "ls-files", "-z"], check=True,
                         capture_output=True).stdout.decode("utf-8", "replace")
    return sorted(path for path in out.split("\0") if path)


class Corpus:
    def __init__(self, root, files, texts):
        self.root = root
        self.files = files
        self.texts = texts
        self.ts_files = [path for path in files if path.endswith(TS_EXT)]
        self.words = {path: set(WORD_RE.findall(texts[path])) for path in self.ts_files}

    @classmethod
    def load(cls, root):
        files = [path for path in tracked_files(root) if path.endswith(SOURCE_EXT)]
        texts = {path: read_text(root, path) for path in files}
        return cls(root, files, texts)

    def word_files(self, name):
        return sorted(path for path in self.ts_files if name in self.words[path])

    def declarations(self):
        found = []
        for path in self.files:
            found.extend(exported_declarations(path, self.texts[path]))
        return found


def seed_rank(pin, name):
    return hashlib.sha256(f"{pin}:{name}".encode()).hexdigest()


def select_seeds(corpus, pin, count_files=None):
    """Apply the registered seed rule; count_files defaults to the corpus word index."""
    count_files = count_files or corpus.word_files
    declarations = corpus.declarations()
    declaring = defaultdict(set)
    for declaration in declarations:
        declaring[declaration["name"]].add(declaration["file"])
    pools = {1: [], 4: [], 5: []}
    rejected = defaultdict(int)
    for declaration in declarations:
        name, path = declaration["name"], declaration["file"]
        slot = slot_of(declaration)
        if slot is None or not path.endswith(TS_EXT):
            continue
        reason = None
        if path.endswith(".d.ts") or is_test_path(path):
            reason = "declared in a test or declaration file"
        elif not NAME_RE.match(name):
            reason = "name outside [A-Za-z_][A-Za-z0-9_]*"
        elif len(declaring[name]) != 1:
            reason = "name declared by more than one file"
        else:
            files = count_files(name)
            if not 5 <= len(files) <= 200:
                reason = "word-boundary file count outside 5..200"
            elif not any(is_test_path(other) and other != path for other in files):
                reason = "no test-path file mentions it"
        if reason:
            rejected[reason] += 1
            continue
        pools[slot].append(dict(declaration, rank=seed_rank(pin, name)))
    for pool in pools.values():
        pool.sort(key=lambda item: item["rank"])
    picks = pools[1][:3] + pools[4][:1] + pools[5][:1]
    seeds = []
    for number, pick in enumerate(picks, start=1):
        seeds.append({"slot": number, "name": pick["name"], "file": pick["file"],
                      "keyword": pick["keyword"], "rank": pick["rank"]})
    return {
        "pin": pin,
        "candidates": {"functions-and-classes": len(pools[1]), "interfaces-and-types": len(pools[4]),
                       "hooks-and-components": len(pools[5])},
        "rejected": dict(sorted(rejected.items())),
        "pools": {str(slot): [item["name"] for item in pool] for slot, pool in pools.items()},
        "seeds": seeds,
        "declaringFiles": sorted({seed["file"] for seed in seeds if seed["slot"] in (1, 4)}),
        "diffFiles": sorted({seed["file"] for seed in seeds if seed["slot"] in (2, 3, 5)}),
    }


# ---- ripgrep ---------------------------------------------------------------------------


def rg_version(rg):
    out = subprocess.run([rg, "--version"], check=True, capture_output=True, text=True).stdout
    first = out.splitlines()[0] if out else ""
    if not first.startswith("ripgrep "):
        raise SystemExit(f"{rg} is not ripgrep: {first!r}")
    return first


def rg_lines(rg, corpus, files, name):
    """{file: [line numbers]} for tracked files matching \\b<name>\\b, read by ripgrep."""
    found = defaultdict(list)
    for start in range(0, len(files), 2000):
        chunk = files[start:start + 2000]
        run = subprocess.run([rg, "--no-config", "-n", "--no-heading", "--with-filename", "-e",
                              rf"\b{name}\b", "--", *chunk], cwd=corpus, capture_output=True, text=True)
        if run.returncode not in (0, 1):
            raise SystemExit(f"ripgrep failed on {name}: {run.stderr.strip()}")
        for line in run.stdout.splitlines():
            path, number, _ = line.split(":", 2)
            found[path].append(int(number))
    return {path: sorted(numbers) for path, numbers in sorted(found.items())}


# ---- truth -----------------------------------------------------------------------------


def target_lists(seeds):
    targets = {f"declaring:{path}": [path] for path in seeds["declaringFiles"]}
    targets["diff"] = list(seeds["diffFiles"])
    return targets


def build_truth(corpus, seeds, lines_for):
    """Raw truth: every seed, plus every exported name the impact target files declare."""
    declared_in = defaultdict(set)
    for seed in seeds["seeds"]:
        declared_in[seed["name"]].add(seed["file"])
    targets = target_lists(seeds)
    target_names = {}
    for key, files in targets.items():
        declared = set()
        for path in files:
            for declaration in exported_declarations(path, corpus.texts.get(path, "")):
                if NAME_RE.match(declaration["name"]):
                    declared.add(declaration["name"])
                    declared_in[declaration["name"]].add(path)
        target_names[key] = sorted(declared)
    truth = {"pin": seeds["pin"], "names": {}, "targets": {}}
    for name in sorted(declared_in):
        excluded = declared_in[name]
        truth["names"][name] = {"declaredIn": sorted(excluded),
                                "files": {path: lines for path, lines in lines_for(name).items()
                                          if path not in excluded}}
    for key, files in targets.items():
        truth["targets"][key] = {"files": files, "names": target_names[key]}
    return truth


def lex_regions(text):
    """Per character: 'c' code, 'm' comment, 's' string. Template expressions count as code."""
    kinds = []
    i, n = 0, len(text)
    stack = []
    state = "code"
    while i < n:
        ch, nxt = text[i], text[i + 1] if i + 1 < n else ""
        if state == "code":
            if ch == "/" and nxt == "/":
                end = text.find("\n", i)
                end = n if end < 0 else end
                kinds.extend("m" * (end - i))
                i = end
                continue
            if ch == "/" and nxt == "*":
                end = text.find("*/", i + 2)
                end = n if end < 0 else end + 2
                kinds.extend("m" * (end - i))
                i = end
                continue
            if ch in "'\"":
                j = i + 1
                while j < n and text[j] != ch and text[j] != "\n":
                    j += 2 if text[j] == "\\" else 1
                j = min(j + 1, n)
                kinds.extend("s" * (j - i))
                i = j
                continue
            if ch == "`":
                state = "template"
                kinds.append("s")
                i += 1
                continue
            if stack and ch == "{":
                stack[-1] += 1
            elif stack and ch == "}":
                if stack[-1] == 0:
                    stack.pop()
                    state = "template"
                    kinds.append("s")
                    i += 1
                    continue
                stack[-1] -= 1
            kinds.append("c")
            i += 1
            continue
        if ch == "\\":
            kinds.extend("ss")
            i += 2
            continue
        if ch == "`":
            state = "code"
            kinds.append("s")
            i += 1
            continue
        if ch == "$" and nxt == "{":
            stack.append(0)
            state = "code"
            kinds.extend("ss")
            i += 2
            continue
        kinds.append("s")
        i += 1
    return "".join(kinds[:n])


def own_declaration_re(name):
    return re.compile(
        rf"(?<![\w$.])(?:function\s*\*?|class|interface|type|enum|namespace|const|let|var)\s+{re.escape(name)}\b"
    )


def classify_file(text, name):
    """Mechanical adjudication flag for one file, or None when the name appears in code."""
    regions = lex_regions(text)
    kinds = set()
    for match in re.finditer(rf"\b{re.escape(name)}\b", text):
        kinds.add(regions[match.start()] if match.start() < len(regions) else "c")
    code_only = "".join(ch if kind == "c" else " " for ch, kind in zip(text, regions))
    if own_declaration_re(name).search(code_only):
        return "other declaration"
    if "c" in kinds:
        return None
    if kinds == {"m"}:
        return "comment only"
    return "string only"


def flag_truth(corpus, truth):
    flags = {}
    for name, entry in truth["names"].items():
        for path in entry["files"]:
            flag = classify_file(corpus.texts.get(path) or read_text(corpus.root, path), name)
            if flag:
                flags.setdefault(name, {})[path] = flag
    return flags


def seal_truth(truth, flags, decisions):
    """decisions: {name: {file: {"remove": bool, "reason": str}}} confirming or overriding each flag."""
    sealed = {"pin": truth["pin"], "names": {}, "targets": truth["targets"], "removed": [], "kept": []}
    for name, entry in truth["names"].items():
        kept = []
        for path in entry["files"]:
            flag = flags.get(name, {}).get(path)
            decision = decisions.get(name, {}).get(path)
            if flag and decision is None:
                raise SystemExit(f"flag without a decision: {name} in {path}")
            if decision is not None and flag is None:
                raise SystemExit(f"decision without a flag: {name} in {path}")
            if decision and decision["remove"]:
                sealed["removed"].append({"name": name, "file": path, "reason": decision["reason"]})
                continue
            if decision:
                sealed["kept"].append({"name": name, "file": path, "flag": flag, "reason": decision["reason"]})
            kept.append(path)
        sealed["names"][name] = {"declaredIn": entry["declaredIn"], "files": sorted(kept)}
    return sealed


def impact_truth(sealed, key):
    target = sealed["targets"][key]
    files = set()
    for name in target["names"]:
        files |= set(sealed["names"][name]["files"])
    return sorted(files - set(target["files"]))


# ---- templates and runner --------------------------------------------------------------


def sha256_file(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def read_templates(path):
    rows = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        arm, task, target, argv = line.split("\t")
        rows.append({"arm": arm, "task": task, "target": target, "argv": argv.split(" ")})
    return rows


def seed_id(seed, arm):
    separator = "::" if arm == "crg" else "#"
    return f"{seed['file']}{separator}{seed['name']}"


def expand(argv, values):
    out = []
    for token in argv:
        if token == "{files}":
            out.extend(values["files"])
        else:
            out.append(token.format(**{key: value for key, value in values.items() if key != "files"}))
    return out


def invocations(templates, arm, seeds, binary, corpus):
    targets = target_lists(seeds)
    runs = []
    for row in templates:
        if row["arm"] != arm:
            continue
        base = {"bin": binary, "corpus": corpus}
        if row["target"] == "setup":
            runs.append((row["task"], "setup", expand(row["argv"], base)))
        elif row["target"] == "seed":
            for seed in seeds["seeds"]:
                runs.append((row["task"], seed["name"], expand(row["argv"], dict(base, seed=seed_id(seed, arm)))))
        elif row["target"] == "file":
            for key, files in targets.items():
                for path in files:
                    runs.append((row["task"], f"{key}|{path}", expand(row["argv"], dict(base, file=path))))
        elif row["target"] == "files":
            for key, files in targets.items():
                runs.append((row["task"], key, expand(row["argv"], dict(base, files=files))))
    return runs


def arm_env(arm, scratch):
    scratch = canonical(scratch)
    env = {"PATH": "/usr/bin:/bin", "LANG": "en_US.UTF-8", "HOME": str(Path(scratch) / f"{arm}-home")}
    if arm == "crg":
        env["CRG_DATA_DIR"] = str(Path(scratch) / f"{arm}-data")
    else:
        env["SCOUT_REGISTRY"] = str(Path(scratch) / f"{arm}-home" / "repos.json")
        env["SCOUT_CONTENT_DB"] = str(Path(scratch) / f"{arm}-home" / "content.db")
    return env


def arm_problems(arm, binary, corpus, scratch, out_dir, bench_dir, head, home_of_user, changes=()):
    """Every guard an arm run must pass, judged on the paths and files the run will use."""
    bench = Path(bench_dir)
    problems = manifest_problems(bench, RUN_INPUTS)
    if problems:
        return problems
    manifest = read_json(bench / "manifest.json")
    templates_path = bench / "templates.tsv"
    problems = template_problems(read_templates(templates_path), manifest["files"]["templates.tsv"], templates_path)
    problems += corpus_problems(corpus, read_json(bench / "seeds.json")["pin"], head, scratch, home_of_user, changes)
    problems += arm_env_problems(arm, arm_env(arm, scratch), scratch, home_of_user)
    problems += place_problems("the output directory", canonical(out_dir), home_of_user, scratch)
    if arm == "crg":
        problems += peer_binary_problems(binary)
    return problems


def run_arm(arm, binary, corpus, scratch, out_dir, bench_dir):
    if not os.path.isabs(scratch):
        return refuse([f"the bench scratch root is not an absolute path: {scratch}"])
    # Resolved once: the child runs with the corpus as its working directory, so every path it
    # receives must already be absolute and every guard must judge the path the child will use.
    binary, corpus, scratch, out_dir = canonical(binary), canonical(corpus), canonical(scratch), canonical(out_dir)
    bench = Path(bench_dir)
    head = subprocess.run(["git", "-C", corpus, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    changes = corpus_changes(corpus)
    if refuse(arm_problems(arm, binary, corpus, scratch, out_dir, bench, head, real_home(), changes)):
        return 1
    templates = read_templates(bench / "templates.tsv")
    seeds = read_json(bench / "seeds.json")
    env = arm_env(arm, scratch)
    for key in ("HOME", "CRG_DATA_DIR"):
        if key in env:
            Path(env[key]).mkdir(parents=True, exist_ok=True)
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=False)
    log = []
    for number, (task, target, argv) in enumerate(invocations(templates, arm, seeds, binary, corpus)):
        started = time.monotonic()
        run = subprocess.run(argv, cwd=corpus, env=env, capture_output=True)
        elapsed = time.monotonic() - started
        stem = f"{number:03d}-{task}"
        (out / f"{stem}.stdout").write_bytes(run.stdout)
        (out / f"{stem}.stderr").write_bytes(run.stderr)
        log.append({"n": number, "task": task, "target": target, "argv": argv, "exit": run.returncode,
                    "seconds": round(elapsed, 3), "stdout": f"{stem}.stdout", "stderr": f"{stem}.stderr"})
    (out / "runs.json").write_text(json.dumps({"arm": arm, "env": env, "corpusHead": head, "runs": log},
                                              indent=1) + "\n", encoding="utf-8")
    return 0


# ---- scoring ---------------------------------------------------------------------------


def relative(path, corpus_root):
    """A peer's absolute answer path relative to the corpus root, however either is spelled."""
    if not path or not os.path.isabs(path):
        return path
    root = canonical(corpus_root)
    spelled = os.path.normpath(path)
    resolved = os.path.join(canonical(os.path.dirname(spelled)), os.path.basename(spelled))
    for candidate in (spelled, resolved):
        for prefix in (os.path.normpath(corpus_root), root):
            if candidate.startswith(prefix.rstrip(os.sep) + os.sep):
                return candidate[len(prefix.rstrip(os.sep)) + 1:]
    raise SystemExit(f"answer path outside the corpus root: {path}")


def devscout_files(task, payload):
    if task == "refs":
        return sorted({row["file"] for table in payload.get("inbound", {}).values() for row in table["rows"]})
    return sorted({row["file"] for row in payload.get("rows", [])})


def crg_files(task, payload, corpus_root):
    if task == "impact":
        return sorted({relative(path, corpus_root) for path in payload.get("impacted_files", [])})
    rows = payload.get("results", [])
    return sorted({relative(row.get("file_path") or row.get("file") or "", corpus_root) for row in rows} - {""})


def answered(arm, run, payload):
    if run.get("exit") not in ANSWER_EXITS.get(arm, (0,)) or not isinstance(payload, dict):
        return False
    statuses = ANSWER_STATUSES.get(arm)
    return statuses is None or payload.get("status") in statuses


def load_answers(run_dir, corpus_root=None):
    """{(task, target): files} for one arm's stored run. A failed or unparsable run answers
    nothing: its target maps to None, even when other runs of the same target answered."""
    data = json.loads((Path(run_dir) / "runs.json").read_text(encoding="utf-8"))
    arm = data["arm"]
    answers = defaultdict(set)
    failed = set()
    for run in data["runs"]:
        if run["target"] == "setup":
            continue
        raw = (Path(run_dir) / run["stdout"]).read_text(encoding="utf-8", errors="replace")
        try:
            payload = json.loads(raw)
        except ValueError:
            payload = None
        task = run["task"]
        key = (task, run["target"].split("|", 1)[0])
        if not answered(arm, run, payload):
            failed.add(key)
            continue
        if arm == "crg":
            files = crg_files(task.split("-", 1)[0], payload, corpus_root)
        else:
            files = devscout_files(task.split("-", 1)[0], payload)
        answers[key] |= set(files)
    result = {key: sorted(value) for key, value in answers.items()}
    result.update({key: None for key in failed})
    return result


def answer_keys(templates, arm, seeds):
    """Every (task, target) the registered templates ask the arm to answer."""
    return {(task, target.split("|", 1)[0])
            for task, target, _ in invocations(templates, arm, seeds, "", "") if target != "setup"}


def registered_answers(answers, keys):
    """The answers over the registered keys: a key the record never answered maps to None."""
    merged = {key: None for key in keys}
    merged.update(answers)
    return merged


def run_record_problems(record, arm, templates, seeds, corpus_root):
    """A stored arm record is scored only when it is the named arm's run at the pin and every
    invocation in it is one the registered templates expand to, each run once."""
    problems = []
    if record.get("arm") != arm:
        problems.append(f"the {arm} record is a run of {record.get('arm')}")
    if record.get("corpusHead") != seeds["pin"]:
        problems.append(f"the {arm} record ran on corpus HEAD {record.get('corpusHead')}, not the pin")
    runs = record.get("runs", [])
    binaries = {run["argv"][0] for run in runs if run.get("argv")}
    if len(binaries) > 1:
        problems.append(f"the {arm} record runs more than one binary")
    binary = next(iter(binaries), "")
    expected = {(task, target, tuple(argv))
                for task, target, argv in invocations(templates, arm, seeds, binary, canonical(corpus_root))}
    seen = set()
    for run in runs:
        key = (run.get("task"), run.get("target"), tuple(run.get("argv") or ()))
        if key not in expected:
            problems.append(f"the {arm} record holds an unregistered invocation: {' '.join(key[2])}")
        elif key in seen:
            problems.append(f"the {arm} record runs one invocation twice: {' '.join(key[2])}")
        seen.add(key)
    return problems


def ratio(numerator, denominator):
    return round(numerator / denominator, 4) if denominator else None


def recall_block(answers, task, truth_sets):
    """Recall over the registered seeds; a seed whose answer is missing or failed counts every
    truth file as missed and is marked failed."""
    per_seed, hits, total = {}, 0, 0
    for name, truth in truth_sets.items():
        stored = answers.get((task, name))
        answer = set(stored or [])
        found = answer & set(truth)
        per_seed[name] = {"truth": len(truth), "found": len(found), "recall": ratio(len(found), len(truth)),
                          "missed": sorted(set(truth) - answer), "extra": sorted(answer - set(truth))}
        if stored is None:
            per_seed[name]["failed"] = True
        hits += len(found)
        total += len(truth)
    return {"pooled": ratio(hits, total), "found": hits, "truth": total, "perSeed": per_seed}


def precision_block(answer, truth, queried):
    answer = set(answer) - set(queried)
    universe = {path for path in answer if path.endswith(TS_EXT)}
    outside = sorted(answer - universe)
    right = universe & set(truth)
    return {"rows": len(universe), "correct": len(right), "precision": ratio(len(right), len(universe)),
            "recall": ratio(len(right), len(truth)), "truth": len(truth), "outsideUniverse": outside,
            "extra": sorted(universe - set(truth)), "missed": sorted(set(truth) - universe)}


def score(sealed, seeds, answers):
    refs_truth = {seed["name"]: sealed["names"][seed["name"]]["files"] for seed in seeds["seeds"]}
    tests_truth = {name: [path for path in files if is_test_path(path)] for name, files in refs_truth.items()}
    result = {"refs": recall_block(answers, "refs", refs_truth),
              "tests": recall_block(answers, "tests", tests_truth), "impact": {}}
    if any(task == "refs-importers" for task, _ in answers):
        widened = {(task, name): files for (task, name), files in answers.items() if task == "refs"}
        for (task, name), files in answers.items():
            if task == "refs-importers":
                merged = None if files is None or widened.get(("refs", name), []) is None else \
                    sorted(set(widened.get(("refs", name), [])) | set(files))
                widened[("refs", name)] = merged
        result["refsWithFileImporters"] = recall_block(widened, "refs", refs_truth)
    for key, files in target_lists(seeds).items():
        truth = impact_truth(sealed, key)
        for task in ("impact", "impact-default"):
            # The gated task is scored on every registered target, answered or not.
            if (task, key) in answers or task == "impact":
                stored = answers.get((task, key))
                block = precision_block(stored or [], truth, files)
                if stored is None:
                    block["failed"] = True
                result["impact"].setdefault(task, {})[key] = block
    return result


def gate(scores, seeds):
    """The registered gate, one check per registered metric and target. A check whose answer is
    missing or failed fails; it is never dropped."""
    def recall_met(block):
        failed = any(seed.get("failed") for seed in block["perSeed"].values())
        return not failed and (block["pooled"] or 0) >= 0.9

    impact = scores["impact"].get("impact", {})
    checks = {
        "refs recall >= 0.9": recall_met(scores["refs"]),
        "tests recall >= 0.9": recall_met(scores["tests"]),
    }
    for key in sorted(target_lists(seeds)):
        block = impact.get(key)
        checks[f"impact precision >= 0.5 on {key}"] = (block is not None and not block.get("failed")
                                                       and block["precision"] is not None
                                                       and block["precision"] >= 0.5)
    return checks


def peers_win(arm_scores):
    """One line per metric cell naming the leading arm; ties name every leader."""
    cells = []
    first = next(iter(arm_scores.values()))
    metrics = [("refs recall (pooled)", lambda s: s["refs"]["pooled"]),
               ("tests recall (pooled)", lambda s: s["tests"]["pooled"])]
    for name in sorted(first["refs"]["perSeed"]):
        metrics.append((f"refs recall, {name}", lambda s, n=name: s["refs"]["perSeed"][n]["recall"]))
        metrics.append((f"tests recall, {name}", lambda s, n=name: s["tests"]["perSeed"][n]["recall"]))
    for key in sorted(first["impact"].get("impact", {})):
        metrics.append((f"impact precision depth 1, {key}",
                        lambda s, k=key: s["impact"].get("impact", {}).get(k, {}).get("precision")))
    for label, read in metrics:
        values = {arm: read(scores) for arm, scores in arm_scores.items()}
        known = {arm: value for arm, value in values.items() if value is not None}
        if not known:
            cells.append((label, [], values))
            continue
        best = max(known.values())
        cells.append((label, sorted(arm for arm, value in known.items() if value == best), values))
    return cells


# ---- commands --------------------------------------------------------------------------


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=1, sort_keys=True) + "\n", encoding="utf-8")


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def cmd_peer_preinstall(args):
    requirements_path, venv = args
    requirements = Path(requirements_path).read_text(encoding="utf-8")
    cache_dir = os.environ.get("UV_CACHE_DIR", "")
    problems = env_problems(dict(os.environ), real_home())
    problems += cache_problems(cache_dir)
    if cache_dir:
        problems += install_path_problems(cache_dir, venv, real_home())
    problems += requirement_problems(requirements)
    problems += registered_file_problems(MANIFEST_DIR, requirements_path)
    return refuse(problems)


def installed_peer(venv):
    script = (
        "import importlib.metadata as m, json;"
        f"d = m.distribution({PEER_PACKAGE!r});"
        f"print(json.dumps([str(d.locate_file({PEER_MODULE!r})), d.version]))"
    )
    out = subprocess.run([str(Path(venv) / "bin" / "python"), "-I", "-c", script],
                         check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def cmd_peer_identity(args):
    package_dir, version = installed_peer(args[0])
    problems = identity_problems(package_dir, version)
    if not problems:
        print(f"{PEER_PACKAGE} {version}: upstream identity files present")
    return refuse(problems)


def cmd_seeds(args):
    corpus_dir, rg, pin, out = args
    print(rg_version(rg))
    corpus = Corpus.load(corpus_dir)
    selection = select_seeds(corpus, pin)
    for slot, names in selection["pools"].items():
        for name in names:
            by_rg = sorted(rg_lines(rg, corpus_dir, corpus.ts_files, name))
            if by_rg != corpus.word_files(name):
                raise SystemExit(f"ripgrep and the word index disagree on {name} (pool {slot})")
    if len(selection["seeds"]) < 5:
        raise SystemExit(f"the seed rule yields {len(selection['seeds'])} seeds; five are required")
    write_json(out, selection)
    return 0


def cmd_truth(args):
    corpus_dir, rg, seeds_path, out = args
    print(rg_version(rg))
    corpus = Corpus.load(corpus_dir)
    seeds = read_json(seeds_path)
    truth = build_truth(corpus, seeds, lambda name: rg_lines(rg, corpus_dir, corpus.ts_files, name))
    write_json(out, truth)
    return 0


def cmd_flags(args):
    corpus_dir, truth_path, out = args
    corpus = Corpus.load(corpus_dir)
    write_json(out, flag_truth(corpus, read_json(truth_path)))
    return 0


def cmd_seal(args):
    truth_path, flags_path, decisions_path, out = args
    write_json(out, seal_truth(read_json(truth_path), read_json(flags_path), read_json(decisions_path)))
    return 0


def cmd_manifest(args):
    directory, *names = args
    root = Path(directory)
    write_json(root / "manifest.json", {"files": {name: sha256_file(root / name) for name in sorted(names)}})
    return 0


def cmd_run(args):
    arm, binary, corpus, scratch, out_dir, bench_dir = args
    return run_arm(arm, binary, corpus, scratch, out_dir, bench_dir)


def cmd_score(args):
    bench_dir, corpus_root, out, *runs = args
    bench = Path(bench_dir)
    if refuse(manifest_problems(bench, SCORE_INPUTS)):
        raise SystemExit(1)
    seeds = read_json(bench / "seeds.json")
    sealed = read_json(bench / "truth-sealed.json")
    raw = read_json(bench / "truth-raw.json")
    templates = read_templates(bench / "templates.tsv")
    raw_as_sealed = {"names": {name: {"files": sorted(entry["files"])} for name, entry in raw["names"].items()},
                     "targets": raw["targets"]}
    result = {}
    for spec in runs:
        arm, run_dir = spec.split("=", 1)
        record = read_json(Path(run_dir) / "runs.json")
        if refuse(run_record_problems(record, arm, templates, seeds, corpus_root)):
            raise SystemExit(1)
        answers = registered_answers(load_answers(run_dir, corpus_root), answer_keys(templates, arm, seeds))
        scores = score(sealed, seeds, answers)
        result[arm] = {"sealed": scores, "raw": score(raw_as_sealed, seeds, answers), "gate": gate(scores, seeds)}
    cells = peers_win({arm: block["sealed"] for arm, block in result.items()})
    result["wherePeersWin"] = [{"metric": label, "leaders": leaders, "values": values}
                               for label, leaders, values in cells]
    write_json(out, result)
    return 0


COMMANDS = {
    "peer-preinstall": cmd_peer_preinstall,
    "peer-identity": cmd_peer_identity,
    "seeds": cmd_seeds,
    "truth": cmd_truth,
    "flags": cmd_flags,
    "seal": cmd_seal,
    "manifest": cmd_manifest,
    "run": cmd_run,
    "score": cmd_score,
}


def main(argv):
    if len(argv) < 2 or argv[1] not in COMMANDS:
        print(f"usage: {argv[0]} {{{','.join(sorted(COMMANDS))}}} ...", file=sys.stderr)
        return 2
    return COMMANDS[argv[1]](argv[2:])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
