# ncrs patch to winit 0.30.13

Copy of winit 0.30.13 from crates.io with the changes below. The first: the private CoreGraphics
functions `CGSMainConnectionID` and `CGSSetWindowBackgroundBlurRadius` are
removed from `src/platform_impl/macos/ffi.rs` (with the two imports only they
used), and `set_blur` in
`window_delegate.rs` is a no-op.

Why: App Review rejects binaries that reference private API (guideline 2.5.1).
ncrs never enables window blur.

Remove this directory and the `[patch.crates-io]` entry once iced uses a winit
version without these symbols.

## Opened files (macOS)

The application delegate answers `application:openFiles:` (declared in
`src/platform_impl/macos/app_state.rs`): the paths go into a static list and
`winit::platform::macos::take_opened_paths()` hands them out. ncrs polls it
for the Finder's "Open With", `open -a ncrs <path>` and drops on the Dock icon.
The delegate is created by winit before `NSApplication` runs, and AppKit asks
for `openFiles:` before `applicationDidFinishLaunching:`, so a later method
added from the app would be too late for a launch with a file. Winit has no
event for it; upstream 0.30 only offers a custom delegate, which would replace
winit's own.

## Quit from the Dock (macOS)

The same delegate answers `applicationShouldTerminate:` with `NSTerminateCancel`
and sets a flag that `winit::platform::macos::take_quit_request()` hands out.
ncrs polls it with the opened paths and quits through its own path, which frees
the local model first. Without it the Dock's "Quit" and `quit app "ncrs"` end
the process with `exit()` while the model still holds Metal buffers, and
llama.cpp aborts in `ggml_metal_rsets_free`. `iced::exit` stops the run loop
directly and does not come back through `terminate:`.

## Trimmed targets

`Cargo.toml` has the dependency sections for `cfg(target_family = "wasm")`
(including the `target_feature = "atomics"` and dev-dependency ones),
`cfg(target_os = "android")`, `cfg(target_os = "redox")` and the iOS-only
`objc2-foundation` and `objc2-ui-kit` removed, and so the features
`android-game-activity` and `android-native-activity` and the `ndk/rwh_0x`
entries of `rwh_04`, `rwh_05` and `rwh_06`. The sections for iOS and macOS
together and all macOS ones are unchanged, and no file under `src` was touched.

Why: ncrs builds for macOS, Linux and Windows only. Cargo.lock resolves every
target, so those entries stood in the lock, and in every tool that reads the
lock whole (Dependabot), although nothing of them is built. `cargo deny` never
saw them: deny.toml limits its graph to the release targets.

When this copy is compared with upstream, the cut has to go back in.
