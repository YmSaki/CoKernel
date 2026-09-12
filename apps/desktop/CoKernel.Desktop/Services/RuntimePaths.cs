using System.IO;

namespace CoKernel.Desktop.Services;

public sealed class RuntimePaths
{
    public RuntimePaths(string root) => Root = root;

    public string Root { get; }
    public string RuntimeScript => Path.Combine(Root, "runtime.ps1");
    public string UpdateScript => Path.Combine(Root, "update.ps1");
    public string InstallCommand => Path.Combine(Root, "install.cmd");

    public static RuntimePaths Resolve()
    {
        var overrideRoot = Environment.GetEnvironmentVariable("COKERNEL_RUNTIME_ROOT");
        if (!string.IsNullOrWhiteSpace(overrideRoot) && IsRuntimeRoot(overrideRoot))
            return new RuntimePaths(Path.GetFullPath(overrideRoot));

        var packaged = Path.Combine(AppContext.BaseDirectory, "runtime");
        if (IsRuntimeRoot(packaged))
            return new RuntimePaths(packaged);

        var current = new DirectoryInfo(AppContext.BaseDirectory);
        for (var i = 0; i < 10 && current is not null; i++, current = current.Parent)
        {
            if (IsRuntimeRoot(current.FullName))
                return new RuntimePaths(current.FullName);
        }

        throw new DirectoryNotFoundException(
            "Could not locate the CoKernel runtime. Set COKERNEL_RUNTIME_ROOT or install the desktop package with its runtime payload.");
    }

    private static bool IsRuntimeRoot(string path) =>
        File.Exists(Path.Combine(path, "install.ps1")) && File.Exists(Path.Combine(path, "compose.yaml"));
}
