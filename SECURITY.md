# Security Policy

## Reporting a vulnerability

Please report security issues privately, **not** in a public issue.

Email: security@cosmicdrift.de — or use GitHub's private reporting form
("Security" → "Report a vulnerability") on this repository.

Please include: affected version, your platform, and the steps to reproduce.
We aim to acknowledge a report within three working days.

## What ncrs touches

ncrs is a file manager. It reads and — as file operations are added — writes
the directories you open, and it reads your home directory to find where to
start. It has no network access, no telemetry, and no account.

Two consequences worth knowing when judging a report:

- It runs with **your** permissions, not its own. Anything you can delete, it
  can delete. File operations are the area where a bug does real damage.
- It is a **local** application. A vulnerability is only exploitable by someone
  who can already run code on your machine or talk to your filesystem — which
  lowers the severity of most findings, but not of bugs in file operations,
  where the input is attacker-controlled file *names*.

## Supported versions

Only the latest release. Fixes land on `main` and ship in the next tag.
