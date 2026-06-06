use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SYSFS_POWER_SUPPLY: &str = "/sys/class/power_supply";
const BATTERY_TYPE: &str = "Battery";
const BATTERY_NAME_PREFIX: &str = "BAT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BatteryState {
    Charging,
    Draining,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BatterySnapshot {
    pub(crate) percentage: f32,
    pub(crate) state: BatteryState,
    pub(crate) estimate: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RateUnit {
    Power,
    Current,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StoredAmount {
    now: f64,
    full: f64,
    unit: RateUnit,
}

pub(crate) fn read_battery() -> Result<BatterySnapshot, String> {
    read_battery_from(Path::new(SYSFS_POWER_SUPPLY))
}

fn read_battery_from(root: &Path) -> Result<BatterySnapshot, String> {
    let battery = first_battery_dir(root)?;
    let stored = stored_amount(&battery);
    let percentage = stored
        .map(|stored| percentage_from_amount(stored.now, stored.full))
        .or_else(|| read_number(&battery, "capacity").map(clamp_percentage))
        .ok_or_else(|| format!("battery: {} has no capacity data", battery.display()))?;
    let status = read_trim(&battery.join("status")).unwrap_or_default();
    let state = battery_state(&status, percentage);
    let estimate = stored.and_then(|stored| estimate_duration(&battery, state, stored));

    Ok(BatterySnapshot {
        percentage,
        state,
        estimate,
    })
}

fn first_battery_dir(root: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(root)
        .map_err(|e| format!("battery: failed to read {}: {e}", root.display()))?;
    let mut batteries = Vec::new();

    for entry in entries {
        let path = entry
            .map_err(|e| format!("battery: failed to read {} entry: {e}", root.display()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let type_is_battery = read_trim(&path.join("type"))
            .map(|value| value.eq_ignore_ascii_case(BATTERY_TYPE))
            .unwrap_or(false);
        let name_is_battery = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with(BATTERY_NAME_PREFIX))
            .unwrap_or(false);
        if type_is_battery || name_is_battery {
            batteries.push(path);
        }
    }

    batteries.sort();
    batteries
        .into_iter()
        .next()
        .ok_or_else(|| format!("battery: no battery found in {}", root.display()))
}

fn battery_state(status: &str, percentage: f32) -> BatteryState {
    if status.eq_ignore_ascii_case("Charging")
        || status.eq_ignore_ascii_case("Full")
        || percentage >= 100.0
    {
        BatteryState::Charging
    } else {
        BatteryState::Draining
    }
}

fn stored_amount(dir: &Path) -> Option<StoredAmount> {
    amount_pair(
        dir,
        "energy_now",
        &["energy_full", "energy_full_design"],
        RateUnit::Power,
    )
    .or_else(|| {
        amount_pair(
            dir,
            "charge_now",
            &["charge_full", "charge_full_design"],
            RateUnit::Current,
        )
    })
}

fn amount_pair(
    dir: &Path,
    now_name: &str,
    full_names: &[&str],
    unit: RateUnit,
) -> Option<StoredAmount> {
    let now = read_number(dir, now_name)?;
    let full = full_names.iter().find_map(|name| read_number(dir, name))?;
    if full <= 0.0 {
        return None;
    }
    Some(StoredAmount { now, full, unit })
}

fn estimate_duration(dir: &Path, state: BatteryState, stored: StoredAmount) -> Option<Duration> {
    let remaining = match state {
        BatteryState::Charging => (stored.full - stored.now).max(0.0),
        BatteryState::Draining => stored.now.max(0.0),
    };
    let rate = rate_for(dir, stored.unit)?;
    let seconds = (remaining / rate) * 3600.0;
    if !seconds.is_finite() || seconds < 0.0 || seconds > u64::MAX as f64 {
        return None;
    }
    Some(Duration::from_secs(seconds.round() as u64))
}

fn rate_for(dir: &Path, unit: RateUnit) -> Option<f64> {
    let rate = match unit {
        RateUnit::Power => read_number(dir, "power_now").or_else(|| {
            let voltage = read_number(dir, "voltage_now")?;
            let current = read_number(dir, "current_now")?;
            Some((voltage * current) / 1_000_000.0)
        }),
        RateUnit::Current => read_number(dir, "current_now").or_else(|| {
            let power = read_number(dir, "power_now")?;
            let voltage = read_number(dir, "voltage_now")?;
            if voltage <= 0.0 {
                return None;
            }
            Some((power * 1_000_000.0) / voltage)
        }),
    }?;
    (rate > 0.0).then_some(rate)
}

fn percentage_from_amount(now: f64, full: f64) -> f32 {
    clamp_percentage((now / full) * 100.0)
}

fn clamp_percentage(value: f64) -> f32 {
    value.clamp(0.0, 100.0) as f32
}

fn read_number(dir: &Path, name: &str) -> Option<f64> {
    let value = read_trim(&dir.join(name)).ok()?;
    value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn read_trim(path: &Path) -> Result<String, std::io::Error> {
    fs::read_to_string(path).map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        root.push(format!(
            "oxibar-battery-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, relative: &str, value: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }

    #[test]
    fn reads_discharge_percentage_and_remaining_time() {
        let root = temp_root("drain");
        write(&root, "BAT0/type", "Battery");
        write(&root, "BAT0/status", "Discharging");
        write(&root, "BAT0/energy_now", "44000000");
        write(&root, "BAT0/energy_full", "50000000");
        write(&root, "BAT0/power_now", "11000000");

        let snapshot = read_battery_from(&root).unwrap();

        assert_eq!(snapshot.state, BatteryState::Draining);
        assert_eq!(snapshot.percentage, 88.0);
        assert_eq!(snapshot.estimate, Some(Duration::from_secs(4 * 60 * 60)));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_charge_percentage_and_time_to_full() {
        let root = temp_root("charge");
        write(&root, "BAT0/type", "Battery");
        write(&root, "BAT0/status", "Charging");
        write(&root, "BAT0/charge_now", "1000000");
        write(&root, "BAT0/charge_full", "2000000");
        write(&root, "BAT0/current_now", "500000");

        let snapshot = read_battery_from(&root).unwrap();

        assert_eq!(snapshot.state, BatteryState::Charging);
        assert_eq!(snapshot.percentage, 50.0);
        assert_eq!(snapshot.estimate, Some(Duration::from_secs(2 * 60 * 60)));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn falls_back_to_capacity_without_time_estimate() {
        let root = temp_root("capacity");
        write(&root, "BAT0/type", "Battery");
        write(&root, "BAT0/status", "Unknown");
        write(&root, "BAT0/capacity", "73");

        let snapshot = read_battery_from(&root).unwrap();

        assert_eq!(snapshot.state, BatteryState::Draining);
        assert_eq!(snapshot.percentage, 73.0);
        assert_eq!(snapshot.estimate, None);

        fs::remove_dir_all(root).unwrap();
    }
}
