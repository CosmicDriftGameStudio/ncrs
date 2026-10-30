# Package signing key

The apt and dnf repositories at <https://packages.cosmicdriftgamestudio.com>
are signed with one dedicated OpenPGP key. It signs nothing else.

| | |
| --- | --- |
| User ID | `ncrs packages <packages@cosmicdriftgamestudio.com>` |
| Fingerprint | `BD1B 15A6 1BEF B249 D7C2  5F22 1365 6CD9 3EB5 808F` |
| Algorithm | RSA 4096, signing only |
| Created | 2026-10-30 |
| Expires | **2028-10-29** |
| Public key | [`packaging/linux/ncrs-packages.asc`](../packaging/linux/ncrs-packages.asc), published as `https://packages.cosmicdriftgamestudio.com/ncrs.asc` |

RSA and not ed25519 because rpm 4.14 (RHEL 8, openSUSE Leap 15) cannot verify
EdDSA package signatures.

## What it signs

`scripts/package-repos.sh` builds both repositories from the `.deb` and `.rpm`
assets of the last three GitHub releases and signs:

- apt: `deb/dists/stable/InRelease` (clear-signed) and `Release.gpg` (detached).
- dnf: every `.rpm` in `rpm/` (`rpmsign`) and `rpm/repodata/repomd.xml.asc`.

The `.rpm` files on the release page stay unsigned; only the copies in the
repository carry the signature. Because the tree is rebuilt and the older
packages are signed again on every release, their bytes can change between
releases. A client holding stale metadata may then report a checksum mismatch
on a reinstall or downgrade; `dnf clean metadata` or `apt update` fixes it.

The script refuses to sign when the private key does not belong to the public
key committed in `packaging/linux/ncrs-packages.asc`, so the published key and
the signatures cannot drift apart.

## Where the private key lives

- In GitHub only as the secret `PACKAGES_GPG_PRIVATE_KEY` (ASCII-armored, no
  passphrase) of the `release` environment of `CosmicDriftGameStudio/ncrs`.
  That environment requires the maintainer's approval, so the key is only
  readable by the `package-repos` job of `package-repos.yml` after that approval.
- An offline copy and the revocation certificate are held by the maintainer.
  Neither is in this repository or any workflow log.
- CI on pull requests never sees the key: `scripts/check-package-repos.sh`
  generates throwaway keys for its test repositories.

## Publishing the repositories by hand

`release.yml` calls `package-repos.yml` after every tag release. To publish the
repositories again, for example for a release made before they existed:

```sh
gh workflow run package-repos.yml --ref main -f tag=v1.1.1
```

The tag must be among the newest three plain releases. The run needs the
maintainer's approval in the `release` environment and only starts from `main`
or a `v*` tag. Nothing is rebuilt: the packages of the published releases are
downloaded and signed.

## Extending the expiry

Do this a few months before 2028-10-29. Extending keeps the fingerprint, so
existing installs keep working once they have the refreshed public key.

1. On the machine holding the private key:
   `gpg --quick-set-expire BD1B15A61BEFB249D7C25F2213656CD93EB5808F 2y`
2. Export the public key and replace `packaging/linux/ncrs-packages.asc`:
   `gpg --armor --export BD1B15A61BEFB249D7C25F2213656CD93EB5808F`
3. Export the private key again and replace the `PACKAGES_GPG_PRIVATE_KEY`
   secret in the `release` environment (the self-signature changed):
   `gpg --armor --export-secret-keys BD1B15A61BEFB249D7C25F2213656CD93EB5808F`
4. Update the expiry date in this file, merge, and cut the next release; its
   deploy publishes the new `ncrs.asc`.
5. Users who installed the key file once need the refreshed one. apt and dnf
   read the file they saved, not the published one, so an expired copy breaks
   `apt update` / `dnf upgrade`. Say so in the release notes, with the key
   download command from the README.

## Rotating to a new key

For a new algorithm or when extending is not wanted.

1. Create the new key (RSA 4096, signing only) and a revocation certificate,
   offline.
2. Replace `packaging/linux/ncrs-packages.asc` and the secret, and update the
   table above.
3. Release. From then on the repository verifies only against the new key.
4. Every user has to download the new `ncrs.asc` (README commands). Announce
   it in the release notes before and with that release.

## Revoking the key

If the private key may have leaked:

1. Delete the `PACKAGES_GPG_PRIVATE_KEY` secret right away, so no further
   release signs with it.
2. Import the revocation certificate held by the maintainer and export the
   revoked public key:
   `gpg --import <revocation.asc>` then
   `gpg --armor --export BD1B15A61BEFB249D7C25F2213656CD93EB5808F`
3. Rotate to a new key as above, and publish the revoked key next to it so
   users who refresh the old key see the revocation.
4. Until the new key is deployed, disable the `deploy-packages` job or
   unpublish the Pages site, so no repository signed with the old key stays
   reachable.
