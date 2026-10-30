#!/usr/bin/env bash
# Builds the apt and dnf repositories with throwaway keys and installs from
# them in clean containers: the install and the upgrade must work, and a wrong
# or a missing signature must make the install fail, for the stated reason. It
# needs no release; CI runs it on every change to the packaging.
#
# <packages-dir> holds the fresh packages of <new-version> and those of an
# earlier release <old-version>, in any subfolders. The containers run with the
# host network and no mounts, so they reach the repositories over HTTP only
# and it also works against a remote Docker socket.
#
# Needs docker, python3, curl, gpg and what package-repos.sh needs.
#
# Usage: scripts/check-package-repos.sh <work-dir> <packages-dir> <new-version> <old-version>
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <work-dir> <packages-dir> <new-version> <old-version>" >&2
  exit 2
fi
work="$1"
packages="$2"
new="$3"
old="$4"
for version in "$new" "$old"; do
  if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "not a plain version: $version" >&2
    exit 2
  fi
done
if [ "$new" = "$old" ]; then
  echo "the old and the new version must differ" >&2
  exit 2
fi

here="$(cd "$(dirname "$0")" && pwd)"
port=8000
origin="http://127.0.0.1:$port"
base="$origin/signed"
mkdir -p "$work"
work=$(cd "$work" && pwd)
packages=$(cd "$packages" && pwd)
[ ! -e "$work/serve" ] || { echo "work folder is not clean: $work/serve exists" >&2; exit 2; }

server_pid=""
cleanup() {
  [ -z "$server_pid" ] || kill "$server_pid" 2>/dev/null || true
  for home in "$work"/gnupg-*; do
    [ -d "$home" ] || continue
    GNUPGHOME="$home" gpgconf --kill all 2>/dev/null || true
    rm -rf "$home"
  done
}
trap cleanup EXIT

# Two throwaway keys: A signs the repositories, B is the key a client must not
# accept for them.
mkdir -p "$work/keys" "$work/serve/keys"
for name in a b; do
  home="$work/gnupg-$name"
  mkdir "$home"
  chmod 700 "$home"
  GNUPGHOME="$home" gpg --batch --quiet --passphrase '' \
    --quick-gen-key "ncrs test $name <$name@example.invalid>" rsa4096 sign 1d
  GNUPGHOME="$home" gpg --armor --export > "$work/keys/$name.pub.asc"
  GNUPGHOME="$home" gpg --armor --export-secret-keys > "$work/keys/$name.sec.asc"
done
fingerprint_a=$(GNUPGHOME="$work/gnupg-a" gpg --list-keys --with-colons | awk -F: '/^fpr:/ { print $10; exit }')
key_id_a=${fingerprint_a: -16}
cp "$work/keys/b.pub.asc" "$work/serve/keys/wrong.asc"

echo "::group::the build refuses a signing key that is not the public key"
guard_log="$work/guard.log"
if PACKAGES_GPG_PRIVATE_KEY=$(cat "$work/keys/a.sec.asc") \
  "$here/package-repos.sh" "$packages" "$work/keys/b.pub.asc" "$base" "$work/guard-out" > "$guard_log" 2>&1; then
  cat "$guard_log"
  echo "::error::package-repos.sh signed with a key that is not the public key"
  exit 1
fi
cat "$guard_log"
grep -Eq "the signing key $fingerprint_a does not match .*b\.pub\.asc" "$guard_log" \
  || { echo "::error::the build failed, but not on the key mismatch"; exit 1; }
echo "::endgroup::"

echo "::group::build the repositories"
PACKAGES_GPG_PRIVATE_KEY=$(cat "$work/keys/a.sec.asc") \
  "$here/package-repos.sh" "$packages" "$work/keys/a.pub.asc" "$base" "$work/serve/signed"
cp -a "$work/serve/signed" "$work/serve/unsigned"
rm "$work/serve/unsigned/deb/dists/stable/InRelease" \
  "$work/serve/unsigned/deb/dists/stable/Release.gpg" \
  "$work/serve/unsigned/rpm/repodata/repomd.xml.asc"
sed -i.bak 's|/signed/|/unsigned/|' "$work/serve/unsigned/ncrs.repo"
rm "$work/serve/unsigned/ncrs.repo.bak"
echo "::endgroup::"

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work/serve" > "$work/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 50); do
  curl -fsS -o /dev/null "$origin/signed/ncrs.asc" 2>/dev/null && break
  kill -0 "$server_pid" 2>/dev/null || { cat "$work/server.log"; echo "::error::the HTTP server stopped"; exit 1; }
  sleep 0.2
done
curl -fsS -o /dev/null "$origin/signed/ncrs.asc" || { echo "::error::the HTTP server does not answer"; exit 1; }

# The Debian version string (with its revision) apt needs to pin the old one.
old_deb_version=""
while IFS= read -r -d '' file; do
  candidate=$(dpkg-deb --field "$file" Version)
  case "$candidate" in "$old" | "$old"-*) old_deb_version="$candidate" ;; esac
done < <(find "$packages" -type f -name '*.deb' -print0)
[ -n "$old_deb_version" ] || { echo "::error::no .deb of version $old in $packages"; exit 1; }

