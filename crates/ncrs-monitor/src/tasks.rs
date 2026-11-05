//! The task list as the page shows it: filtered, sorted, and the offset of the
//! command column.

use ncrs_config::system_monitor::TaskSort;
use ncrs_i18n::Msg;

use crate::snapshot::TaskRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskColumn {
    Cpu,
    Memory,
    Resident,
    Pid,
    User,
    Time,
    Status,
    Command,
}

impl TaskColumn {
    pub const ALL: [Self; 8] = [
        Self::Cpu,
        Self::Memory,
        Self::Resident,
        Self::Pid,
        Self::User,
        Self::Time,
        Self::Status,
        Self::Command,
    ];

    pub const fn from_setting(sort: TaskSort) -> Self {
        match sort {
            TaskSort::Cpu => Self::Cpu,
            TaskSort::Memory => Self::Memory,
            TaskSort::Time => Self::Time,
        }
    }

    pub const fn title(self) -> Msg {
        match self {
            Self::Cpu => Msg::MonitorColCpu,
            Self::Memory => Msg::MonitorColMem,
            Self::Resident => Msg::MonitorColRes,
            Self::Pid => Msg::MonitorColPid,
            Self::User => Msg::MonitorColUser,
            Self::Time => Msg::MonitorColTime,
            Self::Status => Msg::MonitorColStatus,
            Self::Command => Msg::MonitorColCommand,
        }
    }

    /// Numbers sort from the largest down, text from A.
    pub const fn sorts_descending(self) -> bool {
        !matches!(self, Self::User | Self::Status | Self::Command)
    }

    pub const fn is_right_aligned(self) -> bool {
        !matches!(self, Self::User | Self::Status | Self::Command)
    }
}

fn matches_filter(task: &TaskRow, lowercase_filter: &str) -> bool {
    lowercase_filter.is_empty()
        || task.name.to_lowercase().contains(lowercase_filter)
        || task.command.to_lowercase().contains(lowercase_filter)
}

fn compare_rows(column: TaskColumn, a: &TaskRow, b: &TaskRow) -> std::cmp::Ordering {
    let by_column = match column {
        TaskColumn::Cpu => b.cpu.unwrap_or(0.0).total_cmp(&a.cpu.unwrap_or(0.0)),
        TaskColumn::Memory | TaskColumn::Resident => b.resident.cmp(&a.resident),
        TaskColumn::Pid => b.pid.cmp(&a.pid),
        TaskColumn::Time => b.cpu_seconds.cmp(&a.cpu_seconds),
        TaskColumn::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
        TaskColumn::Status => a
            .status
            .map(|status| status.letter())
            .cmp(&b.status.map(|status| status.letter())),
        TaskColumn::Command => a.command.to_lowercase().cmp(&b.command.to_lowercase()),
    };
    by_column.then(a.pid.cmp(&b.pid))
}

/// The positions in `tasks` of the rows to show, in the order to show them.
pub fn order_tasks(tasks: &[TaskRow], column: TaskColumn, filter: &str) -> Vec<usize> {
    let lowercase_filter = filter.to_lowercase();
    let mut order: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| matches_filter(task, &lowercase_filter))
        .map(|(position, _)| position)
        .collect();
    order.sort_by(|&left, &right| match (tasks.get(left), tasks.get(right)) {
        (Some(a), Some(b)) => compare_rows(column, a, b),
        _ => std::cmp::Ordering::Equal,
    });
    order
}

/// The name a task is grouped under: the process name without a closing
/// bracket part, so `Helper (Renderer)` and `Helper` are one program.
pub fn group_key(name: &str) -> &str {
    let trimmed = name.trim_end();
    if trimmed.ends_with(')') {
        if let Some(open) = trimmed.rfind(" (") {
            return trimmed[..open].trim_end();
        }
    }
    trimmed
}

/// The tasks of one program, as one row.
#[derive(Debug, Clone)]
pub struct Group {
    pub name: String,
    /// Positions in the snapshot's tasks, the smallest pid first.
    pub members: Vec<usize>,
    /// The sums, with the smallest pid of the members and the common user or a dash.
    pub row: TaskRow,
}

impl Group {
    pub fn count(&self) -> usize {
        self.members.len()
    }

    /// The task that stands for the group: the one with the smallest pid.
    pub fn representative(&self) -> usize {
        self.members.first().copied().unwrap_or(0)
    }
}

fn sum_group(name: &str, mut members: Vec<usize>, tasks: &[TaskRow]) -> Option<Group> {
    members.sort_by_key(|&position| tasks.get(position).map(|task| task.pid));
    let rows: Vec<&TaskRow> = members.iter().filter_map(|&at| tasks.get(at)).collect();
    let first = rows.first()?;
    let cpu = rows
        .iter()
        .filter_map(|task| task.cpu)
        .reduce(|sum, one| sum + one);
    let user = if rows.iter().all(|task| task.user == first.user) {
        first.user.clone()
    } else {
        crate::units::DASH.to_owned()
    };
    let row = TaskRow {
        pid: first.pid,
        start_time: first.start_time,
        name: name.to_owned(),
        user,
        cpu,
        resident: rows.iter().map(|task| task.resident).sum(),
        cpu_seconds: rows.iter().map(|task| task.cpu_seconds).sum(),
        status: None,
        command: name.to_owned(),
        program: first.program.clone(),
    };
    Some(Group {
        name: name.to_owned(),
        members,
        row,
    })
}

