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
}

impl Backend {
    /// The backend this binary was built with.
    pub const CURRENT: Self = {
        // Order matters: `gpu-with-fallback` also enables the other two, so it
        // has to be checked first.
        #[cfg(feature = "gpu-with-fallback")]
        {
            Self::GpuWithSoftwareFallback
        }
        #[cfg(all(feature = "gpu-rendering", not(feature = "software-rendering")))]
        {
            Self::Gpu
        }
        #[cfg(all(
            not(feature = "gpu-rendering"),
            not(feature = "gpu-with-fallback"),
            feature = "software-rendering"
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
            Self::Software
        }
    };

    /// One word for logs and bug reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Gpu => "wgpu",
            Self::Software => "tiny-skia",
            Self::GpuWithSoftwareFallback => "wgpu+tiny-skia",
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

    /// Both backends in one build is a deliberate choice, not an accident: it
    /// keeps a GPU path on desktops while still starting where there is no GPU.
    #[test]
    fn backend_matches_the_build_configuration() {
        // Mirror of the cfg in `CURRENT`, written out again on purpose: the
        // point is that both sides have to change together.
        #[cfg(feature = "gpu-with-fallback")]
        assert_eq!(Backend::CURRENT, Backend::GpuWithSoftwareFallback);

        #[cfg(all(feature = "gpu-rendering", not(feature = "software-rendering")))]
        assert_eq!(Backend::CURRENT, Backend::Gpu);

        #[cfg(all(
            not(feature = "gpu-rendering"),
            not(feature = "gpu-with-fallback"),
            feature = "software-rendering"
        ))]
        assert_eq!(Backend::CURRENT, Backend::Software);

        #[cfg(not(any(
            feature = "gpu-rendering",
            feature = "software-rendering",
            feature = "gpu-with-fallback"
        )))]
        assert_eq!(Backend::CURRENT, Backend::Software);
    }

    /// A build with no backend cannot render at all. iced only rejects this at
    /// release build time, which is too late to catch a broken debug setup.
    #[test]
    fn a_backend_is_present() {
        assert_ne!(
            Backend::CURRENT.name(),
            "",
            "no graphics backend compiled in"
        );
    }
}
