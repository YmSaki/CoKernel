use crate::{Gpu, Metric, Unavailable};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

const LIMIT: u64 = 65_536;
fn executable() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let mut directory = [0u16; 32768];
        let len = unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                directory.as_mut_ptr(),
                directory.len() as u32,
            )
        } as usize;
        if len == 0 || len >= directory.len() {
            return None;
        }
        let path =
            PathBuf::from(String::from_utf16(&directory[..len]).ok()?).join("nvidia-smi.exe");
        path.is_file().then_some(path)
    }
    #[cfg(target_os = "linux")]
    {
        ["/usr/lib/wsl/lib/nvidia-smi", "/usr/bin/nvidia-smi"]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
    }
}
pub(super) async fn sample() -> Result<Vec<Gpu>, Unavailable> {
    let path = tokio::task::spawn_blocking(executable)
        .await
        .map_err(|_| Unavailable::CollectorFailed)?
        .ok_or(Unavailable::NotInstalled)?;
    let mut command = diagnostic_command(&path);
    // Do not use a Project/current working directory for diagnostic DLL/file lookup.
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    command.args([
        "--query-gpu=uuid,name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
        "--format=csv,noheader,nounits",
    ]);
    let bytes = output(&mut command, Duration::from_secs(2)).await?;
    parse(std::str::from_utf8(&bytes).map_err(|_| Unavailable::InvalidData)?)
}
fn diagnostic_command(path: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(path);
    command.env_clear().env("LC_ALL", "C");
    #[cfg(windows)]
    {
        command.creation_flags(0x08000000);
        // NVIDIA's Windows NVML discovery requires ProgramFiles, even for System32's CLI.
        for key in ["SystemRoot", "ProgramFiles"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
    }
    command
}
async fn output(command: &mut Command, budget: Duration) -> Result<Vec<u8>, Unavailable> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| Unavailable::ReadFailed)?;
    let stdout = child.stdout.take().ok_or(Unavailable::ReadFailed)?;
    let result = tokio::time::timeout(budget, async {
        let mut bytes = Vec::new();
        stdout
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| Unavailable::ReadFailed)?;
        if bytes.len() as u64 > LIMIT {
            return Err(Unavailable::InvalidData);
        }
        let status = child.wait().await.map_err(|_| Unavailable::ReadFailed)?;
        if !status.success() {
            return Err(Unavailable::ReadFailed);
        }
        Ok(bytes)
    })
    .await
    .unwrap_or(Err(Unavailable::TimedOut));
    if result.is_err() {
        let _ = child.start_kill();
        // Reap the supervised diagnostic child, including timeout/output overflow.
        let _ = child.wait().await;
    }
    result
}
fn row(line: &str) -> Result<Vec<String>, Unavailable> {
    let mut fields = Vec::new();
    let mut rest = line.trim();
    loop {
        let (value, tail) = if let Some(quoted) = rest.strip_prefix('"') {
            let mut chars = quoted.char_indices().peekable();
            let mut value = String::new();
            let mut end = None;
            while let Some((index, c)) = chars.next() {
                if c == '"' {
                    if chars.peek().is_some_and(|(_, next)| *next == '"') {
                        chars.next();
                        value.push('"');
                    } else {
                        end = Some(index + 1);
                        break;
                    }
                } else {
                    value.push(c);
                }
            }
            let tail = quoted
                .get(end.ok_or(Unavailable::InvalidData)?..)
                .ok_or(Unavailable::InvalidData)?
                .trim_start();
            (value, tail)
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            let value = rest[..end].trim();
            if value.contains('"') {
                return Err(Unavailable::InvalidData);
            }
            (value.to_owned(), &rest[end..])
        };
        fields.push(value);
        if fields.len() > 7 {
            return Err(Unavailable::InvalidData);
        }
        if tail.is_empty() {
            break;
        }
        rest = tail
            .strip_prefix(',')
            .ok_or(Unavailable::InvalidData)?
            .trim_start();
    }
    if fields.len() != 7 {
        return Err(Unavailable::InvalidData);
    }
    Ok(fields)
}
fn number(text: &str, min: f64, max: f64) -> Metric<f64> {
    if matches!(
        text.trim(),
        "N/A" | "[N/A]" | "Not Supported" | "[Not Supported]"
    ) {
        return Metric::unavailable(Unavailable::NotSupported);
    }
    match text.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && n >= min && n <= max => Metric::available(n),
        _ => Metric::unavailable(Unavailable::InvalidData),
    }
}
fn memory(text: &str) -> Metric<u64> {
    let mib = number(text, 0.0, (u64::MAX / 1_048_576) as f64);
    match mib.value {
        Some(n) => Metric::available((n * 1_048_576.0) as u64),
        None => Metric::unavailable(mib.unavailable.unwrap_or(Unavailable::InvalidData)),
    }
}
fn parse(text: &str) -> Result<Vec<Gpu>, Unavailable> {
    let mut result = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        if result.len() >= 64 {
            return Err(Unavailable::InvalidData);
        }
        let fields = row(line)?;
        if !fields[0].starts_with("GPU-")
            || fields[0].len() > 128
            || fields[1].is_empty()
            || fields[1].len() > 256
            || fields.iter().any(|s| s.chars().any(char::is_control))
        {
            return Err(Unavailable::InvalidData);
        }
        if result.iter().any(|gpu: &Gpu| gpu.uuid == fields[0]) {
            return Err(Unavailable::InvalidData);
        }
        let mut used = memory(&fields[3]);
        let total = memory(&fields[4]);
        if used
            .value
            .zip(total.value)
            .is_some_and(|(used, total)| used > total)
        {
            used = Metric::unavailable(Unavailable::InvalidData);
        }
        result.push(Gpu {
            uuid: fields[0].clone(),
            name: fields[1].clone(),
            utilization_percent: number(&fields[2], 0.0, 100.0),
            memory_used_bytes: used,
            memory_total_bytes: total,
            temperature_celsius: number(&fields[5], -273.15, 1000.0),
            power_watts: number(&fields[6], 0.0, 10_000_000.0),
        });
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn diagnostic_does_not_inherit_product_secrets_or_python_injection() {
        if std::env::var_os("CK_METRIC_ENV_TEST").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "gpu::tests::diagnostic_does_not_inherit_product_secrets_or_python_injection",
                ])
                .env("CK_METRIC_ENV_TEST", "1")
                .env("COKERNEL_MCP_BEARER", "metric-test-secret")
                .env("PYTHONPATH", "metric-test-injection")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let mut command = diagnostic_command(if cfg!(windows) {
            "python"
        } else {
            "/usr/bin/python3"
        });
        command.args(["-c","import os; assert 'COKERNEL_MCP_BEARER' not in os.environ; assert 'PYTHONPATH' not in os.environ; assert 'CK_METRIC_ENV_TEST' not in os.environ; print('guard-ok')"]);
        assert_eq!(
            String::from_utf8(output(&mut command, Duration::from_secs(3)).await.unwrap())
                .unwrap()
                .trim(),
            "guard-ok"
        );
    }
    #[test]
    fn nvidia_csv_preserves_unknown_values_and_validates_units_and_bounds() {
        let data=parse("GPU-one, \"Model, One\", 25, 4, 24, 50, 120.5\nGPU-two, Other, N/A, [N/A], 10, N/A, N/A").unwrap();
        assert_eq!(data[0].name, "Model, One");
        assert_eq!(data[0].memory_used_bytes.value, Some(4 * 1_048_576));
        assert_eq!(
            data[1].utilization_percent.unavailable,
            Some(Unavailable::NotSupported)
        );
        let invalid = parse("GPU-one, Model, NaN, 25, 24, 5000, -1").unwrap();
        assert_eq!(
            invalid[0].utilization_percent.unavailable,
            Some(Unavailable::InvalidData)
        );
        assert_eq!(
            invalid[0].memory_used_bytes.unavailable,
            Some(Unavailable::InvalidData)
        );
        assert!(parse("GPU-x, \"unclosed, 1, 2, 3, 4, 5").is_err());
        assert!(parse(&"GPU-x, Model, 1, 2, 3, 4, 5\n".repeat(2)).is_err());
        assert!(parse("secret-bearing arbitrary stdout").is_err());
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn diagnostic_timeout_overflow_and_exit_are_bounded_and_reaped() {
        let root = std::env::temp_dir().join(format!("ckgpu-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        for mode in ["hang", "overflow", "failure"] {
            let pid = root.join(mode);
            let mut command = Command::new("/usr/bin/python3");
            command.args(["-c","import os,sys,time; open(sys.argv[1],'w').write(str(os.getpid())); m=sys.argv[2]; print('x'*70000 if m=='overflow' else '',flush=True); time.sleep(60) if m!='failure' else sys.exit(1)"]).arg(&pid).arg(mode);
            let result = output(&mut command, Duration::from_millis(500)).await;
            assert_eq!(
                result.unwrap_err(),
                match mode {
                    "hang" => Unavailable::TimedOut,
                    "overflow" => Unavailable::InvalidData,
                    _ => Unavailable::ReadFailed,
                }
            );
            let pid = std::fs::read_to_string(pid)
                .unwrap()
                .parse::<i32>()
                .unwrap();
            assert_ne!(
                unsafe { libc::kill(pid, 0) },
                0,
                "diagnostic child survived"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
