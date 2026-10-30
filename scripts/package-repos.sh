#!/usr/bin/env bash
# Builds the signed apt and dnf repositories from the .deb and .rpm packages of
# the last releases. The release job publishes the result on GitHub Pages, and
# CI builds it with throwaway keys to install from it.
#
# apt-ftparchive and not reprepro or aptly: the tree is built from scratch out
# of the packages on every release, so nothing may keep state between runs.
# reprepro keeps one version per package unless a Limit is set, and aptly
# needs a database and snapshots.
#
# Needs apt-utils, dpkg-deb, gpg, gpgv, rpm (rpmsign, rpmkeys) and createrepo-c.
# The private signing key comes from the environment, never from an argument.
#
# Usage: PACKAGES_GPG_PRIVATE_KEY=<armored key> \
#        scripts/package-repos.sh <packages-dir> <public-key.asc> <base-url> <out-dir>
set -euo pipefail

fail() {
  echo "error: $*" >&2
  exit 1
}

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <packages-dir> <public-key.asc> <base-url> <out-dir>" >&2
  exit 2
fi
packages="$1"
public_key="$2"
base_url="$3"
out="$4"

[ -d "$packages" ] || { echo "not a directory: $packages" >&2; exit 2; }
[ -f "$public_key" ] || { echo "not a file: $public_key" >&2; exit 2; }
base_url_pattern='^https?://[A-Za-z0-9.-]+(:[0-9]+)?(/[A-Za-z0-9._/-]*[A-Za-z0-9._-])?$'
if ! [[ "$base_url" =~ $base_url_pattern ]]; then
  echo "not a base URL without a trailing slash: $base_url" >&2
  exit 2
fi
if [ -e "$out" ]; then
  if ! [ -d "$out" ] || [ -n "$(ls -A "$out")" ]; then
    echo "output folder exists and is not empty: $out" >&2
    exit 2
  fi
fi
[ -n "${PACKAGES_GPG_PRIVATE_KEY:-}" ] || fail "PACKAGES_GPG_PRIVATE_KEY is empty"

# Absolute, because the apt steps run from inside the output folder.
mkdir -p "$out"
out=$(cd "$out" && pwd)
packages=$(cd "$packages" && pwd)
public_key=$(cd "$(dirname "$public_key")" && pwd)/$(basename "$public_key")

work=$(mktemp -d)
GNUPGHOME="$work/gnupg"
export GNUPGHOME
mkdir "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
rpmdb="$work/rpmdb"
# gpg-agent keeps running after the import and would hold the key folder.
trap 'gpgconf --kill all 2>/dev/null || true; rm -rf "$work"' EXIT

printf '%s\n' "$PACKAGES_GPG_PRIVATE_KEY" | gpg --batch --quiet --import
unset PACKAGES_GPG_PRIVATE_KEY

# The committed public key is what users trust. Signing with any other key
# would publish a repository nobody can verify, so that stops here.
if [ "$(gpg --show-keys --with-colons "$public_key" | grep -c '^pub:')" != 1 ]; then
  fail "$public_key must hold exactly one public key"
fi
fpr=$(gpg --show-keys --with-colons "$public_key" | awk -F: '/^fpr:/ { print $10; exit }')
secret_fpr=$(gpg --list-secret-keys --with-colons | awk -F: '/^sec:/ { found = 1; next } found && /^fpr:/ { print $10; exit }')
if [ "$secret_fpr" != "$fpr" ]; then
  fail "the signing key ${secret_fpr:-<none>} does not match $public_key ($fpr)"
fi

deb_field() { dpkg-deb --field "$1" "$2"; }

mkdir -p "$out/deb/pool/main/n/ncrs" "$out/rpm"
declare -A found=()
declare -a rpm_files=()
while IFS= read -r -d '' file; do
  case "$file" in
    *.deb)
      name=$(deb_field "$file" Package)
      version=$(deb_field "$file" Version)
      arch=$(deb_field "$file" Architecture)
      [ "$name" = ncrs ] || fail "$file is the package '$name', not ncrs"
      [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9]+)?$ ]] || fail "$file has an unexpected version: $version"
      case "$arch" in amd64 | arm64) ;; *) fail "$file has an unexpected architecture: $arch" ;; esac
      # The name is what `apt-ftparchive --arch` filters on.
      target="$out/deb/pool/main/n/ncrs/ncrs_${version}_${arch}.deb"
      [ ! -e "$target" ] || fail "duplicate package: ncrs $version $arch"
      cp "$file" "$target"
      found["deb-$arch"]=1
      ;;
    *.rpm)
      read -r name version release arch < <(rpm -qp --nosignature --qf '%{NAME} %{VERSION} %{RELEASE} %{ARCH}\n' "$file")
      [ "$name" = ncrs ] || fail "$file is the package '$name', not ncrs"
      [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "$file has an unexpected version: $version"
      [[ "$release" =~ ^[0-9]+$ ]] || fail "$file has an unexpected release: $release"
      case "$arch" in x86_64 | aarch64) ;; *) fail "$file has an unexpected architecture: $arch" ;; esac
      target="$out/rpm/ncrs-$version-$release.$arch.rpm"
      [ ! -e "$target" ] || fail "duplicate package: ncrs $version-$release $arch"
      cp "$file" "$target"
      rpm_files+=("$target")
      found["rpm-$arch"]=1
      ;;
  esac
