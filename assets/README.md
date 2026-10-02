# Assets

`icon.svg` is the source of the app icon. The other files are generated from it
and committed, so CI and contributors need no tooling. Regenerating needs
macOS (AppKit renders the SVG, `iconutil` builds the `.icns`); run from this
directory:

```bash
swift render.swift png 1024 icon-1024.png
swift render.swift rgba 128 icon-128.rgba   # window icon, embedded by src/main.rs

rm -rf ncrs.iconset && mkdir ncrs.iconset
for s in 16 32 128 256 512; do
  swift render.swift png $s ncrs.iconset/icon_${s}x${s}.png
  swift render.swift png $((s * 2)) ncrs.iconset/icon_${s}x${s}@2x.png
done
iconutil -c icns ncrs.iconset -o ncrs.icns
rm -rf ncrs.iconset
```
