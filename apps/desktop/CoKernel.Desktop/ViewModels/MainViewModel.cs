using System.Windows;
using CoKernel.Desktop.Infrastructure;
using CoKernel.Desktop.Models;
using CoKernel.Desktop.Services;
using WpfMessageBox = System.Windows.MessageBox;

namespace CoKernel.Desktop.ViewModels;

public sealed class MainViewModel : ObservableObject, IDisposable
{
    private readonly CoKernelRuntimeService _runtime;
    private readonly MetricsService _metrics;
    private readonly DiagnosticsService _diagnostics;
    private readonly SettingsStore _settingsStore;
    private readonly StartupService _startup;
    private DesktopSettings _settings;
    private bool _refreshing;
    private bool _recovering;
    private int _unhealthySamples;
    private int _recoveryAttempt;
    private DateTimeOffset _nextRecoveryAt = DateTimeOffset.MinValue;
    private RuntimeHealth _lastHealth = RuntimeHealth.Stopped;

    private string _runtimeState = "Stopped";
    private string _wslStatus = "Stopped";
    private string _dockerStatus = "Unavailable";
    private string _jupyterStatus = "Unavailable";
    private string _mcpStatus = "Unavailable";
    private string _tunnelStatus = "Not configured";
    private string _gpuName = "Unavailable";
    private string _cpuText = "0 %";
    private string _memoryText = "0 / 0 GB";
    private string _storageText = "0 / 0 GB";
    private string _gpuText = "0 %";
    private string _vramText = "0 / 0 GB";
    private string _gpuTempText = "-- °C";
    private string _gpuPowerText = "-- W";
    private string _wslMemoryText = "0 / 0 GB";
    private string _kernelText = "0";
    private string _logs = "";
    private string _lastError = "";
    private bool _autoStart;
    private string _tunnelId = "";

    public MainViewModel(CoKernelRuntimeService runtime, MetricsService metrics, DiagnosticsService diagnostics,
        SettingsStore settingsStore, StartupService startup)
    {
        _runtime = runtime;
        _metrics = metrics;
        _diagnostics = diagnostics;
        _settingsStore = settingsStore;
        _startup = startup;
        _settings = settingsStore.Load();
        _autoStart = _settings.AutoStart && startup.IsEnabled;
        _tunnelId = _settings.TunnelId;

        StartCommand = new AsyncCommand(StartAsync);
        StopCommand = new AsyncCommand(StopAsync);
        RestartCommand = new AsyncCommand(RestartAsync);
        RefreshCommand = new AsyncCommand(RefreshAsync);
        LogsCommand = new AsyncCommand(RefreshLogsAsync);
        UpdateCommand = new AsyncCommand(UpdateAsync);
        DiagnosticsCommand = new AsyncCommand(ExportDiagnosticsAsync);
        SaveSettingsCommand = new AsyncCommand(SaveSettingsAsync);
        RepairCommand = new AsyncCommand(() => { _runtime.LaunchRepair(); return Task.CompletedTask; });
    }

    public event EventHandler<RuntimeHealth>? HealthChanged;

    public AsyncCommand StartCommand { get; }
    public AsyncCommand StopCommand { get; }
    public AsyncCommand RestartCommand { get; }
    public AsyncCommand RefreshCommand { get; }
    public AsyncCommand LogsCommand { get; }
    public AsyncCommand UpdateCommand { get; }
    public AsyncCommand DiagnosticsCommand { get; }
    public AsyncCommand SaveSettingsCommand { get; }
    public AsyncCommand RepairCommand { get; }

    public string RuntimeState { get => _runtimeState; private set => Set(ref _runtimeState, value); }
    public string WslStatus { get => _wslStatus; private set => Set(ref _wslStatus, value); }
    public string DockerStatus { get => _dockerStatus; private set => Set(ref _dockerStatus, value); }
    public string JupyterStatus { get => _jupyterStatus; private set => Set(ref _jupyterStatus, value); }
    public string McpStatus { get => _mcpStatus; private set => Set(ref _mcpStatus, value); }
    public string TunnelStatus { get => _tunnelStatus; private set => Set(ref _tunnelStatus, value); }
    public string GpuName { get => _gpuName; private set => Set(ref _gpuName, value); }
    public string CpuText { get => _cpuText; private set => Set(ref _cpuText, value); }
    public string MemoryText { get => _memoryText; private set => Set(ref _memoryText, value); }
    public string StorageText { get => _storageText; private set => Set(ref _storageText, value); }
    public string GpuText { get => _gpuText; private set => Set(ref _gpuText, value); }
    public string VramText { get => _vramText; private set => Set(ref _vramText, value); }
    public string GpuTempText { get => _gpuTempText; private set => Set(ref _gpuTempText, value); }
    public string GpuPowerText { get => _gpuPowerText; private set => Set(ref _gpuPowerText, value); }
    public string WslMemoryText { get => _wslMemoryText; private set => Set(ref _wslMemoryText, value); }
    public string KernelText { get => _kernelText; private set => Set(ref _kernelText, value); }
    public string Logs { get => _logs; private set => Set(ref _logs, value); }
    public string LastError { get => _lastError; private set => Set(ref _lastError, value); }
    public bool AutoStart { get => _autoStart; set => Set(ref _autoStart, value); }
    public string TunnelId { get => _tunnelId; set => Set(ref _tunnelId, value); }
    public bool HasStoredTunnelKey => !string.IsNullOrWhiteSpace(_settings.ProtectedTunnelApiKey);

