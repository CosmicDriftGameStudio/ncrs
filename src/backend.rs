//! Which graphics backend this build actually uses.
//!
//! The renderer is not chosen in `main.rs`; it follows from the `iced`
//! features in `Cargo.toml`. That makes it invisible in the code and easy to
//! change by accident — enabling `wgpu` for one build and forgetting it for the
//! next produces two different backends with two different performance
//! profiles.
//!
//! The report: navigation on a high-resolution display was reported as visibly
//! laggy, and software rasterisation is the leading suspect. This test pins the
//! answer so it cannot drift silently, and prints it for anyone debugging a
//! slow build.
//!
//! `iced` resolves the features like this (iced_renderer-0.13.0/src/lib.rs:24-59):
//! both features give a fallback renderer that tries wgpu first; exactly one
//! gives that renderer alone; neither fails to compile in release builds.
/// The backend in use, as far as the build configuration can say.
///
/// This is a compile-time constant, so a test cannot be fooled by it: a wrong
/// value is a wrong value at build time too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Only one variant is current per build, so the others read as dead code here.
#[allow(dead_code)]
pub enum Backend {
    /// wgpu, using the GPU through Vulkan, Metal or DX11.
    Gpu,
    /// tiny-skia, rasterising on the CPU. No GPU or driver required.
    Software,
    /// Both compiled in. `gpu-with-fallback` asks for this deliberately.
    GpuWithSoftwareFallback,
    /// Both compiled in because `gpu-rendering` was added to the default
    /// feature set. Draws on the GPU, falls back to software if unavailable —
    /// the same renderer as above, reached by accident.
    SoftwareWithGpuFallback,
    /// Neither backend was compiled in. iced only rejects this in release
    /// builds; the debug build still runs, on a stub renderer.
    Missing,
}

impl Backend {
    /// The backend this binary was built with.
    ///
    /// Written as a single `cfg_if`-style expression rather than a block with
    /// several `#[cfg]` arms: a `#[cfg]` that does not apply leaves an empty
    /// block behind, and an empty block in a const block evaluates to `()`
    /// rather than to a `Backend`. That failure only appears in the
    /// configurations the default build does not exercise.
    pub const CURRENT: Self = {
        // The fallback build enables both single-backend features, so it has to
        // be checked before either of them.
        if cfg!(feature = "gpu-with-fallback") {
            Self::GpuWithSoftwareFallback
        } else if cfg!(feature = "gpu-rendering") {
            // wgpu without the explicit fallback flag. Software may still be on
            // through the default feature, in which case iced builds a fallback
            // renderer rather than a plain GPU one.
            if cfg!(feature = "software-rendering") {
                Self::SoftwareWithGpuFallback
            } else {
                Self::Gpu
            }
        } else if cfg!(feature = "software-rendering") {
            // Reached only when the software renderer is the sole backend.
            // The case of both features being on was handled above.
            Self::Software
        } else {
            // No backend feature: iced rejects this in release builds, so the
            // name says so plainly rather than pretending to be software.
            Self::Missing
        }
    };

    /// One word for logs and bug reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gpu => "wgpu",
            Self::Software => "tiny-skia",
            Self::GpuWithSoftwareFallback => "wgpu+tiny-skia",
            Self::SoftwareWithGpuFallback => "wgpu+tiny-skia (via default feature)",
            Self::Missing => "none (enable software-rendering or gpu-rendering)",
        }
    }
}

/// Prints the backend at startup. Debug builds only: the cost is a single
/// format, and knowing the backend is the first question when a build feels
/// slow.
pub fn log_backend() {
    #[cfg(debug_assertions)]
    eprintln!("[ncrs] graphics backend: {}", Backend::CURRENT.name());
}

// A failing assertion in a test is the signal, so `unwrap` belongs here; the
// lint is meant for the production paths.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
// reason: a test module belongs next to what it tests
#[allow(clippy::inline_modules)]
#[cfg(test)]
mod tests {
    use super::*;

    /// Every feature combination and the backend iced builds for it, taken from
    /// `iced_renderer-0.13.0/src/lib.rs:24-59`:
    ///
    ///   wgpu + tiny-skia -> fallback renderer, GPU first
    ///   wgpu            -> GPU only
    ///   tiny-skia       -> software only
    ///   neither         -> nothing; iced rejects this in release builds.
    ///
    /// Written as a table rather than a second copy of the `if` chain in
    /// `CURRENT`: a duplicated chain drifts, and then the test agrees with the
    /// bug it should catch. Here the expected value comes from the feature set
    /// alone.
    #[test]
    fn backend_matches_the_features() {
        let gpu = cfg!(feature = "gpu-rendering");
        let software = cfg!(feature = "software-rendering");
        let fallback = cfg!(feature = "gpu-with-fallback");

        let expected = match (gpu, software, fallback) {
            (_, _, true) => Backend::GpuWithSoftwareFallback,
            (true, true, false) => Backend::SoftwareWithGpuFallback,
            (true, false, false) => Backend::Gpu,
            (false, true, false) => Backend::Software,
            (false, false, false) => Backend::Missing,
        };

        assert_eq!(
            Backend::CURRENT,
            expected,
            "features: gpu={gpu} software={software} fallback={fallback}"
        );
    }

    /// A build with no backend draws nothing. iced only rejects that in release
    /// mode, so the default test run has to be the one that catches it.
    #[test]
    fn a_backend_is_compiled_in() {
        assert_ne!(
            Backend::CURRENT,
            Backend::Missing,
            "no graphics backend compiled in: the app would open an empty window"
        );
    }

    /// The name reaches the startup log, where it is the first thing worth
    /// knowing when a build feels slow.
    #[test]
    fn the_backend_has_a_readable_name() {
        assert!(!Backend::CURRENT.name().is_empty());
    }

    /// `--features` adds to the default set, it does not replace it. Asking for
    /// `gpu-rendering` on top of the default therefore yields the fallback
    /// renderer, not a pure GPU one — surprising enough to pin down.
    #[test]
    fn requesting_a_feature_adds_to_the_default() {
        let gpu = cfg!(feature = "gpu-rendering");
        let software = cfg!(feature = "software-rendering");
        let fallback = cfg!(feature = "gpu-with-fallback");

        if gpu && software && !fallback {
            assert_eq!(Backend::CURRENT, Backend::SoftwareWithGpuFallback);
        }
    }
}
