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
- **Hermes AI server:** when you point Hermes at a model server, ncrs sends the text of the documents you file to that server. You choose that server; it can be on your own computer or on another one. When it is on another computer, ncrs asks once for each server before it switches, and lists what then goes to that server, over https:
  - the text of every document Hermes reads to sort it, including the text recognised in scans and photos;
  - everything you write to Hermes in the chat, and the last question, search and answer your message refers to;
  - when Hermes learns a filing rule or reads how a folder is organised: the paths of sample documents in your archive and what Hermes found in them (sender, date, kind, amount, number);
  - when you rename several files at once: the names of up to 30 files of the folder, also in folders on another computer (SFTP).

  Cancelling keeps the model you had. Nothing goes to a server on another computer that you did not agree to. The API key for the server is kept in the system's credential store, not in the settings file.

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
- What Hermes used on your own AI server (requests, tokens, the price when the server reports one, no document text) is counted in a small log file on your computer, shown on the Usage page of the settings tab. It is never sent anywhere.
- SFTP passwords, the API key of an AI server and the key of that index are stored in the system's credential store (Keychain on macOS).

## Contact

Questions go to the issue tracker: https://github.com/CosmicDriftGameStudio/ncrs/issues