# Run in the container of every case: the checks the README leaves to the user.
prelude='
export DEBIAN_FRONTEND=noninteractive
installed_version() {
  ncrs --version | grep -F "ncrs $1 "
  test -f "$(dirname "$(readlink -f "$(command -v ncrs)")")/libpdfium.so"
}
# Runs the command, requires it to fail, and requires its output to say why.
expect_failure() {
  reason=$1
  shift
  if "$@" > /tmp/case.log 2>&1; then
    cat /tmp/case.log
    echo "::error::succeeded, but must fail: $*"
    exit 1
  fi
  cat /tmp/case.log
  grep -Eiq "$reason" /tmp/case.log || { echo "::error::failed, but not for the expected reason ($reason)"; exit 1; }
}
'

# docker run with the shared options; the script is the last argument.
in_container() {
  local image=$1 script=$2
  docker run --rm --network host \
    --env BASE="$base" --env ORIGIN="$origin" --env OLD="$old" --env NEW="$new" \
    --env OLD_DEB_VERSION="$old_deb_version" --env KEY_ID="$key_id_a" \
    "$image" bash -euc "$prelude$script"
}

apt_setup='
apt-get update -qq
apt-get install -y -qq --no-install-recommends curl ca-certificates > /dev/null
'

for image in debian:13 ubuntu:24.04; do
  echo "::group::apt on $image: install $old, upgrade to $new"
  in_container "$image" "$apt_setup"'
    curl -fsSLo /etc/apt/keyrings/ncrs.asc "$BASE/ncrs.asc"
    echo "deb [signed-by=/etc/apt/keyrings/ncrs.asc] $BASE/deb stable main" > /etc/apt/sources.list.d/ncrs.list
    apt-get update && apt-get install -y ncrs="$OLD_DEB_VERSION"
    installed_version "$OLD"
    apt-get install -y --only-upgrade ncrs
    installed_version "$NEW"
  '
  echo "::endgroup::"

  echo "::group::apt on $image: the wrong key is refused"
  in_container "$image" "$apt_setup"'
    curl -fsSLo /etc/apt/keyrings/wrong.asc "$ORIGIN/keys/wrong.asc"
    echo "deb [signed-by=/etc/apt/keyrings/wrong.asc] $BASE/deb stable main" > /etc/apt/sources.list.d/ncrs.list
    apt-get update > /tmp/update.log 2>&1 || true
    cat /tmp/update.log
    grep -Eq "NO_PUBKEY|BADSIG|not signed|Missing key" /tmp/update.log
    expect_failure "Unable to locate package|has no installation candidate" apt-get install -y ncrs
  '
  echo "::endgroup::"

  echo "::group::apt on $image: an unsigned repository is refused"
  in_container "$image" "$apt_setup"'
    curl -fsSLo /etc/apt/keyrings/ncrs.asc "$ORIGIN/signed/ncrs.asc"
    echo "deb [signed-by=/etc/apt/keyrings/ncrs.asc] $ORIGIN/unsigned/deb stable main" > /etc/apt/sources.list.d/ncrs.list
    apt-get update > /tmp/update.log 2>&1 || true
    cat /tmp/update.log
    grep -q "not signed" /tmp/update.log
    expect_failure "Unable to locate package|has no installation candidate" apt-get install -y ncrs
  '
  echo "::endgroup::"
done

echo "::group::dnf on fedora:44: install $old, upgrade to $new"
in_container fedora:44 '
  curl -fsSLo /etc/yum.repos.d/ncrs.repo "$BASE/ncrs.repo"
  dnf install -y "ncrs-$OLD"
  installed_version "$OLD"
  rpm -qi ncrs | grep -i "$KEY_ID"
  dnf upgrade -y ncrs
  installed_version "$NEW"
  rpm -qi ncrs | grep -i "$KEY_ID"
'
echo "::endgroup::"

dnf_refused='
  curl -fsSLo /etc/yum.repos.d/ncrs.repo "$BASE/ncrs.repo"
'
echo "::group::dnf: signed packages, but the metadata key is wrong"
in_container fedora:44 "$dnf_refused"'
  sed -i -e "s|^gpgcheck=.*|gpgcheck=0|" -e "s|^gpgkey=.*|gpgkey=$ORIGIN/keys/wrong.asc|" /etc/yum.repos.d/ncrs.repo
  cat /etc/yum.repos.d/ncrs.repo
  expect_failure "repomd.xml GPG signature verification error" dnf install -y ncrs
'
echo "::endgroup::"

echo "::group::dnf: signed metadata, but the package key is wrong"
in_container fedora:44 "$dnf_refused"'
  sed -i -e "s|^repo_gpgcheck=.*|repo_gpgcheck=0|" -e "s|^gpgkey=.*|gpgkey=$ORIGIN/keys/wrong.asc|" /etc/yum.repos.d/ncrs.repo
  cat /etc/yum.repos.d/ncrs.repo
  expect_failure "OpenPGP check for package .* has failed" dnf install -y ncrs
'
echo "::endgroup::"

echo "::group::dnf: unsigned metadata is refused"
in_container fedora:44 '
  curl -fsSLo /etc/yum.repos.d/ncrs.repo "$ORIGIN/unsigned/ncrs.repo"
  cat /etc/yum.repos.d/ncrs.repo
  expect_failure "GPG signature is not available" dnf install -y ncrs
'
echo "::endgroup::"

echo "::group::zypper on openSUSE Tumbleweed: install $new"
in_container opensuse/tumbleweed '
  zypper --non-interactive --gpg-auto-import-keys addrepo "$BASE/ncrs.repo"
  zypper --non-interactive --gpg-auto-import-keys install ncrs
  installed_version "$NEW"
'
echo "::endgroup::"

echo "all repository checks passed"
