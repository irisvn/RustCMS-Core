#!/bin/sh
# Places built guides in a checkout of the gh-pages branch of irisvn/RustCMS
# (release.yml and docs.yml; tested by scripts/test-pages-publish.sh). No git,
# no network: the caller clones, runs this, commits and pushes, and on a push
# conflict re-clones and runs it again (so the checks below run again).
#
#   pages-publish.sh release-stable PAGES VERSION VERSIONED ROOT INSTALL_SH
#   pages-publish.sh release-rc     PAGES VERSION VERSIONED
#   pages-publish.sh docs-stable    PAGES VERSION VERSIONED ROOT
#   pages-publish.sh docs-dev       PAGES DEV
#
# VERSIONED is built for /v/VERSION/, ROOT for /, DEV for /dev/.
#
#   release-stable  writes v/VERSION, replaces the root guide, install.sh, latest.txt
#   release-rc      writes v/VERSION only
#   docs-stable     only for the version in latest.txt: rewrites v/VERSION and the
#                   root guide; install.sh and latest.txt are kept as they are
#   docs-dev        replaces dev/ only
#
# The built guides are data: never executed. Symlinks and top-level names this
# script manages (.git, .github, v, dev, install.sh, latest.txt, CNAME, .nojekyll)
# are refused in them.
set -eu

die() { echo "pages-publish: $*" >&2; exit 1; }

mode=${1:-}
[ $# -ge 3 ] || die "usage: see the header of $0"
pages=$2
[ -d "$pages/.git" ] || die "$pages is not a checkout"

reserved=".git .github v dev install.sh latest.txt CNAME .nojekyll"

# A built guide: a directory with index.html, no symlink, no reserved top-level name.
check_build() {
  [ -d "$1" ] && [ -f "$1/index.html" ] || die "$1: not a built guide (no index.html)"
  [ -z "$(find "$1" -type l)" ] || die "$1: contains a symlink"
  for name in $reserved; do
    [ ! -e "$1/$name" ] || die "$1: contains $name, which only this script writes"
  done
}

check_version() {
  # One line of digits, dots and `-rc.` only (grep alone checks line by line).
  case $1 in *[!0-9.rc-]* | "") die "$1 is not a $2 version" ;; esac
  case $2 in
    stable) printf '%s\n' "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' ;;
    rc) printf '%s\n' "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-rc\.(0|[1-9][0-9]*)$' ;;
  esac || die "$1 is not a $2 version"
}

# Replaces directory $2 (inside the checkout) with a copy of $1.
replace_dir() {
  rm -rf "${pages:?}/$2"
  mkdir -p "$pages/$2"
  cp -R "$1/." "$pages/$2/"
}

# Replaces the root guide: everything at the top level except what is listed in $2.
replace_root() {
  keep=".git v dev CNAME $2"
  for entry in "$pages"/* "$pages"/.[!.]* "$pages"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    name=${entry##*/}
    case " $keep " in *" $name "*) continue ;; esac
    rm -rf "$entry"
  done
  cp -R "$1/." "$pages/"
}

case $mode in
  release-stable)
    [ $# -eq 6 ] || die "release-stable PAGES VERSION VERSIONED ROOT INSTALL_SH"
    version=$3; check_version "$version" stable
    check_build "$4"; check_build "$5"
    [ -f "$6" ] || die "$6: no install script"
    replace_dir "$4" "v/$version"
    replace_root "$5" ""
    cp "$6" "$pages/install.sh"
    echo "v$version" > "$pages/latest.txt"
    ;;
  release-rc)
    [ $# -eq 4 ] || die "release-rc PAGES VERSION VERSIONED"
    version=$3; check_version "$version" rc
    check_build "$4"
    replace_dir "$4" "v/$version"
    ;;
  docs-stable)
    [ $# -eq 5 ] || die "docs-stable PAGES VERSION VERSIONED ROOT"
    version=$3; check_version "$version" stable
    latest=$(cat "$pages/latest.txt" 2>/dev/null || true)
    [ "$latest" = "v$version" ] || die "v$version is not the latest stable release (latest.txt: ${latest:-none})"
    check_build "$4"; check_build "$5"
    replace_dir "$4" "v/$version"
    replace_root "$5" "install.sh latest.txt"
    ;;
  docs-dev)
    [ $# -eq 3 ] || die "docs-dev PAGES DEV"
    check_build "$3"
    replace_dir "$3" dev
    ;;
  *) die "unknown mode: $mode" ;;
esac
touch "$pages/.nojekyll"
echo "pages-publish: $mode done"
