using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace CoKernel.Desktop.Services;

public sealed class DesktopSettings
{
    public bool AutoStart { get; set; } = true;
    public string DesiredRuntimeState { get; set; } = "RUNNING";
    public string TunnelId { get; set; } = "";
    public string ProtectedTunnelApiKey { get; set; } = "";
}

public sealed class SettingsStore
{
    private readonly string _path;
    private static readonly JsonSerializerOptions JsonOptions = new() { WriteIndented = true };

    public SettingsStore()
    {
        var dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "CoKernel");
        Directory.CreateDirectory(dir);
        _path = Path.Combine(dir, "desktop-settings.json");
    }

    public DesktopSettings Load()
    {
        try
        {
            return File.Exists(_path)
                ? JsonSerializer.Deserialize<DesktopSettings>(File.ReadAllText(_path)) ?? new DesktopSettings()
                : new DesktopSettings();
        }
        catch { return new DesktopSettings(); }
    }

    public void Save(DesktopSettings settings) => File.WriteAllText(_path, JsonSerializer.Serialize(settings, JsonOptions));

    public static string ProtectSecret(string value)
    {
        if (string.IsNullOrEmpty(value)) return "";
        var bytes = ProtectedData.Protect(Encoding.UTF8.GetBytes(value), null, DataProtectionScope.CurrentUser);
        return Convert.ToBase64String(bytes);
    }

    public static string UnprotectSecret(string protectedValue)
    {
        if (string.IsNullOrWhiteSpace(protectedValue)) return "";
        try
        {
            var bytes = ProtectedData.Unprotect(Convert.FromBase64String(protectedValue), null, DataProtectionScope.CurrentUser);
            return Encoding.UTF8.GetString(bytes);
        }
        catch { return ""; }
    }
}
