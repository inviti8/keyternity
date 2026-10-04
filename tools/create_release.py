#!/usr/bin/env python3
"""
Cut a Keyternity release: tag the version in Cargo.toml and push the tag,
which runs .github/workflows/release.yml (Windows, macOS and Linux builds,
collected into a draft GitHub Release to review and publish).

Adapted from Inkternity's scripts/create_release.py. The difference: the tag
must equal Cargo.toml's version, because the app shows that version and its
update check compares it with the latest release's tag. So instead of
inventing a tag, this script reads the version, and --bump raises it first
(Cargo.toml, Cargo.lock and the installer's default) in its own commit.

By default it only prints what it would do; --push does it.

Examples (from the repo root):

  python tools/create_release.py                  # show the tag for Cargo.toml's version
  python tools/create_release.py --push           # tag it and push -> CI builds
  python tools/create_release.py --bump patch --push   # 0.1.0 -> 0.1.1, commit, tag, push
  python tools/create_release.py --suffix rc1 --push   # v0.1.0-rc1, a pre-release
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"
INSTALL_ISS = ROOT / "release" / "install.iss"
PACKAGE = "Keyternity"


def git(*args: str, capture: bool = True) -> str:
    out = subprocess.run(["git", *args], cwd=ROOT, capture_output=capture, text=True, check=True)
    return out.stdout.strip() if capture else ""


def cargo_version() -> str:
    m = re.search(r'^version = "([^"]+)"', CARGO_TOML.read_text(encoding="utf-8"), re.M)
    if not m:
        sys.exit("ERROR: no version in Cargo.toml")
    return m.group(1)


def bumped(version: str, part: str) -> str:
    major, minor, patch = (int(x) for x in version.split("."))
    if part == "major":
        return f"{major + 1}.0.0"
    if part == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def replace_once(path: Path, pattern: str, repl: str) -> None:
    text = path.read_text(encoding="utf-8")
    new, n = re.subn(pattern, repl, text, count=1, flags=re.M)
    if n != 1:
        sys.exit(f"ERROR: couldn't update the version in {path.name}")
    path.write_text(new, encoding="utf-8", newline="")


def write_version(version: str) -> None:
    replace_once(CARGO_TOML, r'^version = "[^"]+"', f'version = "{version}"')
    replace_once(CARGO_LOCK, rf'(name = "{PACKAGE}"\r?\nversion = )"[^"]+"', rf'\g<1>"{version}"')
    replace_once(INSTALL_ISS, r'(#define MyAppVersion )"[^"]+"', rf'\g<1>"{version}"')


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--bump", choices=["patch", "minor", "major"],
                   help="Raise the version first (committed as its own commit)")
    p.add_argument("--suffix", help="Pre-release suffix, e.g. rc1 -> v0.1.0-rc1")
    p.add_argument("--push", action="store_true", help="Do it (default: just print the plan)")
    p.add_argument("--remote", default="origin")
    args = p.parse_args()

    current = cargo_version()
    version = bumped(current, args.bump) if args.bump else current
    tag = f"v{version}" + (f"-{args.suffix}" if args.suffix else "")
    tags = git("tag", "--sort=-version:refname", "--list", "v*").splitlines()

    print(f"Last release tag: {tags[0] if tags else '(none)'}")
    print(f"Cargo.toml:       {current}" + (f" -> {version}" if args.bump else ""))
    print(f"Tag:              {tag}")
    if tag in tags:
        sys.exit(f"ERROR: {tag} already exists. Use --bump to release a new version.")

    if not args.push:
        print("\nRe-run with --push to " + ("commit the bump, " if args.bump else "") + "tag and push.")
        return

    if git("status", "--porcelain"):
        sys.exit("ERROR: the working tree isn't clean. Commit or stash first.")
    branch = git("rev-parse", "--abbrev-ref", "HEAD")

    if args.bump:
        write_version(version)
        git("add", str(CARGO_TOML), str(CARGO_LOCK), str(INSTALL_ISS))
        git("commit", "-m", f"Version {version}")
        git("push", args.remote, branch, capture=False)

    git("tag", "-a", tag, "-m", f"Keyternity {tag}")
    git("push", args.remote, tag, capture=False)
    print(f"\nPushed {tag}. The build runs at https://github.com/inviti8/keyternity/actions;")
    print("when it finishes, review and publish the draft release.")


if __name__ == "__main__":
    main()
