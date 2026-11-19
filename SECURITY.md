# Security Policy

## Reporting a vulnerability

Please report security issues privately, **not** in a public issue.

Email: marc@cosmicdriftgamestudio.com — or use GitHub's private reporting form
("Security" → "Report a vulnerability") on this repository.

Please include: affected version, your platform, and the steps to reproduce.
We aim to acknowledge a report within three working days.

## What ncrs touches

ncrs is a file manager. It reads and writes the directories you open, moves
files to the trash and deletes them, and it reads your home directory to find
where to start. It has no telemetry and no account. Which servers it talks to,
and when, is listed in [PRIVACY.md](PRIVACY.md); that file is the single list.

Things worth knowing when judging a report:

- It runs with **your** permissions, not its own. Anything you can delete, it
  can delete. File operations are the area where a bug does real damage, and
  the input there is attacker-controlled file *names*.
- It reads files from untrusted sources: PDFs, archives, office documents and
  images that you open, search or let Hermes read. A parser bug in that path
  is in scope, also when the file arrives by download or mail.
- SFTP: host keys are checked against ncrs's own `known_hosts` file and your
  `~/.ssh/known_hosts`.
- Downloads (themes, AI model, text recognition models) only go to the hosts
  listed in PRIVACY.md, and the file is checked before it is used.
- A Hermes AI server that you configure receives document text, and a server
  on another computer only after you allowed it. Its answers are untrusted
  input.

## How the installer is anchored

`install.sh` and `install.ps1` fetch the release archive and `SHA256SUMS` from
the same GitHub release and refuse to continue when the checksum does not
match. The trust anchor is GitHub: releases of this repository are immutable
once published, so a published archive and its checksums cannot be replaced
afterwards. The installer script is also attached to each release as an asset,
so you can read the exact version before you run it. Every release asset also
carries a build provenance attestation that ties it to the release workflow
run; check a downloaded file with
`gh attestation verify <file> --repo CosmicDriftGameStudio/ncrs`. The archives
have no separate detached signature; the checksum protects against a damaged or
mismatched download, and the attestation against a file that did not come from
the release workflow, not against a compromised GitHub account.

## Supported versions

Only the latest release. Fixes land on `main` and ship in the next tag.
