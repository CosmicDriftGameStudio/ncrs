//! Touch ID through LocalAuthentication. The policy is "device owner
//! authentication", so the Mac password asks when there is no Touch ID or it
//! failed. The framework has no safe binding in the lock, so the three calls
//! are made by hand.

use std::ptr::NonNull;
use std::sync::mpsc;

use block2::{Block, RcBlock};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_foundation::{NSError, NSString};

use crate::Denied;

#[link(name = "LocalAuthentication", kind = "framework")]
extern "C" {}

/// LAPolicyDeviceOwnerAuthentication.
const POLICY_DEVICE_OWNER_AUTHENTICATION: isize = 2;
/// LAErrorUserCancel, LAErrorSystemCancel and LAErrorAppCancel.
const CANCEL_CODES: [isize; 3] = [-2, -4, -9];

type Reply = Block<dyn Fn(Bool, *mut NSError)>;

pub fn verify(reason: &str) -> Result<(), Denied> {
    let Some(class) = AnyClass::get(c"LAContext") else {
        return Err(Denied::Failed(
            "LocalAuthentication is not available".to_owned(),
        ));
    };
    let Some(context) = new_context(class) else {
        return Err(Denied::Failed(
            "LocalAuthentication gave no context".to_owned(),
        ));
    };
    let (sender, receiver) = mpsc::channel();
    let reply = RcBlock::new(move |success: Bool, error: *mut NSError| {
        let outcome = if success.as_bool() {
            Ok(())
        } else {
            Err(denial(error))
        };
        // The receiver is gone only when the caller gave up waiting.
        drop(sender.send(outcome));
    });
    evaluate(&context, &NSString::from_str(reason), &reply);
    receiver
        .recv()
        .unwrap_or_else(|err| Err(Denied::Failed(err.to_string())))
}

#[allow(unsafe_code)]
fn new_context(class: &AnyClass) -> Option<Retained<AnyObject>> {
    // SAFETY: `new` on a class that has a no-argument initializer; nil is
    // turned into None.
    unsafe { msg_send![class, new] }
}

#[allow(unsafe_code)]
fn evaluate(context: &AnyObject, reason: &NSString, reply: &Reply) {
    // SAFETY: `evaluatePolicy:localizedReason:reply:` takes an NSInteger, an
    // NSString and a block of (BOOL, NSError *), which is the type of `reply`.
    // The framework copies the block, and the caller keeps `context` alive
    // until the reply came.
    let () = unsafe {
        msg_send![
            context,
            evaluatePolicy: POLICY_DEVICE_OWNER_AUTHENTICATION,
            localizedReason: reason,
            reply: reply
        ]
    };
}

#[allow(unsafe_code)]
fn denial(raw_error: *mut NSError) -> Denied {
    let Some(error_pointer) = NonNull::new(raw_error) else {
        return Denied::Failed("not verified".to_owned());
    };
    // SAFETY: LocalAuthentication passes a live NSError for the length of the
    // reply; it is only read here and not kept.
    let error = unsafe { error_pointer.as_ref() };
    if CANCEL_CODES.contains(&error.code()) {
        Denied::Cancelled
    } else {
        Denied::Failed(error.localizedDescription().to_string())
    }
}
