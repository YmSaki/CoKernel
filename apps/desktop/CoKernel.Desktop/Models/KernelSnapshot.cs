namespace CoKernel.Desktop.Models;

public sealed record KernelSnapshot(
    string NotebookPath,
    string KernelId,
    string KernelName,
    string ExecutionState,
    int Connections,
    DateTimeOffset? LastActivity);
