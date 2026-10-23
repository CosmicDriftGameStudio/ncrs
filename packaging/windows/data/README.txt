This folder makes ncrs portable. While it sits next to ncrs.exe, config.toml
and index.db are kept here instead of in %APPDATA%\ncrs.

Delete this folder to use %APPDATA%\ncrs again.

The key of an encrypted index is not stored here but in the Windows
Credential Manager. The entry is found through a random ID in the file key-id
in this folder, so you can move this folder or use another drive letter.
Do not delete key-id. On another computer you need the recovery key.
If key-id is damaged, delete it; ncrs then asks for the recovery key.
