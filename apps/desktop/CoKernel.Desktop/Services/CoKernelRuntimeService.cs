using System.Diagnostics;
using System.Net.Http;
using System.Net.Sockets;
using CoKernel.Desktop.Models;

namespace CoKernel.Desktop.Services;

public sealed class CoKernelRuntimeService
{
    private readonly RuntimePaths _paths;
    private readonly CommandRunner _runner;
    private readonly HttpClient _http = new() { Timeout = TimeSpan.FromSeconds(3) };

    public CoKernelRuntimeService(RuntimePaths paths, CommandRunner runner)
    {
        _paths = paths;
        _runner = runner;
    }

    public async Task<CommandResult> StartAsync(CancellationToken cancellationToken = default)
    {
        var keeper = await _runner.RunAsync("powershell.exe", new[]
        {
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", _paths.RuntimeScript,
            "-Action", "Start"
        }, _paths.Root, TimeSpan.FromSeconds(20), cancellationToken: cancellationToken);
        if (!keeper.Success) return keeper;

        return await _runner.RunAsync("wsl.exe", new[]
        {
            "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc",
            "cd ~/src/CoKernel && ./up.sh"
        }, timeout: TimeSpan.FromMinutes(4), cancellationToken: cancellationToken);
    }

    public Task<CommandResult> StopAsync(CancellationToken cancellationToken = default) =>
        _runner.RunAsync("powershell.exe", new[]
        {
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", _paths.RuntimeScript,
            "-Action", "Stop"
        }, _paths.Root, TimeSpan.FromMinutes(2), cancellationToken: cancellationToken);

    public async Task<CommandResult> RestartAsync(CancellationToken cancellationToken = default)
    {
        var stop = await StopAsync(cancellationToken);
        if (!stop.Success) return stop;
        return await StartAsync(cancellationToken);
    }

    public Task<CommandResult> UpdateAsync(CancellationToken cancellationToken = default) =>
        _runner.RunAsync("powershell.exe", new[]
        {
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", _paths.UpdateScript
        }, _paths.Root, TimeSpan.FromMinutes(15), cancellationToken: cancellationToken);

    public void LaunchRepair()
    {
        Process.Start(new ProcessStartInfo
        {
            FileName = _paths.InstallCommand,
            WorkingDirectory = _paths.Root,
            UseShellExecute = true
        });
    }

    public void OpenJupyter() => Process.Start(new ProcessStartInfo
    {
        FileName = "http://127.0.0.1:8888",
        UseShellExecute = true
    });

    public async Task<ComponentSnapshot> GetStatusAsync(CancellationToken cancellationToken = default)
    {
        var list = await _runner.RunAsync("wsl.exe", new[] { "--list", "--running", "--quiet" },
            timeout: TimeSpan.FromSeconds(5), cancellationToken: cancellationToken);
        var wslRunning = list.Success && list.StandardOutput.Replace("\0", "").Split('\n', StringSplitOptions.RemoveEmptyEntries)
            .Any(x => x.Trim().Equals("CoKernel", StringComparison.OrdinalIgnoreCase));

        var dockerReady = false;
        if (wslRunning)
        {
            var docker = await _runner.RunAsync("wsl.exe", new[]
            {
                "-d", "CoKernel", "--cd", "/", "--", "docker", "info"
            }, timeout: TimeSpan.FromSeconds(5), cancellationToken: cancellationToken);
            dockerReady = docker.Success;
        }

        var jupyter = await TestTcpAsync(8888, cancellationToken);
        var mcp = await TestTcpAsync(4040, cancellationToken);
        var tunnelConfigured = await IsTunnelConfiguredAsync(cancellationToken);
        var tunnelReady = tunnelConfigured && await TestTunnelReadyAsync(cancellationToken);
        var kernels = jupyter ? await GetActiveKernelCountAsync(cancellationToken) : 0;

        return new ComponentSnapshot(wslRunning, dockerReady, jupyter, mcp, tunnelConfigured, tunnelReady, kernels);
    }

    public async Task<int> GetActiveKernelCountAsync(CancellationToken cancellationToken = default)
    {
        var result = await _runner.RunAsync("wsl.exe", new[]
        {
            "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc",
            "cd ~/src/CoKernel && docker compose exec -T jupyter python -c \"import json,os,urllib.request; print(len(json.load(urllib.request.urlopen('http://127.0.0.1:8888/api/sessions?token='+os.environ['JUPYTER_TOKEN'], timeout=3))))\""
        }, timeout: TimeSpan.FromSeconds(8), cancellationToken: cancellationToken);

        return result.Success && int.TryParse(result.StandardOutput.Trim(), out var count) ? count : 0;
    }

    public Task<CommandResult> GetLogsAsync(int lines = 200, CancellationToken cancellationToken = default) =>
        _runner.RunAsync("wsl.exe", new[]
        {
            "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc",
            $"cd ~/src/CoKernel && docker compose --profile tunnel logs --tail={Math.Clamp(lines, 20, 2000)}"
        }, timeout: TimeSpan.FromSeconds(15), cancellationToken: cancellationToken);

    public Task<CommandResult> ConfigureTunnelAsync(string tunnelId, string apiKey, CancellationToken cancellationToken = default)
    {
        if (tunnelId.Contains('\n') || tunnelId.Contains('\r') || apiKey.Contains('\n') || apiKey.Contains('\r'))
            throw new ArgumentException("Tunnel credentials must be single-line values.");

        return _runner.RunAsync("wsl.exe", new[]
        {
            "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc",
            "cd ~/src/CoKernel && ./scripts/set-tunnel-config.sh"
        }, timeout: TimeSpan.FromSeconds(10), standardInput: tunnelId + "\n" + apiKey + "\n", cancellationToken: cancellationToken);
    }

    private async Task<bool> IsTunnelConfiguredAsync(CancellationToken cancellationToken)
    {
        if (!await TestTcpAsync(8080, cancellationToken))
        {
            var result = await _runner.RunAsync("wsl.exe", new[]
            {
                "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc",
                "cd ~/src/CoKernel && awk -F= '$1==\"CONTROL_PLANE_TUNNEL_ID\" && length($2)>0 {found=1} END {exit found?0:1}' .env"
            }, timeout: TimeSpan.FromSeconds(4), cancellationToken: cancellationToken);
            return result.Success;
        }
        return true;
    }

    private async Task<bool> TestTunnelReadyAsync(CancellationToken cancellationToken)
    {
        try
        {
            using var response = await _http.GetAsync("http://127.0.0.1:8080/readyz", cancellationToken);
            return response.IsSuccessStatusCode;
        }
        catch { return false; }
    }

    private static async Task<bool> TestTcpAsync(int port, CancellationToken cancellationToken)
    {
        try
        {
            using var tcp = new TcpClient();
            using var cts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            cts.CancelAfter(TimeSpan.FromSeconds(2));
            await tcp.ConnectAsync("127.0.0.1", port, cts.Token);
            return true;
        }
        catch { return false; }
    }
}
