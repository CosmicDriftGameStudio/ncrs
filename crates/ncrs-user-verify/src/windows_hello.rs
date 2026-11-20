//! Windows Hello through the consent verifier of the system.

use windows::core::HSTRING;
use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};

use crate::Denied;

fn failed(error: windows::core::Error) -> Denied {
    Denied::Failed(error.message())
}

pub fn verify(reason: &str) -> Result<(), Denied> {
    let availability = UserConsentVerifier::CheckAvailabilityAsync()
        .and_then(|asked| asked.join())
        .map_err(failed)?;
    // No Hello set up on this PC: nothing to ask. The key stays in the
    // Credential Manager, which is what protected it before.
    if availability != UserConsentVerifierAvailability::Available {
        return Ok(());
    }
    let result = UserConsentVerifier::RequestVerificationAsync(&HSTRING::from(reason))
        .and_then(|asked| asked.join())
        .map_err(failed)?;
    if result == UserConsentVerificationResult::Verified {
        Ok(())
    } else if result == UserConsentVerificationResult::Canceled {
        Err(Denied::Cancelled)
    } else {
        Err(Denied::Failed(format!(
            "Windows Hello answered {}",
            result.0
        )))
    }
}
