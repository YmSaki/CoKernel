using System.IO.Compression;
using System.Text.RegularExpressions;

namespace CoKernel.Desktop.Services;

public sealed class DiagnosticsService
{
    private readonly RuntimePaths _paths;
    private readonly CommandRunner _runner;

    public DiagnosticsService(RuntimePaths paths, CommandRunner runner)
    {
        _paths = paths;
        _runner = runner;
    }

    public async Task<string> ExportAsync(CancellationToken cancellationToken = default)
    {
        var stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss");
        var temp = Path.Combine(Path.GetTempPath(), "CoKernel-Diagnostics-" + stamp);
        Directory.CreateDirectory(temp);

        try
        {
            await Capture(temp, "wsl-version.txt", "wsl.exe", new[] { "--version" }, cancellationToken);
            await Capture(temp, "wsl-list.txt", "wsl.exe", new[] { "--list", "--verbose" }, cancellationToken);
            await Capture(temp, "nvidia-smi.txt", "nvidia-smi", Array.Empty<string>(), cancellationToken);
            await Capture(temp, "runtime-status.txt", "powershell.exe", new[]
            {
                "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", _paths.RuntimeScript, "-Action", "Status"
            }, cancellationToken);
            await Capture(temp, "doctor.txt", "wsl.exe", new[]
            {
                "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc", "cd ~/src/CoKernel && ./scripts/doctor.sh"
            }, cancellationToken);
            await Capture(temp, "compose-ps.txt", "wsl.exe", new[]
            {
                "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc", "cd ~/src/CoKernel && docker compose --profile tunnel ps"
            }, cancellationToken);
            await Capture(temp, "compose-logs.txt", "wsl.exe", new[]
            {
                "-d", "CoKernel", "--cd", "/", "--", "bash", "-lc", "cd ~/src/CoKernel && docker compose --profile tunnel logs --tail=300"
            }, cancellationToken);

            var desktop = Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory);
            var zip = Path.Combine(desktop, "CoKernel-Diagnostics-" + stamp + ".zip");
            if (File.Exists(zip)) File.Delete(zip);
            ZipFile.CreateFromDirectory(temp, zip, CompressionLevel.Fastest, includeBaseDirectory: false);
            return zip;
        }
        finally
        {
            try { Directory.Delete(temp, recursive: true); } catch { }
        }
    }

    private async Task Capture(string dir, string name, string exe, IEnumerable<string> args, CancellationToken cancellationToken)
    {
        try
        {
            var result = await _runner.RunAsync(exe, args, _paths.Root, TimeSpan.FromSeconds(30), cancellationToken: cancellationToken);
            await File.WriteAllTextAsync(Path.Combine(dir, name), Redact(result.Combined), cancellationToken);
        }
        catch (Exception ex)
        {
            await File.WriteAllTextAsync(Path.Combine(dir, name), Redact(ex.ToString()), cancellationToken);
        }
    }

    private static string Redact(string text)
    {
        text = Regex.Replace(text, @"(?im)^(CONTROL_PLANE_API_KEY|MCP_TOKEN|JUPYTER_TOKEN)\s*=.*$", "$1=<redacted>");
        text = Regex.Replace(text, @"\bsk-[A-Za-z0-9_\-]{8,}\b", "sk-<redacted>");
        text = Regex.Replace(text, @"(?i)Bearer\s+[A-Za-z0-9._\-]+", "Bearer <redacted>");
        return text;
    }
}
