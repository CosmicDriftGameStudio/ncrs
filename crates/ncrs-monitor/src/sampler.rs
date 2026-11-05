//! The only file that knows sysinfo. The objects live as long as the page or
//! the status bar needs them: made new for every measurement, CPU use and
//! rates would always read zero.

use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use sysinfo::{
    Components, CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, Pid,
    ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind, Users,
};

use crate::drives::{is_system_volume, DriveProbe};
use crate::kill::{check_kill, KillFailure, KillTarget};
use crate::snapshot::{
    CpuInfo, DriveRow, HostInfo, LoadInfo, NetworkRow, SensorRow, Snapshot, TaskCounts, TaskRow,
    TaskStatus, Usage,
};
use crate::units::rate_per_second;

/// What a measurement is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// The page is open: everything but the drives.
    Full,
    /// Only the values the status bar shows.
    Light(Wanted),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wanted {
    pub cpu: bool,
    pub memory: bool,
    pub swap: bool,
    pub load: bool,
    pub network: bool,
}

pub struct Sampler {
    system: System,
    networks: Networks,
    networks_read_at: Instant,
    // sysinfo's App Store build offers neither, and no tasks either.
    components: Option<Components>,
    users: Option<Users>,
    host: HostInfo,
    cpu_refreshes: u32,
    process_refreshes: u32,
}

impl std::fmt::Debug for Sampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sampler").finish_non_exhaustive()
    }
}

impl Sampler {
    pub fn open() -> Result<Self, String> {
        let system = System::new().map_err(|err| err.to_string())?;
        let networks = Networks::new_with_refreshed_list().map_err(|err| err.to_string())?;
        Ok(Self {
            system,
            networks,
            networks_read_at: Instant::now(),
            components: Components::new().ok(),
            users: Users::new_with_refreshed_list().ok(),
            host: HostInfo {
                name: System::host_name().unwrap_or_default(),
                os: System::long_os_version().unwrap_or_default(),
                kernel: System::kernel_version().unwrap_or_default(),
                arch: System::cpu_arch(),
                uptime_seconds: None,
            },
            cpu_refreshes: 0,
            process_refreshes: 0,
        })
    }

    pub fn measure(&mut self, depth: Depth) -> Snapshot {
        match depth {
            Depth::Full => self.measure_full(),
            Depth::Light(wanted) => self.measure_light(wanted),
        }
    }

    fn measure_light(&mut self, wanted: Wanted) -> Snapshot {
        let mut snapshot = Snapshot::default();
        if wanted.cpu {
            snapshot.cpu = Some(self.read_cpu());
        }
        if wanted.memory || wanted.swap {
            self.system
                .refresh_memory_specifics(MemoryRefreshKind::everything());
            snapshot.memory = wanted.memory.then(|| self.memory());
            snapshot.swap = wanted.swap.then(|| self.swap());
        }
        if wanted.load {
            snapshot.load = load_average();
        }
        if wanted.network {
            snapshot.networks = self.read_networks();
        }
        snapshot
    }

    fn measure_full(&mut self) -> Snapshot {
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        let mut host = self.host.clone();
        host.uptime_seconds = System::uptime().ok();
        if let Some(components) = &mut self.components {
            components.refresh(true);
        }
        let (tasks, counts) = self.read_tasks();
        Snapshot {
            host: Some(host),
            cpu: Some(self.read_cpu()),
            memory: Some(self.memory()),
            swap: Some(self.swap()),
            load: load_average(),
            networks: self.read_networks(),
            sensors: self
                .components
                .iter()
                .flatten()
                .filter_map(|component| {
                    Some(SensorRow {
                        label: component.label().to_owned(),
                        celsius: component.temperature()?,
                    })
                })
                .collect(),
            tasks,
            counts,
        }
    }

