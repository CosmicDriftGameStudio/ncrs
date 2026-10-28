# Assets

`icon.svg` is the source of the app icon, drawn in the colours of the Dracula
preset (`src/palette.rs`). The other files are generated from it and committed,
so CI and contributors need no tooling: `icon-1024.png`, the window icon
`icon-128.rgba` (embedded by `src/main.rs`), the macOS `ncrs.icns` and the
Linux icons in `packaging/linux/icons/hicolor`. After a change to the SVG,
regenerate all of them on macOS (AppKit renders the SVG through `render.swift`,
`iconutil` builds the `.icns`):

```bash
scripts/icon.sh
```

The panels and rows sit on a 32-unit grid, one pixel of the 32 px icon, so the
small sizes stay sharp; keep new shapes on it.

`icons/` holds the Lucide icons the main window draws, unchanged; their license (ISC, with the MIT part for the Feather icons they derive from) is `icons/LICENSE`.
