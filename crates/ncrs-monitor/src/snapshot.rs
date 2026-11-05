//! What one measurement found, as plain data. Nothing here knows sysinfo; a
//! value the system does not give is `None` or an empty list, and the view
//! leaves that block out.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub host: Option<HostInfo>,
    pub cpu: Option<CpuInfo>,
    pub memory: Option<Usage>,
    pub swap: Option<Usage>,
    pub load: Option<LoadInfo>,
    pub networks: Vec<NetworkRow>,
    pub sensors: Vec<SensorRow>,
    pub tasks: Vec<TaskRow>,
    /// Missing where the system does not tell what a task is doing.
    pub counts: Option<TaskCounts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub name: String,
    pub os: String,
    pub kernel: String,
    pub arch: String,
    pub uptime_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuInfo {
    pub brand: String,
    pub cores: usize,
    /// `None` until the CPU has been measured twice.
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub total: u64,
    pub used: u64,
    pub free: u64,
}

impl Usage {
    pub fn percent(self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used as f64 * 100.0 / self.total as f64) as f32
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadInfo {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkRow {
    pub name: String,
    pub received_per_second: Option<u64>,
    pub transmitted_per_second: Option<u64>,
    /// Anything has been sent or received since the interface came up.
    pub has_traffic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRow {
    pub name: String,
    pub mount: PathBuf,
    pub file_system: String,
    pub total: u64,
    pub available: u64,
    pub read_per_second: Option<u64>,
    pub written_per_second: Option<u64>,
    pub is_system_volume: bool,
}

impl DriveRow {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    pub fn used_percent(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used() as f64 * 100.0 / self.total as f64) as f32
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SensorRow {
    pub label: String,
    pub celsius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Running,
    Sleeping,
    Other,
}

impl TaskStatus {
    pub const fn letter(self) -> char {
        match self {
            Self::Running => 'R',
            Self::Sleeping => 'S',
            Self::Other => 'O',
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub pid: u32,
    /// Seconds since the epoch; with the pid it tells one task from a later
    /// one that got the same pid.
    pub start_time: u64,
    pub name: String,
    pub user: String,
    /// `None` until the task has been measured twice.
    pub cpu: Option<f32>,
    pub resident: u64,
    pub cpu_seconds: u64,
    pub status: Option<TaskStatus>,
    pub command: String,
    pub program: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TaskCounts {
    pub total: usize,
    pub running: usize,
    pub sleeping: usize,
    pub other: usize,
}
