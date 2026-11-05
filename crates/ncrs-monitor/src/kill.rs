//! Whether a task may be killed: the pid alone is not enough, a pid is handed
//! out again once its task is gone.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillTarget {
    pub pid: u32,
    pub start_time: u64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillRefusal {
    OwnProcess,
    /// The task is gone, or the pid belongs to another task now.
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillFailure {
    Refused(KillRefusal),
    /// The system did not let the task die.
    NotKilled,
}

/// `live_start_time` is what the system says about the task of that pid now.
pub fn check_kill(
    target: &KillTarget,
    own_pid: u32,
    live_start_time: Option<u64>,
) -> Result<(), KillRefusal> {
    // A signal to pid 0 goes to the sender's own process group.
    if target.pid == own_pid || target.pid == 0 {
        Err(KillRefusal::OwnProcess)
    } else if live_start_time == Some(target.start_time) {
        Ok(())
    } else {
        Err(KillRefusal::Changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(pid: u32) -> KillTarget {
        KillTarget {
            pid,
            start_time: 1000,
            name: "task".into(),
        }
    }

    #[test]
    fn the_own_process_is_never_killed() {
        assert_eq!(
            check_kill(&target(7), 7, Some(1000)),
            Err(KillRefusal::OwnProcess)
        );
    }

    #[test]
    fn pid_zero_is_never_killed() {
        assert_eq!(
            check_kill(&target(0), 7, Some(1000)),
            Err(KillRefusal::OwnProcess)
        );
    }

    #[test]
    fn a_task_that_started_later_under_the_same_pid_is_left_alone() {
        assert_eq!(
            check_kill(&target(8), 7, Some(2000)),
            Err(KillRefusal::Changed)
        );
        assert_eq!(check_kill(&target(8), 7, None), Err(KillRefusal::Changed));
    }

    #[test]
    fn the_same_task_may_be_killed() {
        assert_eq!(check_kill(&target(8), 7, Some(1000)), Ok(()));
    }
}
