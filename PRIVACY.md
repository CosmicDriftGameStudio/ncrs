# Privacy policy

Last updated: 2026-11-19

ncrs is a file manager that runs on your computer. It collects no data about you.

- No account, no sign-in.
- No analytics, no tracking, no crash reports, no advertising.
- Nothing about you or your files is sent to the developer. Document text goes to a server only when you point Hermes at one (see below).

## When ncrs goes online

ncrs connects to the network only for the purposes below. The update check runs without a click when it is on; everything else starts when you ask for it.

- **SFTP:** when you open an SFTP server, ncrs connects to that server.
- **Themes:** when you download a theme, ncrs fetches it from Open VSX (open-vsx.org), whose downloads come from openvsx.eclipsecontent.org. When you enter a link to a theme file, ncrs fetches it from the host in that link.
- **AI model:** when you download the AI model for Hermes, ncrs fetches it from Hugging Face (huggingface.co and its download hosts below hf.co).
- **Text recognition models (Windows and Linux):** when you download the text recognition models, ncrs fetches them from ocrs-models.s3-accelerate.amazonaws.com (Amazon S3). macOS recognises text with the system and downloads nothing.
- **Hermes AI server:** when you point Hermes at a model server, ncrs sends the text of the documents you file to that server. You choose that server; it can be on your own computer or on another one. When it is on another computer, you agree to three kinds of data one by one in Settings, for that server. Of a kind you have not agreed to, nothing is sent. Over https, the three kinds are:
  - incoming documents, the text of every document Hermes reads to sort it, including the text recognised in scans and photos, and also of all documents in Read folders, again after updates that change what Hermes reads;
  - filed documents, everything you write to Hermes in the chat, the last question Hermes asked with its answer options (including the file and folder names in them), and your previous request and message that the new one refers to;
  - folders and file names, in two cases:
    - when Hermes learns a filing rule or reads how a folder is organised: the folders of sample documents in your archive and what Hermes found in them (sender, recipient, kind, date, number, tags);
    - when you rename several files at once: the names of up to 30 files of the folder, also in folders on another computer (SFTP).

  Learning a filing rule sends both filed documents and folders and file names, so it needs both agreements. A settings file from before this split, with its single agreement for a server, no longer counts: you are asked again for each kind. Of a document's text ncrs sends at most the first 16,000 characters. Cancelling keeps the model you had. Nothing goes to a server on another computer that you did not agree to. The API key for the server is kept in the system's credential store, not in the settings file.

- **Update check:** when the check is on, ncrs asks GitHub (api.github.com) for the newest release of ncrs, at most once a day, without a click. It is on by default for a download from the project's releases page. See below.

These services have their own privacy policies. ncrs sends them only what the request itself needs.

## Update check

ncrs can tell you when a newer version exists. If the check is on, ncrs sends one HTTPS request to `https://api.github.com/repos/CosmicDriftGameStudio/ncrs/releases/latest`, at most once every 24 hours. The request carries a user agent with the version of ncrs (`ncrs/<version>`) and the usual connection data every web request has (your IP address). It carries no identifier, no file names and nothing from your settings. GitHub's privacy policy applies to that request.

- The answer is only the number and the web page of the newest release. When it is newer than the version you run, a dot appears at the version number in the status bar and the About card says so. The release page opens only when you press the button for it.
- The time of the last request and the version you chose to skip are kept in a small file, `updates.toml`, next to the index on your computer.
- The setting is **Check for updates** on the Updates page of the settings tab (`check_for_updates` in the settings file).
- Default: on for a download from the project's releases page; off when ncrs was installed through a package manager (apt, dnf, zypper, Homebrew), which has its own updates, and off in development builds. You can switch it on or off in any case.
- The App Store version makes no such request and has no such setting.

## What stays on your computer

- Settings are stored in ncrs's settings file on your computer. That includes the names, IBANs and tax numbers you enter as your own for Hermes. They are kept there as text, not in the encrypted index, so keep the file out of shared dotfile repositories and unencrypted backups if that matters to you.
- Favourites, the folder history of both panels and the window size are stored in `state.toml` next to the index.
- The paths of the last 1000 files ncrs moved to the trash are kept in `journal.toml` next to the index, so that they can be put back.
- While ncrs reads a file for its text, the full path of that file is written to `reading-now` next to the index and removed when the reading is done. If ncrs is killed meanwhile, the file stays and is used at the next start to skip a file that crashes the reader.
- The host keys of SFTP servers you trusted are kept in `known_hosts` next to the index; your own `~/.ssh/known_hosts` is only read.
- The index behind Quick Open, the content search and Hermes is a database on your computer. It is encrypted unless you chose otherwise; the Hermes overview says which.
- When a file is gone from a folder ncrs reads, its entry in the index (the recognised text and the fields Hermes found) stays for 30 days, in case the file only moved or comes back. After that the entry is removed from the index. The check runs when ncrs reads that folder again.
- What Hermes used on your own AI server (requests, tokens, the price when the server reports one, no document text) is counted in a small log file on your computer, shown on the Usage page of the settings tab. It is never sent anywhere.
- SFTP passwords, the API key of an AI server and the key of that index are stored in the system's credential store (Keychain on macOS). The passphrase of a key file is stored there too, when you tick "remember" (the box starts ticked). With `--data-dir`, which is meant for throwaway test data, the index key is a file `index.key` (readable by you only) next to the index instead, so the encryption does not protect a copy of that folder.

## Contact

Questions go to the issue tracker: https://github.com/CosmicDriftGameStudio/ncrs/issues
