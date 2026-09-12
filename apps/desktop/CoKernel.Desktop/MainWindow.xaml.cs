using System.ComponentModel;
using System.Diagnostics;
using System.Windows;
using System.Windows.Threading;
using CoKernel.Desktop.Models;
using CoKernel.Desktop.Services;
using CoKernel.Desktop.ViewModels;
using Forms = System.Windows.Forms;

namespace CoKernel.Desktop;

public partial class MainWindow : Window
{
    private readonly CoKernelRuntimeService _runtime;
    private readonly MainViewModel _vm;
    private readonly DispatcherTimer _timer;
    public bool AllowClose { get; set; }

    public MainWindow()
    {
        InitializeComponent();

        var paths = RuntimePaths.Resolve();
        var runner = new CommandRunner();
        _runtime = new CoKernelRuntimeService(paths, runner);
        var settings = new SettingsStore();
        _vm = new MainViewModel(_runtime, new MetricsService(runner), new DiagnosticsService(paths, runner), settings, new StartupService());
        DataContext = _vm;
        _vm.HealthChanged += OnHealthChanged;

        _timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(3) };
        _timer.Tick += async (_, _) => await _vm.RefreshAsync();
        Loaded += async (_, _) =>
        {
            await _vm.RefreshAsync();
            _timer.Start();
        };
        Closing += OnClosing;
    }

    public Task InitializeBackgroundAsync() => _vm.InitializeBackgroundAsync();
    public Task StartRuntimeAsync() => _vm.StartAsync();
    public Task StopRuntimeAsync() => _vm.StopAsync();
    public Task RefreshAsync() => _vm.RefreshAsync();
    public void OpenJupyter() => _runtime.OpenJupyter();

    private void OpenJupyter_Click(object sender, RoutedEventArgs e) => OpenJupyter();

    private async void SaveTunnel_Click(object sender, RoutedEventArgs e)
    {
        await _vm.SaveTunnelAsync(TunnelIdBox.Text, TunnelKeyBox.Password);
        TunnelKeyBox.Clear();
    }

    private void OnHealthChanged(object? sender, RuntimeHealth health)
    {
        var app = System.Windows.Application.Current as App;
        switch (health)
        {
            case RuntimeHealth.Healthy:
                app?.Notify("CoKernel", "Runtime is healthy.", Forms.ToolTipIcon.Info);
                break;
            case RuntimeHealth.Degraded:
                app?.Notify("CoKernel degraded", "One or more runtime components are unhealthy. Open CoKernel for details.", Forms.ToolTipIcon.Warning);
                break;
            case RuntimeHealth.Error:
                app?.Notify("CoKernel error", "The runtime needs attention. Open CoKernel for diagnostics.", Forms.ToolTipIcon.Error);
                break;
        }
    }

    private void OnClosing(object? sender, CancelEventArgs e)
    {
        if (AllowClose) return;
        e.Cancel = true;
        Hide();
    }
}
