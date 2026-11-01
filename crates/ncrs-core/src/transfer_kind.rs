/// Copy or move. One message with the kind inside, so the keymap and the job
/// queue do not each need a variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Copy,
    Move,
}
