//! The `[system]` section: how the System page samples and shows the machine.
//! A missing section, or a missing key, gives the defaults.

use std::time::Duration;

use serde::Deserialize;

macro_rules! named_choice {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
    };
}

named_choice!(RefreshEvery {
    OneSecond => "1s",
    TwoSeconds => "2s",
    FiveSeconds => "5s",
});

impl RefreshEvery {
    pub const fn duration(self) -> Duration {
        Duration::from_secs(match self {
            Self::OneSecond => 1,
            Self::TwoSeconds => 2,
            Self::FiveSeconds => 5,
        })
    }
}

named_choice!(TaskSort {
    Cpu => "cpu",
    Memory => "memory",
    Time => "time",
});

named_choice!(NetworkUnit {
    Bytes => "bytes",
    Bits => "bits",
});

named_choice!(DriveColumn {
    Used => "used",
    Free => "free",
});

named_choice!(
    /// What the status bar badge can show.
    StatusValue {
        Cpu => "cpu",
        Memory => "memory",
        Swap => "swap",
        Load => "load",
        Network => "network",
    }
);

named_choice!(StatusStyle {
    Number => "number",
    Symbol => "symbol",
});

named_choice!(Visibility {
    Hide => "hide",
    Show => "show",
});

impl StatusValue {
    /// The load average does not exist on Windows.
    pub const fn available(self) -> bool {
        !matches!(self, Self::Load) || cfg!(not(windows))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SystemSettings {
    pub refresh: RefreshEvery,
    pub sort: TaskSort,
    pub network_unit: NetworkUnit,
    pub drive_column: DriveColumn,
    pub status_values: Vec<StatusValue>,
    pub status_style: StatusStyle,
    pub idle_interfaces: Visibility,
    pub system_volumes: Visibility,
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            refresh: RefreshEvery::TwoSeconds,
            sort: TaskSort::Cpu,
            network_unit: NetworkUnit::Bytes,
            drive_column: DriveColumn::Used,
            status_values: Vec::new(),
            status_style: StatusStyle::Number,
            idle_interfaces: Visibility::Hide,
            system_volumes: Visibility::Hide,
        }
    }
}

impl SystemSettings {
    /// The badge values in the fixed order of [`StatusValue::ALL`], whatever
    /// order the file lists them in.
    pub fn badge_values(&self) -> Vec<StatusValue> {
        StatusValue::ALL
            .iter()
            .copied()
            .filter(|value| value.available() && self.status_values.contains(value))
            .collect()
    }

    pub fn toggled(&self, value: StatusValue, on: bool) -> Self {
        let mut next = self.clone();
        next.status_values.retain(|chosen| *chosen != value);
        if on {
            next.status_values.push(value);
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_section_gives_the_defaults() {
        let settings = toml_edit::de::from_str::<SystemSettings>("").ok();
        assert_eq!(settings, Some(SystemSettings::default()));
        assert_eq!(
            SystemSettings::default().refresh.duration(),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn partial_section_keeps_the_other_defaults() {
        let settings = toml_edit::de::from_str::<SystemSettings>(
            "refresh = \"5s\"\nstatus_values = [\"swap\", \"cpu\"]",
        )
        .ok();
        assert_eq!(
            settings.as_ref().map(|chosen| chosen.refresh),
            Some(RefreshEvery::FiveSeconds)
        );
        assert_eq!(
            settings.as_ref().map(|chosen| chosen.sort),
            Some(TaskSort::Cpu)
        );
        assert_eq!(
            settings.map(|chosen| chosen.badge_values()),
            Some(vec![StatusValue::Cpu, StatusValue::Swap])
        );
    }

    #[test]
    fn toggling_adds_and_removes() {
        let on = SystemSettings::default().toggled(StatusValue::Memory, true);
        assert_eq!(on.status_values, [StatusValue::Memory]);
        assert!(on
            .toggled(StatusValue::Memory, false)
            .status_values
            .is_empty());
    }
}
