//! Cross-platform desktop hardware discovery (`NVIDIA` `nvidia-smi`, 64-bit Windows `Display`
//! registry + `CIM` `GPU`/`RAM`/`Disk`, and local `TCP` port availability checks).

use std::{
    net::{SocketAddr, TcpListener},
    path::Path,
    process::Command,
};

use domain::hardware::{DetectedGpu, GpuVendor, SystemSpecs};

fn classify_vendor_from_name(name: &str) -> GpuVendor {
    let lower = name.to_ascii_lowercase();
    if lower.contains("nvidia")
        || lower.contains("geforce")
        || lower.contains("rtx")
        || lower.contains("quadro")
    {
        GpuVendor::Nvidia
    } else if lower.contains("amd") || lower.contains("radeon") {
        GpuVendor::Amd
    } else if lower.contains("intel") || lower.contains("arc") || lower.contains("iris") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}

/// Parses `nvidia-smi --query-gpu=index,name,memory.total,memory.free,driver_version,uuid --format=csv,noheader,nounits`.
#[must_use]
pub fn parse_nvidia_smi_csv(csv_text: &str) -> Vec<DetectedGpu> {
    let mut gpus = Vec::new();
    for line in csv_text.lines() {
        let cols: Vec<&str> = line.split(',').map(str::trim).collect();
        if cols.len() < 4 {
            continue;
        }
        let Ok(index) = cols[0].parse::<u32>() else {
            continue;
        };
        let name = cols[1].to_owned();
        let vram_total_mib = cols[2].parse::<u32>().unwrap_or(0);
        let vram_free_mib = cols[3].parse::<u32>().unwrap_or(0);
        let driver_version = cols
            .get(4)
            .copied()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        let uuid = cols
            .get(5)
            .copied()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        gpus.push(DetectedGpu {
            index,
            name,
            vendor: GpuVendor::Nvidia,
            vram_total_mib,
            vram_free_mib,
            driver_version,
            uuid,
        });
    }
    gpus
}

fn probe_nvidia_gpus() -> Vec<DetectedGpu> {
    let candidates = [
        "nvidia-smi",
        "/usr/lib/wsl/lib/nvidia-smi",
        "C:\\Windows\\System32\\nvidia-smi.exe",
        "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
    ];
    for bin in candidates {
        let Ok(out) = Command::new(bin)
            .args([
                "--query-gpu=index,name,memory.total,memory.free,driver_version,uuid",
                "--format=csv,noheader,nounits",
            ])
            .output()
        else {
            continue;
        };
        if out.status.success() {
            let parsed = parse_nvidia_smi_csv(&String::from_utf8_lossy(&out.stdout));
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    Vec::new()
}

fn probe_windows_registry_and_cim_gpus() -> Vec<DetectedGpu> {
    if !cfg!(windows) {
        return Vec::new();
    }
    // Reads 64-bit HardwareInformation.qwMemorySize (>4 GiB safe!) with fallback to AdapterRAM.
    let script = r#"
$reg = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' -ErrorAction SilentlyContinue
Get-CimInstance Win32_VideoController | ForEach-Object {
  $vc = $_
  $match = $reg | Where-Object { $_.'DriverDesc' -eq $vc.Name } | Select-Object -First 1
  $qw = if ($match -and $match.'HardwareInformation.qwMemorySize') { [int64]$match.'HardwareInformation.qwMemorySize' } else { [int64]$vc.AdapterRAM }
  "$($vc.Name)|$qw|$($vc.DriverVersion)"
}
"#;
    let Ok(out) = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let mut gpus = Vec::new();
    for (idx, line) in String::from_utf8_lossy(&out.stdout).lines().enumerate() {
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(name) = parts.first().copied().filter(|s| !s.is_empty()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower.contains("microsoft basic") || lower.contains("remote desktop") {
            continue;
        }
        let ram_bytes = parts
            .get(1)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let vram_mib = u32::try_from(ram_bytes / (1024 * 1024)).unwrap_or(0);
        let driver = parts
            .get(2)
            .copied()
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        gpus.push(DetectedGpu {
            index: u32::try_from(idx).unwrap_or(0),
            name: name.to_owned(),
            vendor: classify_vendor_from_name(name),
            vram_total_mib: vram_mib,
            vram_free_mib: vram_mib,
            driver_version: driver,
            uuid: None,
        });
    }
    gpus
}

fn sanitize_windows_drive_letter(target_dir: &Path) -> char {
    let raw = target_dir.to_string_lossy();
    let mut chars = raw.chars();
    if let (Some(first), Some(':')) = (chars.next(), chars.next())
        && first.is_ascii_alphabetic()
    {
        return first.to_ascii_uppercase();
    }
    'C'
}

fn probe_ram_and_free_disk_mib(target_dir: &Path) -> (u64, u64) {
    if cfg!(windows) {
        let drive_letter = sanitize_windows_drive_letter(target_dir);
        let ps = format!(
            "$os = Get-CimInstance Win32_OperatingSystem; $ram = [math]::Round($os.TotalVisibleMemorySize / 1024); $drv = Get-PSDrive -Name '{drive_letter}' -ErrorAction SilentlyContinue; $free = if ($drv) {{ [math]::Round($drv.Free / 1MB) }} else {{ 0 }}; \"$ram|$free\""
        );
        if let Ok(out) = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some((r, d)) = text.trim().split_once('|') {
                return (
                    r.trim().parse::<u64>().unwrap_or(0),
                    d.trim().parse::<u64>().unwrap_or(0),
                );
            }
        }
        return (0, 0);
    }

    let ram_mib = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            text.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|kb| kb.parse::<u64>().ok())
                .map(|kb| kb / 1024)
        })
        .unwrap_or(0);

    let probe_path = if target_dir.exists() {
        target_dir
    } else {
        Path::new(".")
    };
    let disk_mib = Command::new("df")
        .args(["-Pm", &probe_path.to_string_lossy()])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .nth(1)
                .and_then(|line| line.split_whitespace().nth(3))
                .and_then(|m| m.parse::<u64>().ok())
        })
        .unwrap_or(0);

    (ram_mib, disk_mib)
}

/// Probes host CPU cores, system RAM, free disk space on `target_dir`, and installed GPUs.
#[must_use]
pub fn probe_system_specs(target_dir: &Path) -> SystemSpecs {
    let cpu_logical_cores =
        std::thread::available_parallelism().map_or(4, |n| u32::try_from(n.get()).unwrap_or(4));
    let (system_ram_mib, free_disk_mib) = probe_ram_and_free_disk_mib(target_dir);
    let mut gpus = probe_nvidia_gpus();
    if gpus.is_empty() {
        gpus = probe_windows_registry_and_cim_gpus();
    }

    SystemSpecs {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu_logical_cores,
        system_ram_mib,
        free_disk_mib,
        gpus,
    }
}

/// Checks whether `127.0.0.1:<port>` can currently be bound.
#[must_use]
pub fn is_local_port_free(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_ok()
}

/// Picks `preferred` if free on `127.0.0.1`, otherwise scans upward for the next free port.
#[must_use]
pub fn pick_free_local_port(preferred: u16) -> u16 {
    for offset in 0..50_u16 {
        let candidate = preferred.saturating_add(offset);
        if is_local_port_free(candidate) {
            return candidate;
        }
    }
    preferred
}