    public async Task InitializeBackgroundAsync()
    {
        if (_settings.AutoStart && string.Equals(_settings.DesiredRuntimeState, "RUNNING", StringComparison.OrdinalIgnoreCase))
            await StartAsync();
        else
            await RefreshAsync();
    }

    public async Task RefreshAsync()
    {
        if (_refreshing) return;
        _refreshing = true;
        try
        {
            var statusTask = _runtime.GetStatusAsync();
            var metricsTask = _metrics.GetAsync();
            await Task.WhenAll(statusTask, metricsTask);
            var status = statusTask.Result;
            Apply(status, metricsTask.Result);

            if (status.Overall == RuntimeHealth.Healthy ||
                string.Equals(_settings.DesiredRuntimeState, "STOPPED", StringComparison.OrdinalIgnoreCase))
            {
                _unhealthySamples = 0;
                _recoveryAttempt = 0;
                _nextRecoveryAt = DateTimeOffset.MinValue;
                if (!_recovering) LastError = "";
            }
            else
            {
                _unhealthySamples++;
                if (_unhealthySamples >= 3 && !_recovering && DateTimeOffset.Now >= _nextRecoveryAt &&
                    string.Equals(_settings.DesiredRuntimeState, "RUNNING", StringComparison.OrdinalIgnoreCase))
                {
                    _ = RecoverAsync(status);
                }
            }
        }
        catch (Exception ex)
        {
            LastError = ex.Message;
        }
        finally { _refreshing = false; }
    }

    private async Task RecoverAsync(ComponentSnapshot observed)
    {
        if (_recovering) return;
        _recovering = true;
        RuntimeState = "Recovering";
        try
        {
            CommandResult result;
            if (!observed.WslRunning || !observed.DockerReady)
                result = await _runtime.StartAsync();
            else
                result = await _runtime.ReconcileAsync();

            if (!result.Success)
                throw new InvalidOperationException(result.Combined.Trim());

            _unhealthySamples = 0;
            _recoveryAttempt = 0;
            _nextRecoveryAt = DateTimeOffset.MinValue;
            LastError = "";
        }
        catch (Exception ex)
        {
            _recoveryAttempt++;
            var delays = new[] { 5, 15, 30, 60, 120 };
            var delay = delays[Math.Min(_recoveryAttempt - 1, delays.Length - 1)];
            _nextRecoveryAt = DateTimeOffset.Now.AddSeconds(delay);
            LastError = $"Automatic recovery attempt {_recoveryAttempt} failed; retrying in {delay}s. {ex.Message}";
        }
        finally { _recovering = false; }
    }

    public async Task StartAsync()
    {
        RuntimeState = "Starting";
        LastError = "";
        try
        {
            var apiKey = SettingsStore.UnprotectSecret(_settings.ProtectedTunnelApiKey);
            if (!string.IsNullOrWhiteSpace(_settings.TunnelId) && !string.IsNullOrWhiteSpace(apiKey))
            {
                var config = await _runtime.ConfigureTunnelAsync(_settings.TunnelId, apiKey);
                if (!config.Success) throw new InvalidOperationException(config.Combined.Trim());
            }

            var result = await _runtime.StartAsync();
            if (!result.Success) throw new InvalidOperationException(result.Combined.Trim());
            _settings.DesiredRuntimeState = "RUNNING";
            _settingsStore.Save(_settings);
            _unhealthySamples = 0;
            _recoveryAttempt = 0;
        }
        catch (Exception ex) { LastError = ex.Message; }
        await RefreshAsync();
    }

    public async Task StopAsync()
    {
        RuntimeState = "Stopping";
        LastError = "";
        try
        {
            _settings.DesiredRuntimeState = "STOPPED";
            _settingsStore.Save(_settings);
            var result = await _runtime.StopAsync();
            if (!result.Success) throw new InvalidOperationException(result.Combined.Trim());
            _unhealthySamples = 0;
            _recoveryAttempt = 0;
        }
        catch (Exception ex) { LastError = ex.Message; }
        await RefreshAsync();
    }

