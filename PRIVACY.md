# Privacy policy

Last updated: 2026-10-28

ncrs is a file manager that runs on your computer. It collects no data about you.

- No account, no sign-in.
- No analytics, no tracking, no crash reports, no advertising.
- Nothing about you or your files is sent to the developer or to anyone else.

## When ncrs goes online

ncrs only connects to the network when you ask it to:

- **SFTP:** when you open an SFTP server, ncrs connects to that server.
- **Themes:** when you download a theme, ncrs fetches it from Open VSX (open-vsx.org).
- **Models:** when you download a text recognition or AI model for Hermes, ncrs fetches it from Hugging Face (huggingface.co).
- **Hermes AI server:** when you point Hermes at a model server, ncrs sends the text of the documents you file to that server. You choose and run that server yourself; it can be on your own computer.

- **Update check:** when the check is on, ncrs asks GitHub (api.github.com) for the newest release of ncrs, at most once a day. See below.

These services have their own privacy policies. ncrs sends them only what the request itself needs.

## Update check

ncrs can tell you when a newer version exists. If the check is on, ncrs sends one HTTPS request to `https://api.github.com/repos/CosmicDriftGameStudio/ncrs/releases/latest`, at most once every 24 hours. The request carries a user agent with the version of ncrs (`ncrs/<version>`) and the usual connection data every web request has (your IP address). It carries no identifier, no file names and nothing from your settings. GitHub's privacy policy applies to that request.

- The answer is only the number and the web page of the newest release. When it is newer than the version you run, a dot appears at the version number in the status bar and the About card says so. The release page opens only when you press the button for it.
- The time of the last request and the version you chose to skip are kept in a small file next to the index on your computer.
- The setting is **Check for updates** on the System page of the settings tab (`check_for_updates` in the settings file).
- Default: on for a download from the project's releases page; off when ncrs was installed through a package manager (apt, dnf, zypper, Homebrew), which has its own updates, and off in development builds. You can switch it on or off in any case.
- The App Store version makes no such request and has no such setting.

## What stays on your computer

- Settings, favourites and folder history are stored in ncrs's settings file on your computer.
- The index behind Quick Open, the content search and Hermes is an encrypted database on your computer.
- SFTP passwords and the key of that index are stored in the system's credential store (Keychain on macOS).

## Contact

Questions go to the issue tracker: https://github.com/CosmicDriftGameStudio/ncrs/issues
