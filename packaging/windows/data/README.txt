This folder makes ncrs portable. While it sits next to ncrs.exe, config.toml
and index.db are kept here instead of in %APPDATA%\ncrs.

Delete this folder to use %APPDATA%\ncrs again.

The key of an encrypted index is not stored here but in the Windows
Credential Manager, under the path of this folder. On another computer, after
moving this folder, or when the drive gets another letter, you need the
recovery key.
