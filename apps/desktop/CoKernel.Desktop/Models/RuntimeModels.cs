namespace CoKernel.Desktop.Models;

public enum RuntimeHealth
{
    Stopped,
    Starting,
    Healthy,
    Degraded,
    Error
}

public sealed record ComponentSnapshot(
    bool WslRunning,
    bool DockerReady,
    bool JupyterReady,
    bool McpReady,
    bool TunnelConfigured,
    bool TunnelReady,
    int ActiveKernels)
{
    public RuntimeHealth Overall => !WslRunning
        ? RuntimeHealth.Stopped
        : JupyterReady && McpReady && (!TunnelConfigured || TunnelReady)
            ? RuntimeHealth.Healthy
            : DockerReady || JupyterReady || McpReady
                ? RuntimeHealth.Degraded
                : RuntimeHealth.Error;
}

public sealed record ResourceSnapshot(
    double CpuPercent,
    ulong MemoryUsedBytes,
    ulong MemoryTotalBytes,
    long StorageUsedBytes,
    long StorageTotalBytes,
    string GpuName,
    double GpuUtilization,
    long GpuMemoryUsedMiB,
    long GpuMemoryTotalMiB,
    double GpuTemperatureC,
    double GpuPowerW,
    long WslMemoryUsedKiB,
    long WslMemoryTotalKiB);
