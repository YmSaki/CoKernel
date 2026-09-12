using System.Drawing;
using System.Windows;
using Forms = System.Windows.Forms;

namespace CoKernel.Desktop;

public partial class App : System.Windows.Application
{
    private Forms.NotifyIcon? _tray;
    private MainWindow? _window;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        ShutdownMode = ShutdownMode.OnExplicitShutdown;

        _window = new MainWindow();
        _tray = new Forms.NotifyIcon
        {
            Icon = SystemIcons.Application,
            Text = "CoKernel",
            Visible = true
        };

        var menu = new Forms.ContextMenuStrip();
        menu.Items.Add("Open CoKernel", null, (_, _) => ShowWindow());
        menu.Items.Add("Open Jupyter", null, (_, _) => _window.OpenJupyter());
        menu.Items.Add(new Forms.ToolStripSeparator());
        menu.Items.Add("Start", null, async (_, _) => await _window.StartRuntimeAsync());
        menu.Items.Add("Stop", null, async (_, _) => await _window.StopRuntimeAsync());
        menu.Items.Add("Refresh", null, async (_, _) => await _window.RefreshAsync());
        menu.Items.Add(new Forms.ToolStripSeparator());
        menu.Items.Add("Exit UI", null, (_, _) => ExitUi());
        _tray.ContextMenuStrip = menu;
        _tray.DoubleClick += (_, _) => ShowWindow();

        var background = e.Args.Any(a => string.Equals(a, "--background", StringComparison.OrdinalIgnoreCase));
        if (!background)
            ShowWindow();
        else
            _ = _window.InitializeBackgroundAsync();
    }

    public void Notify(string title, string message, Forms.ToolTipIcon icon = Forms.ToolTipIcon.Info)
    {
        _tray?.ShowBalloonTip(4000, title, message, icon);
    }

    private void ShowWindow()
    {
        if (_window is null) return;
        _window.Show();
        if (_window.WindowState == WindowState.Minimized)
            _window.WindowState = WindowState.Normal;
        _window.Activate();
    }

    private void ExitUi()
    {
        if (_window is not null)
            _window.AllowClose = true;
        _tray?.Dispose();
        _tray = null;
        Shutdown();
    }
}
