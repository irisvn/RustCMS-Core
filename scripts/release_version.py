#!/usr/bin/env python3
"""Release version of the workspace (see docs/core/guides/releasing.md).

  release_version.py bump X.Y.Z[-rc.N]   every version in one go, then Cargo.lock
  release_version.py check vX.Y.Z[-rc.N] the tag matches every version (release workflow)
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERSION = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-rc\.(0|[1-9]\d*))?$")
# `version = "…"` of [workspace.package] and of the internal workspace dependencies.
PACKAGE = re.compile(r'(\[workspace\.package\]\n(?:[^\[\n].*\n)*?version = ")([^"]+)(")')
INTERNAL = re.compile(r'^(rustcms[a-z-]* = \{ path = "crates/[^"]+", version = ")([^"]+)(")', re.M)
# The examples record the RustCMS version they follow (`package.metadata.rustcms`).
METADATA = re.compile(r'(\[package\.metadata\.rustcms\]\nversion = ")([^"]+)(")')
EXAMPLES = ["examples/demo/Cargo.toml", "examples/lms/Cargo.toml"]


def fail(message):
    sys.exit(f"error: {message}")


def parse(version):
    if not VERSION.match(version):
        fail(f"`{version}` is not X.Y.Z or X.Y.Z-rc.N")
    return version


def replace(path, pattern, version, expected):
    text = path.read_text()
    new, count = pattern.subn(lambda m: m.group(1) + version + m.group(3), text)
    if count != expected:
        fail(f"{path.relative_to(ROOT)}: expected {expected} version(s), found {count}")
    path.write_text(new)


def bump(version):
    parse(version)
    manifest = ROOT / "Cargo.toml"
    replace(manifest, PACKAGE, version, 1)
    replace(manifest, INTERNAL, version, len(INTERNAL.findall(manifest.read_text())) or 1)
    for example in EXAMPLES:
        replace(ROOT / example, METADATA, version, 1)
    subprocess.run(["cargo", "update", "--workspace", "--quiet"], cwd=ROOT, check=True)
    print(f"bumped to {version}; now update docs/core/CHANGELOG.md and docs/guide/src/upgrade.md")
    check_changelog(version, strict=False)


def check_changelog(version, strict):
    changelog = (ROOT / "docs/core/CHANGELOG.md").read_text()
    if f"## [{version}]" not in changelog:
        message = f"docs/core/CHANGELOG.md has no `## [{version}]` section"
        if strict:
            fail(message)
        print(f"warning: {message}")


def check(tag):
    if not tag.startswith("v"):
        fail(f"tag `{tag}` must start with `v`")
    version = parse(tag[1:])
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
    )
    wrong = [
        f"{p['name']} {p['version']}"
        for p in metadata["packages"]
        if p["name"].startswith("rustcms") and p["version"] != version
    ]
    manifest = (ROOT / "Cargo.toml").read_text()
    wrong += [f"workspace dependency {m.group(0).split(' ')[0]} = {m.group(2)}"
              for m in INTERNAL.finditer(manifest) if m.group(2) != version]
    for example in EXAMPLES:
        found = METADATA.search((ROOT / example).read_text())
        if not found or found.group(2) != version:
            wrong.append(f"{example} metadata {found.group(2) if found else 'missing'}")
    if wrong:
        fail(f"tag {tag} does not match: " + ", ".join(wrong) + " (run `just release-bump`)")
    # A release candidate may still be listed under [Unreleased].
    check_changelog(version, strict="-rc." not in version)
    print(f"{tag}: every version matches")


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in ("bump", "check"):
        sys.exit(__doc__)
    (bump if sys.argv[1] == "bump" else check)(sys.argv[2])
