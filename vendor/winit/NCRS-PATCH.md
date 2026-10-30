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
