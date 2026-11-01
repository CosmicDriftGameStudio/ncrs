//! What the app tells the file operations when it calls `update`.

/// The facts about the app that the trash view's update reads.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    /// Rows of the trash view that fit the window.
    pub trash_rows: usize,
    /// The latest read of the trash; a listing of another one is dropped.
    pub trash_generation: u64,
}
