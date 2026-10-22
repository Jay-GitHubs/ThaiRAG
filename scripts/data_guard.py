#!/usr/bin/env python3
"""Data guard: block document-derived test data from entering git.

Policy (README, "Test data policy"): nothing derived from a real document is
tracked — no PDFs or office files, no page images, no OCR text, no question or
gold sets, no evaluation results, no chunk dumps. Tooling is fine; unit-test
fixtures are synthetic or public-domain and listed explicitly.

Checks, per added/modified file:
  1. document / archive / data file types          -> blocked unless allowlisted
  2. images outside UI asset and docs locations    -> blocked unless allowlisted
  3. JSON under scripts/, tests/, data/, eval/      -> blocked unless allowlisted
  4. data directories (eval work/gold/results, backups/, */data/)
  5. file name containing Thai characters (document-like names)
  6. file larger than 2 MB
  7. Thai-heavy text files (>= 1500 Thai chars in data-like files,
     >= 4000 in source files) -> looks like dumped document text
  8. forbidden terms (customer names etc.), never stored in the repo:
       env FORBIDDEN_TERMS (CI secret; comma/newline separated), or a file at
       $DATA_GUARD_TERMS_FILE, ~/.config/thairag/forbidden-terms.txt or
       <repo>/.git/forbidden-terms.txt. Matches are reported by index only.

Allowlist: `.data-guard-allow` at the repo root, one path or glob per line.
Adding to it is a reviewed change.

Usage
  scripts/data_guard.py --staged                 # pre-commit hook
  scripts/data_guard.py --changed-since origin/main   # CI on a PR
  scripts/data_guard.py --all                    # audit the whole tree
  scripts/data_guard.py --files a b c            # explicit paths ('-' = stdin)
Exit 1 when anything is blocked. Stdlib only.
"""
import argparse
import fnmatch
import os
import re
import subprocess
import sys

DOC_EXT = {"pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "tsv", "parquet", "sqlite", "db", "zip", "tar", "gz", "tgz", "7z", "rar"}
IMG_EXT = {"png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic"}
DATA_LIKE_EXT = {"json", "jsonl", "csv", "tsv", "txt", "md", "yaml", "yml", "html", "xml"}
SOURCE_EXT = {"rs", "ts", "tsx", "js", "jsx", "py", "sh", "toml", "sql"}
JSON_DATA_DIRS = ("scripts/", "tests/", "data/", "eval/")
BLOCKED_DIR_RE = re.compile(r"^(scripts/eval/(work|gold|results)/|backups/|(.*/)?data/)")
THAI_RE = re.compile(r"[฀-๿]")
MAX_BYTES = 2_000_000
THAI_DATA_LIMIT, THAI_SOURCE_LIMIT = 1500, 4000


def repo_root():
    try:
        return subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    except Exception:
        return os.getcwd()


def load_allow(root):
    pats = []
    try:
        with open(os.path.join(root, ".data-guard-allow"), encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if line and not line.startswith("#"):
                    pats.append(line)
    except FileNotFoundError:
        pass
    return pats


def allowed(path, pats):
    return any(fnmatch.fnmatchcase(path, p) for p in pats)


def load_terms(root):
    raw = []
    env = os.environ.get("FORBIDDEN_TERMS", "")
    if env:
        raw += re.split(r"[\n,;]+", env)
    for f in (os.environ.get("DATA_GUARD_TERMS_FILE"), os.path.expanduser("~/.config/thairag/forbidden-terms.txt"), os.path.join(root, ".git", "forbidden-terms.txt")):
        if f and os.path.isfile(f):
            with open(f, encoding="utf-8") as fh:
                raw += fh.read().splitlines()
    return [t.strip().lower() for t in raw if t.strip() and not t.strip().startswith("#")]


def git_files(args):
    if args.staged:
        cmd = ["git", "diff", "--cached", "--name-only", "--diff-filter=AMR"]
    elif args.changed_since:
        cmd = ["git", "diff", "--name-only", "--diff-filter=AMR", f"{args.changed_since}...HEAD"]
    elif args.all:
        cmd = ["git", "ls-files"]
    else:
        return None
    return [l for l in subprocess.check_output(cmd, text=True).splitlines() if l.strip()]


def read_bytes(root, path, staged):
    if staged:
        try:
            return subprocess.check_output(["git", "show", f":{path}"])
        except subprocess.CalledProcessError:
            return b""
    try:
        with open(os.path.join(root, path), "rb") as fh:
            return fh.read()
    except OSError:
        return b""


def check(path, data, pats, terms):
    problems = []
    ext = path.rsplit(".", 1)[-1].lower() if "." in os.path.basename(path) else ""
    if allowed(path, pats):
        return problems
    if THAI_RE.search(path):
        problems.append("Thai characters in the file name (document-like name)")
    if BLOCKED_DIR_RE.match(path):
        problems.append("data directory (eval work/gold/results, backups, data/)")
    if ext in DOC_EXT:
        problems.append(f"document/archive file type .{ext}")
    if ext in IMG_EXT:
        problems.append(f"image .{ext} outside UI asset or docs locations")
    if ext in ("json", "jsonl") and path.startswith(JSON_DATA_DIRS):
        problems.append("JSON data under scripts/ tests/ data/ eval/ (question set or results?)")
    if len(data) > MAX_BYTES:
        problems.append(f"{len(data) / 1e6:.1f} MB (limit 2 MB)")
    text = None
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        pass
    if text is not None:
        thai = len(THAI_RE.findall(text))
        limit = THAI_SOURCE_LIMIT if ext in SOURCE_EXT else THAI_DATA_LIMIT
        if thai >= limit:
            problems.append(f"{thai} Thai characters (limit {limit}): looks like dumped document text")
        low = text.lower()
        for i, t in enumerate(terms, 1):
            if t in low or t in path.lower():
                problems.append(f"forbidden term #{i}")
    return problems


def main():
    ap = argparse.ArgumentParser()
    g = ap.add_mutually_exclusive_group()
    g.add_argument("--staged", action="store_true")
    g.add_argument("--changed-since", metavar="REF")
    g.add_argument("--all", action="store_true")
    g.add_argument("--files", nargs="+")
    args = ap.parse_args()
    root = repo_root()
    pats = load_allow(root)
    terms = load_terms(root)

    if args.files:
        files = sys.stdin.read().split() if args.files == ["-"] else args.files
    else:
        files = git_files(args)
        if files is None:
            ap.error("choose --staged, --changed-since REF, --all or --files")

    bad = 0
    for path in files:
        path = path.replace(os.sep, "/")
        data = read_bytes(root, path, args.staged)
        if not data and not os.path.exists(os.path.join(root, path)):
            continue  # deleted
        probs = check(path, data, pats, terms)
        if probs:
            bad += 1
            print(f"  {path}")
            for p in probs:
                print(f"      - {p}")
    if bad:
        print(f"\ndata-guard: {bad} file(s) blocked. Document-derived test data stays local (README, Test data policy).")
        print("If a file is genuinely synthetic or public-domain, add its exact path to .data-guard-allow in a reviewed PR.")
        return 1
    print(f"data-guard: ok ({len(files)} file(s) checked, {len(terms)} forbidden term(s) loaded)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
