#!/bin/sh
# The four modes of pages-publish.sh against a fixture gh-pages checkout:
# what each one may change, what it must keep, what it refuses.
#
#   sh scripts/test-pages-publish.sh
set -eu

script=$(cd "$(dirname "$0")" && pwd)/pages-publish.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
has() { [ "$(cat "$1")" = "$2" ] || fail "$1 is '$(cat "$1" 2>/dev/null)', expected '$2'"; }
refused() { if sh "$script" "$@" 2>/dev/null; then fail "accepted: $*"; fi; }

build() { mkdir -p "$tmp/$1/assets"; echo "$1" > "$tmp/$1/index.html"; echo "$1" > "$tmp/$1/assets/app.js"; }

# gh-pages as it is after 0.1.0 and 0.2.0-rc.1.
fixture() {
  rm -rf "$tmp/pages"; mkdir -p "$tmp/pages/v/0.1.0" "$tmp/pages/v/0.2.0-rc.1" "$tmp/pages/dev" "$tmp/pages/old"
  git -C "$tmp/pages" init -q
  echo root-0.1.0 > "$tmp/pages/index.html"
  echo stale > "$tmp/pages/old/page.html"
  echo v0.1.0 > "$tmp/pages/v/0.1.0/index.html"
  echo rc > "$tmp/pages/v/0.2.0-rc.1/index.html"
  echo dev-old > "$tmp/pages/dev/index.html"
  echo install-0.1.0 > "$tmp/pages/install.sh"
  echo v0.1.0 > "$tmp/pages/latest.txt"
  echo docs.example.com > "$tmp/pages/CNAME"
}
kept_history() {
  has "$tmp/pages/v/0.2.0-rc.1/index.html" rc
  has "$tmp/pages/CNAME" docs.example.com
  [ -d "$tmp/pages/.git" ] || fail ".git removed"
  [ -f "$tmp/pages/.nojekyll" ] || fail "no .nojekyll"
}

build versioned; build root; build dev
echo install-new > "$tmp/install.sh"

# release-stable: version dir, root, installer and latest.txt advance.
fixture
sh "$script" release-stable "$tmp/pages" 0.2.0 "$tmp/versioned" "$tmp/root" "$tmp/install.sh" >/dev/null
has "$tmp/pages/v/0.2.0/index.html" versioned
has "$tmp/pages/index.html" root
has "$tmp/pages/install.sh" install-new
has "$tmp/pages/latest.txt" v0.2.0
has "$tmp/pages/v/0.1.0/index.html" v0.1.0
has "$tmp/pages/dev/index.html" dev-old
[ ! -e "$tmp/pages/old" ] || fail "release-stable kept a page of the old root guide"
kept_history

# release-rc: only its version dir.
fixture
sh "$script" release-rc "$tmp/pages" 0.3.0-rc.1 "$tmp/versioned" >/dev/null
has "$tmp/pages/v/0.3.0-rc.1/index.html" versioned
has "$tmp/pages/index.html" root-0.1.0
has "$tmp/pages/install.sh" install-0.1.0
has "$tmp/pages/latest.txt" v0.1.0
kept_history

# docs-dev: only dev/.
fixture
sh "$script" docs-dev "$tmp/pages" "$tmp/dev" >/dev/null
has "$tmp/pages/dev/index.html" dev
has "$tmp/pages/index.html" root-0.1.0
has "$tmp/pages/v/0.1.0/index.html" v0.1.0
has "$tmp/pages/install.sh" install-0.1.0
kept_history

# docs-stable: the latest release's guide (root + v/X), installer and latest.txt untouched.
fixture
sh "$script" docs-stable "$tmp/pages" 0.1.0 "$tmp/versioned" "$tmp/root" >/dev/null
has "$tmp/pages/v/0.1.0/index.html" versioned
has "$tmp/pages/index.html" root
has "$tmp/pages/install.sh" install-0.1.0
has "$tmp/pages/latest.txt" v0.1.0
has "$tmp/pages/dev/index.html" dev-old
[ ! -e "$tmp/pages/old" ] || fail "docs-stable kept a page of the old root guide"
kept_history

# Refusals.
fixture
refused docs-stable "$tmp/pages" 0.0.9 "$tmp/versioned" "$tmp/root"        # not the latest release
refused release-stable "$tmp/pages" 0.2.0-rc.1 "$tmp/versioned" "$tmp/root" "$tmp/install.sh"
refused release-rc "$tmp/pages" 0.2.0 "$tmp/versioned"
refused release-rc "$tmp/pages" '../x' "$tmp/versioned"
for name in install.sh latest.txt CNAME v dev .git .github .nojekyll; do
  build "bad-$name"; mkdir -p "$tmp/bad-$name/$name" 2>/dev/null || true; touch "$tmp/bad-$name/$name" 2>/dev/null || true
  refused docs-dev "$tmp/pages" "$tmp/bad-$name"
done
build linked; ln -s /etc/passwd "$tmp/linked/assets/x"
refused docs-dev "$tmp/pages" "$tmp/linked"
mkdir -p "$tmp/empty"
refused docs-dev "$tmp/pages" "$tmp/empty"
refused unknown "$tmp/pages" "$tmp/dev"
# Nothing changed by the refused runs.
has "$tmp/pages/dev/index.html" dev-old
has "$tmp/pages/latest.txt" v0.1.0

echo "pages-publish: 4 modes and refusals ok"
