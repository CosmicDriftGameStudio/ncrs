# ncrs patch to winit 0.30.13

Copy of winit 0.30.13 from crates.io with one change: the private CoreGraphics
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