    fn read_cpu(&mut self) -> CpuInfo {
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::everything());
        self.cpu_refreshes = self.cpu_refreshes.saturating_add(1);
        CpuInfo {
            brand: self
                .system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            cores: self.system.cpus().len(),
            percent: (self.cpu_refreshes >= 2).then(|| self.system.global_cpu_usage()),
        }
    }

    fn memory(&self) -> Usage {
        Usage {
            total: self.system.total_memory(),
            used: self.system.used_memory(),
            free: self.system.free_memory(),
        }
    }

    fn swap(&self) -> Usage {
        Usage {
            total: self.system.total_swap(),
            used: self.system.used_swap(),
            free: self.system.free_swap(),
        }
    }

    fn read_networks(&mut self) -> Vec<NetworkRow> {
        self.networks.refresh(true);
        let elapsed = self.networks_read_at.elapsed();
        self.networks_read_at = Instant::now();
        let mut rows: Vec<NetworkRow> = self
            .networks
            .iter()
            .map(|(name, data)| NetworkRow {
                name: name.to_string_lossy().into_owned(),
                received_per_second: rate_per_second(data.received(), elapsed),
                transmitted_per_second: rate_per_second(data.transmitted(), elapsed),
                has_traffic: data.total_received() + data.total_transmitted() > 0,
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }

    fn read_tasks(&mut self) -> (Vec<TaskRow>, Option<TaskCounts>) {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .without_tasks()
                .with_cpu()
                .with_memory()
                .with_user(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        self.process_refreshes = self.process_refreshes.saturating_add(1);
        let measured_twice = self.process_refreshes >= 2;
        let tasks: Vec<TaskRow> = self
            .system
            .processes()
            .values()
            .map(|process| {
                let name = process.name().to_string_lossy().into_owned();
                let command = process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");
                TaskRow {
                    pid: process.pid().as_u32(),
                    start_time: process.start_time(),
                    user: process
                        .user_id()
                        .map(|id| {
                            self.users
                                .as_ref()
                                .and_then(|users| users.get_user_by_id(id))
                                .map_or_else(|| id.to_string(), |user| user.name().to_owned())
                        })
                        .unwrap_or_default(),
                    cpu: measured_twice.then(|| process.cpu_usage()),
                    resident: process.memory(),
                    cpu_seconds: process.accumulated_cpu_time().div_euclid(1000),
                    status: task_status(process.status()),
                    command: if command.is_empty() {
                        name.clone()
                    } else {
                        command
                    },
                    program: process.exe().map(std::path::Path::to_path_buf),
                    name,
                }
            })
            .collect();
        let counts = cfg!(not(windows)).then(|| TaskCounts {
            total: tasks.len(),
            running: count_status(&tasks, TaskStatus::Running),
            sleeping: count_status(&tasks, TaskStatus::Sleeping),
            other: count_status(&tasks, TaskStatus::Other),
        });
        (tasks, counts)
    }

    /// Kills the task if it still is the one the page showed.
    pub fn kill(&mut self, target: &KillTarget) -> Result<(), KillFailure> {
        let pid = Pid::from_u32(target.pid);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().without_tasks(),
        );
        let process = self.system.process(pid);
        check_kill(
            target,
            std::process::id(),
            process.map(sysinfo::Process::start_time),
        )
        .map_err(KillFailure::Refused)?;
        if process.is_some_and(sysinfo::Process::kill) {
            Ok(())
        } else {
            Err(KillFailure::NotKilled)
        }
    }
}

fn count_status(tasks: &[TaskRow], status: TaskStatus) -> usize {
    tasks
        .iter()
        .filter(|task| task.status == Some(status))
        .count()
}

fn task_status(status: ProcessStatus) -> Option<TaskStatus> {
    if cfg!(windows) {
        return None;
    }
    Some(match status {
        ProcessStatus::Run => TaskStatus::Running,
        ProcessStatus::Sleep | ProcessStatus::Idle => TaskStatus::Sleeping,
        _ => TaskStatus::Other,
    })
}

fn load_average() -> Option<LoadInfo> {
    if cfg!(windows) {
        return None;
    }
    let load = System::load_average().ok()?;
    Some(LoadInfo {
        one: load.one,
        five: load.five,
        fifteen: load.fifteen,
    })
}

/// The drives as the system lists them. The lock is this measurement's own.
#[derive(Debug, Default)]
pub struct SystemDrives {
    state: Mutex<Option<DriveState>>,
}

#[derive(Debug)]
struct DriveState {
    disks: Disks,
    read_at: Instant,
}

impl DriveProbe for SystemDrives {
    fn measure(&self) -> Vec<DriveRow> {
        let mut slot = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            let Ok(disks) = Disks::new() else {
                return Vec::new();
            };
            *slot = Some(DriveState {
                disks,
                read_at: Instant::now(),
            });
        }
        let Some(state) = slot.as_mut() else {
            return Vec::new();
        };
        state.disks.refresh_specifics(
            true,
            DiskRefreshKind::nothing().with_storage().with_io_usage(),
        );
        let elapsed = state.read_at.elapsed();
        state.read_at = Instant::now();
        state
            .disks
            .iter()
            .map(|disk| {
                let usage = disk.usage();
                let file_system = disk.file_system().to_string_lossy().into_owned();
                DriveRow {
                    name: disk.mount_point().display().to_string(),
                    mount: disk.mount_point().to_path_buf(),
                    is_system_volume: is_system_volume(disk.mount_point(), &file_system),
                    file_system,
                    total: disk.total_space(),
                    available: disk.available_space(),
                    read_per_second: rate_per_second(usage.read_bytes, elapsed),
                    written_per_second: rate_per_second(usage.written_bytes, elapsed),
                }
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn the_second_measurement_has_cpu_use_and_the_first_has_none() {
        let mut sampler = Sampler::open().unwrap();
        let first = sampler.measure(Depth::Full);
        assert_eq!(first.cpu.as_ref().unwrap().percent, None);
        assert!(first.tasks.iter().all(|task| task.cpu.is_none()));
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let second = sampler.measure(Depth::Full);
        assert!(second.cpu.unwrap().percent.is_some());
        assert!(second.tasks.iter().all(|task| task.cpu.is_some()));
        assert!(second.memory.unwrap().total > 0);
        assert!(second
            .tasks
            .iter()
            .any(|task| task.pid == std::process::id()));
    }

    #[test]
    fn a_light_measurement_reads_only_what_was_asked_for() {
        let mut sampler = Sampler::open().unwrap();
        let snapshot = sampler.measure(Depth::Light(Wanted {
            memory: true,
            ..Wanted::default()
        }));
        assert!(snapshot.memory.is_some());
        assert!(snapshot.cpu.is_none() && snapshot.swap.is_none());
        assert!(snapshot.tasks.is_empty() && snapshot.sensors.is_empty());
        assert!(snapshot.host.is_none());
    }
}