    public async Task RestartAsync()
    {
        RuntimeState = "Restarting";
        try
        {
            var result = await _runtime.RestartAsync();
            if (!result.Success) throw new InvalidOperationException(result.Combined.Trim());
            _settings.DesiredRuntimeState = "RUNNING";
            _settingsStore.Save(_settings);
            _unhealthySamples = 0;
            _recoveryAttempt = 0;
            LastError = "";
        }
        catch (Exception ex) { LastError = ex.Message; }
        await RefreshAsync();
    }

    public async Task UpdateAsync()
    {
        var kernels = await _runtime.GetActiveKernelCountAsync();
        if (kernels > 0)
        {
            var answer = WpfMessageBox.Show(
                $"{kernels} active Jupyter kernel(s) will be interrupted by the update. Update now?",
                "CoKernel update", MessageBoxButton.YesNo, MessageBoxImage.Warning);
            if (answer != MessageBoxResult.Yes) return;
        }

        RuntimeState = "Updating";
        try
        {
            var result = await _runtime.UpdateAsync();
            if (!result.Success) throw new InvalidOperationException(result.Combined.Trim());
            LastError = "";
        }
        catch (Exception ex) { LastError = ex.Message; }
        await RefreshAsync();
    }

    public async Task RefreshLogsAsync()
    {
        var result = await _runtime.GetLogsAsync();
        Logs = result.Combined.Trim();
    }

    public async Task ExportDiagnosticsAsync()
    {
        try
        {
            var path = await _diagnostics.ExportAsync();
            WpfMessageBox.Show($"Diagnostics exported to:\n{path}", "CoKernel", MessageBoxButton.OK, MessageBoxImage.Information);
        }
        catch (Exception ex) { LastError = ex.Message; }
    }

    public async Task SaveTunnelAsync(string tunnelId, string apiKey)
    {
        _settings.TunnelId = tunnelId.Trim();
        TunnelId = _settings.TunnelId;
        if (!string.IsNullOrWhiteSpace(apiKey))
            _settings.ProtectedTunnelApiKey = SettingsStore.ProtectSecret(apiKey.Trim());
        else if (string.IsNullOrWhiteSpace(tunnelId))
            _settings.ProtectedTunnelApiKey = "";
        _settingsStore.Save(_settings);

        var resolvedKey = SettingsStore.UnprotectSecret(_settings.ProtectedTunnelApiKey);
        var result = await _runtime.ConfigureTunnelAsync(_settings.TunnelId, resolvedKey);
        if (!result.Success) LastError = result.Combined.Trim();
        else LastError = "";
        Raise(nameof(HasStoredTunnelKey));
        await RefreshAsync();
    }

    public Task SaveSettingsAsync()
    {
        _settings.AutoStart = AutoStart;
        _startup.SetEnabled(AutoStart);
        _settingsStore.Save(_settings);
        return Task.CompletedTask;
    }

    private void Apply(ComponentSnapshot status, ResourceSnapshot metrics)
    {
        RuntimeState = status.Overall.ToString();
        WslStatus = status.WslRunning ? "Running" : "Stopped";
        DockerStatus = status.DockerReady ? "Healthy" : "Unavailable";
        JupyterStatus = status.JupyterReady ? "Healthy" : "Unavailable";
        McpStatus = status.McpReady ? "Healthy" : "Unavailable";
        TunnelStatus = !status.TunnelConfigured ? "Not configured" : status.TunnelReady ? "Connected" : "Degraded";
        KernelText = status.ActiveKernels.ToString();

        GpuName = metrics.GpuName;
        CpuText = $"{metrics.CpuPercent:0} %";
        MemoryText = $"{ToGiB(metrics.MemoryUsedBytes):0.0} / {ToGiB(metrics.MemoryTotalBytes):0.0} GB";
        StorageText = $"{ToGiB((ulong)Math.Max(0, metrics.StorageUsedBytes)):0} / {ToGiB((ulong)Math.Max(0, metrics.StorageTotalBytes)):0} GB";
        GpuText = $"{metrics.GpuUtilization:0} %";
        VramText = $"{metrics.GpuMemoryUsedMiB / 1024.0:0.0} / {metrics.GpuMemoryTotalMiB / 1024.0:0.0} GB";
        GpuTempText = metrics.GpuTemperatureC > 0 ? $"{metrics.GpuTemperatureC:0} °C" : "-- °C";
        GpuPowerText = metrics.GpuPowerW > 0 ? $"{metrics.GpuPowerW:0} W" : "-- W";
        WslMemoryText = $"{metrics.WslMemoryUsedKiB / 1024.0 / 1024.0:0.0} / {metrics.WslMemoryTotalKiB / 1024.0 / 1024.0:0.0} GB";

        if (status.Overall != _lastHealth)
        {
            _lastHealth = status.Overall;
            HealthChanged?.Invoke(this, status.Overall);
        }
    }

    private static double ToGiB(ulong bytes) => bytes / 1024d / 1024d / 1024d;

    public void Dispose() => _metrics.Dispose();
}