/// The programs among the tasks that pass the filter, sorted by their sums.
/// The pid column sorts by the number of members.
pub fn order_groups(tasks: &[TaskRow], column: TaskColumn, filter: &str) -> Vec<Group> {
    let mut by_name: Vec<(String, Vec<usize>)> = Vec::new();
    for position in order_tasks(tasks, TaskColumn::Pid, filter) {
        let Some(task) = tasks.get(position) else {
            continue;
        };
        let key = group_key(&task.name);
        match by_name.iter_mut().find(|(name, _)| name == key) {
            Some((_, members)) => members.push(position),
            None => by_name.push((key.to_owned(), vec![position])),
        }
    }
    let mut groups: Vec<Group> = by_name
        .into_iter()
        .filter_map(|(name, members)| sum_group(&name, members, tasks))
        .collect();
    groups.sort_by(|a, b| match column {
        TaskColumn::Pid => b.count().cmp(&a.count()).then(a.row.pid.cmp(&b.row.pid)),
        _ => compare_rows(column, &a.row, &b.row),
    });
    groups
}

/// How many characters one press of an arrow moves the command column.
pub const COMMAND_SHIFT_STEP: usize = 8;

/// The offset kept inside the longest command in view: the last screenful of
/// it never scrolls out entirely.
pub fn clamp_command_offset(offset: usize, longest_command: usize) -> usize {
    offset.min(longest_command.saturating_sub(1))
}

pub fn shifted_command_offset(offset: usize, direction: isize, longest_command: usize) -> usize {
    let step = COMMAND_SHIFT_STEP.saturating_mul(direction.unsigned_abs());
    let moved = if direction < 0 {
        offset.saturating_sub(step)
    } else {
        offset.saturating_add(step)
    };
    clamp_command_offset(moved, longest_command)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::snapshot::TaskStatus;

    pub fn task(pid: u32, name: &str, cpu: f32, resident: u64) -> TaskRow {
        TaskRow {
            pid,
            start_time: 1,
            name: name.into(),
            user: "marc".into(),
            cpu: Some(cpu),
            resident,
            cpu_seconds: u64::from(pid),
            status: Some(TaskStatus::Running),
            command: format!("/bin/{name} --flag"),
            program: None,
        }
    }

    fn pids(tasks: &[TaskRow], order: &[usize]) -> Vec<u32> {
        order.iter().map(|&position| tasks[position].pid).collect()
    }

    #[test]
    fn numbers_sort_down_and_text_sorts_up() {
        let tasks = [
            task(1, "zed", 5.0, 300),
            task(2, "Alpha", 50.0, 100),
            task(3, "mid", 20.0, 200),
        ];
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Cpu, "")),
            [2, 3, 1]
        );
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Memory, "")),
            [1, 3, 2]
        );
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Time, "")),
            [3, 2, 1]
        );
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Command, "")),
            [2, 3, 1]
        );
    }

    #[test]
    fn the_filter_looks_at_name_and_command_line_without_regard_to_case() {
        let mut tasks = [task(1, "zed", 1.0, 1), task(2, "Alpha", 1.0, 1)];
        tasks[1].command = "/usr/bin/other --Needle".into();
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Pid, "ZED")),
            [1]
        );
        assert_eq!(
            pids(&tasks, &order_tasks(&tasks, TaskColumn::Pid, "needle")),
            [2]
        );
        assert!(order_tasks(&tasks, TaskColumn::Pid, "nothing").is_empty());
    }

    #[test]
    fn a_closing_bracket_part_is_not_part_of_the_group_name() {
        assert_eq!(
            group_key("Google Chrome Helper (Renderer)"),
            "Google Chrome Helper"
        );
        assert_eq!(group_key("Google Chrome Helper"), "Google Chrome Helper");
        assert_eq!(group_key("Helper (GPU) (Renderer)"), "Helper (GPU)");
        assert_eq!(group_key("(sd-pam)"), "(sd-pam)");
        assert_eq!(group_key("tool (x) "), "tool");
    }

    #[test]
    fn groups_sum_their_members_and_sort_by_the_sums() {
        let mut tasks = vec![
            task(30, "Helper (Renderer)", 10.0, 100),
            task(10, "Helper", 5.0, 50),
            task(20, "solo", 12.0, 10),
        ];
        tasks[2].user = "root".into();
        tasks[1].user = "other".into();
        let groups = order_groups(&tasks, TaskColumn::Cpu, "");
        let names: Vec<&str> = groups.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(names, ["Helper", "solo"]);
        let helper = &groups[0];
        assert_eq!(helper.count(), 2);
        assert_eq!(helper.row.pid, 10);
        assert_eq!(helper.row.cpu, Some(15.0));
        assert_eq!(helper.row.resident, 150);
        assert_eq!(helper.row.cpu_seconds, 40);
        assert_eq!(helper.row.user, crate::units::DASH);
        assert_eq!(helper.representative(), 1);
        assert_eq!(groups[1].row.user, "root");
        let by_count = order_groups(&tasks, TaskColumn::Pid, "");
        assert_eq!(by_count[0].name, "Helper");
    }

    #[test]
    fn the_filter_acts_on_the_tasks_before_they_are_grouped() {
        let tasks = [
            task(1, "Helper (Renderer)", 1.0, 1),
            task(2, "Helper", 1.0, 1),
        ];
        let groups = order_groups(&tasks, TaskColumn::Cpu, "renderer");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count(), 1);
    }

    #[test]
    fn the_command_offset_stops_at_the_longest_command() {
        assert_eq!(shifted_command_offset(0, 1, 20), 8);
        assert_eq!(shifted_command_offset(16, 1, 20), 19);
        assert_eq!(shifted_command_offset(19, 1, 20), 19);
        assert_eq!(shifted_command_offset(3, -1, 20), 0);
        assert_eq!(shifted_command_offset(0, 1, 0), 0);
    }
}
