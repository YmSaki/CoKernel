using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using CoKernel.Desktop.Models;

namespace CoKernel.Desktop.Services;

public sealed class MetricsService : IDisposable
{
    private readonly CommandRunner _runner;
    private readonly PerformanceCounter? _cpu;

    public MetricsService(CommandRunner runner)
    {
        _runner = runner;
        try
        {
            _cpu = new PerformanceCounter("Processor", "% Processor Time", "_Total");
            _ = _cpu.NextValue();
        }
        catch { }
    }

    public async Task<ResourceSnapshot> GetAsync(CancellationToken cancellationToken = default)
    {
        var (usedMemory, totalMemory) = GetMemory();
        var drive = new DriveInfo(Path.GetPathRoot(Environment.SystemDirectory) ?? "C:\\");
        var storageTotal = drive.IsReady ? drive.TotalSize : 0L;
        var storageUsed = drive.IsReady ? drive.TotalSize - drive.AvailableFreeSpace : 0L;
        var cpu = _cpu?.NextValue() ?? 0;

        string gpuName = "Unavailable";
        double gpuUtil = 0, temp = 0, power = 0;
        long vramUsed = 0, vramTotal = 0;
        var gpu = await _runner.RunAsync("nvidia-smi", new[]
        {
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw",
            "--format=csv,noheader,nounits"
        }, timeout: TimeSpan.FromSeconds(4), cancellationToken: cancellationToken);
        if (gpu.Success)
        {
            var first = gpu.StandardOutput.Split('\n', StringSplitOptions.RemoveEmptyEntries).FirstOrDefault();
            var parts = first?.Split(',').Select(x => x.Trim()).ToArray();
            if (parts is { Length: >= 6 })
            {
                gpuName = parts[0];
                _ = double.TryParse(parts[1], NumberStyles.Any, CultureInfo.InvariantCulture, out gpuUtil);
                _ = long.TryParse(parts[2], NumberStyles.Any, CultureInfo.InvariantCulture, out vramUsed);
                _ = long.TryParse(parts[3], NumberStyles.Any, CultureInfo.InvariantCulture, out vramTotal);
                _ = double.TryParse(parts[4], NumberStyles.Any, CultureInfo.InvariantCulture, out temp);
                _ = double.TryParse(parts[5], NumberStyles.Any, CultureInfo.InvariantCulture, out power);
            }
        }

        long wslUsed = 0, wslTotal = 0;
        var wsl = await _runner.RunAsync("wsl.exe", new[]
        {
            "-d", "CoKernel", "--cd", "/", "--", "sh", "-lc",
            "awk '/MemTotal:/ {t=$2} /MemAvailable:/ {a=$2} END {print t-a, t}' /proc/meminfo"
        }, timeout: TimeSpan.FromSeconds(4), cancellationToken: cancellationToken);
        if (wsl.Success)
        {
            var p = wsl.StandardOutput.Trim().Split(' ', StringSplitOptions.RemoveEmptyEntries);
            if (p.Length >= 2)
            {
                _ = long.TryParse(p[0], out wslUsed);
                _ = long.TryParse(p[1], out wslTotal);
            }
        }

        return new ResourceSnapshot(cpu, usedMemory, totalMemory, storageUsed, storageTotal,
            gpuName, gpuUtil, vramUsed, vramTotal, temp, power, wslUsed, wslTotal);
    }

    private static (ulong Used, ulong Total) GetMemory()
    {
        var status = new MemoryStatusEx();
        if (!GlobalMemoryStatusEx(status)) return (0, 0);
        return (status.ullTotalPhys - status.ullAvailPhys, status.ullTotalPhys);
    }

    public void Dispose() => _cpu?.Dispose();

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Auto)]
    private sealed class MemoryStatusEx
    {
        public uint dwLength = (uint)Marshal.SizeOf(typeof(MemoryStatusEx));
        public uint dwMemoryLoad;
        public ulong ullTotalPhys;
        public ulong ullAvailPhys;
        public ulong ullTotalPageFile;
        public ulong ullAvailPageFile;
        public ulong ullTotalVirtual;
        public ulong ullAvailVirtual;
        public ulong ullAvailExtendedVirtual;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
    private static extern bool GlobalMemoryStatusEx([In, Out] MemoryStatusEx lpBuffer);
}
