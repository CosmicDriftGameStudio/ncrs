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
    /// Both compiled in; wgpu is tried first and tiny-skia takes over if the
    /// GPU is unavailable.
    GpuWithSoftwareFallback,
    /// Neither backend was compiled in. iced only rejects this in release
    /// builds; the debug build still runs, on a stub renderer.
    Missing,
}

impl Backend {
    /// The backend this binary was built with.
    ///
    /// Every branch is an exhaustive, mutually exclusive condition. Writing
    /// them as `not(...)` chains instead looks tidier but breaks: a `#[cfg]`
    /// that does not apply leaves an empty block, and the const block then
    /// evaluates to `()` rather than to a `Backend`. That only shows up in the
    /// configurations the default build does not exercise.
    pub const CURRENT: Self = {
        #[cfg(feature = "gpu-with-fallback")]
        {
            Self::GpuWithSoftwareFallback
        }
        #[cfg(all(feature = "gpu-rendering", not(feature = "software-rendering")))]
        {
            Self::Gpu
        }
        #[cfg(all(
            feature = "software-rendering",
            not(feature = "gpu-rendering"),
            not(feature = "gpu-with-fallback")
        ))]
        {
            Self::Software
        }
        #[cfg(not(any(
            feature = "gpu-rendering",
            feature = "software-rendering",
            feature = "gpu-with-fallback"
        )))]
        {
            // No backend feature: iced rejects this in release builds. Reporting
            // software keeps the debug build running instead of failing to
            // compile, so the message below can explain the situation.
            Self::Missing
        }
    };

    /// One word for logs and bug reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gpu => "wgpu",
            Self::Software => "tiny-skia",
            Self::GpuWithSoftwareFallback => "wgpu+tiny-skia",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirror of the cfg in `CURRENT`. Both sides have to change together, and
    /// this is what notices when only one does.
    #[test]
    fn backend_matches_the_build_configuration() {
        #[cfg(feature = "gpu-with-fallback")]
        assert_eq!(Backend::CURRENT, Backend::GpuWithSoftwareFallback);

        #[cfg(all(feature = "gpu-rendering", not(feature = "software-rendering")))]
        assert_eq!(Backend::CURRENT, Backend::Gpu);

        #[cfg(all(
            feature = "software-rendering",
            not(feature = "gpu-rendering"),
            not(feature = "gpu-with-fallback")
        ))]
        assert_eq!(Backend::CURRENT, Backend::Software);

        #[cfg(not(any(
            feature = "gpu-rendering",
            feature = "software-rendering",
            feature = "gpu-with-fallback"
        )))]
        assert_eq!(Backend::CURRENT, Backend::Missing);
    }

    /// A build with no backend draws nothing. iced only rejects that in release
    /// builds, so the default test run has to be the one that catches it.
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
}