done < <(find "$packages" -type f \( -name '*.deb' -o -name '*.rpm' \) -print0 | sort -z)

for combination in deb-amd64 deb-arm64 rpm-x86_64 rpm-aarch64; do
  [ -n "${found[$combination]:-}" ] || fail "no $combination package in $packages"
done

# apt: the index per architecture, then the signed Release.
for arch in amd64 arm64; do
  index="$out/deb/dists/stable/main/binary-$arch"
  mkdir -p "$index"
  (cd "$out/deb" && apt-ftparchive --arch "$arch" packages pool) > "$index/Packages"
  gzip -9n < "$index/Packages" > "$index/Packages.gz"
done
# Written outside dists/stable: apt-ftparchive lists every file in the folder,
# the Release file itself included.
(
  cd "$out/deb"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=ncrs \
    -o APT::FTPArchive::Release::Label=ncrs \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Architectures=amd64 arm64" \
    -o "APT::FTPArchive::Release::Description=ncrs packages" \
    release dists/stable > "$work/Release"
)
mv "$work/Release" "$out/deb/dists/stable/Release"
(
  cd "$out/deb/dists/stable"
  gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --clearsign -o InRelease Release
  gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --armor --detach-sign -o Release.gpg Release
)

# dnf: rpmsign signs in place, so it works on the copies. The key is passed as
# macros, never through ~/.rpmmacros. Debian's rpm defines no __gpg, so rpmsign
# would look for a gpg it cannot exec.
rpmsign --addsign --define "__gpg $(command -v gpg)" \
  --define "_gpg_name $fpr" --define "_gpg_path $GNUPGHOME" "${rpm_files[@]}"
createrepo_c --general-compress-type=gz --no-database "$out/rpm"
gpg --batch --yes --local-user "$fpr" --digest-algo SHA512 --armor --detach-sign \
  -o "$out/rpm/repodata/repomd.xml.asc" "$out/rpm/repodata/repomd.xml"
# zypper looks for the key next to the signature.
cp "$public_key" "$out/rpm/repodata/repomd.xml.key"

cp "$public_key" "$out/ncrs.asc"
cat > "$out/ncrs.repo" <<REPO
[ncrs]
name=ncrs
baseurl=$base_url/rpm
type=rpm-md
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$base_url/ncrs.asc
REPO

# The release path has no container test, so the signatures are verified the
# way a client does it before anything is published.
gpg --dearmor < "$public_key" > "$work/ncrs.gpg"
gpgv --keyring "$work/ncrs.gpg" "$out/deb/dists/stable/InRelease"
gpgv --keyring "$work/ncrs.gpg" "$out/deb/dists/stable/Release.gpg" "$out/deb/dists/stable/Release"
gpgv --keyring "$work/ncrs.gpg" "$out/rpm/repodata/repomd.xml.asc" "$out/rpm/repodata/repomd.xml"
rpmkeys --dbpath "$rpmdb" --import "$public_key"
for file in "${rpm_files[@]}"; do
  # Read the output and not only the exit code: an unsigned package can pass
  # with its digests alone.
  verdict=$(rpmkeys --dbpath "$rpmdb" --checksig "$file")
  echo "$verdict"
  case "$verdict" in
    *"signatures OK"*) ;;
    *) fail "$file is not signed with $fpr" ;;
  esac
done

echo "Signed apt and dnf repositories in $out (key $fpr):"
echo "  deb: $(find "$out/deb/pool" -name '*.deb' -exec basename {} \; | sort | tr '\n' ' ')"
echo "  rpm: $(find "$out/rpm" -maxdepth 1 -name '*.rpm' -exec basename {} \; | sort | tr '\n' ' ')"
