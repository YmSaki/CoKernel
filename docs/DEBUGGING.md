# CoKernel installation debugging

CoKernel's Windows installer is designed to be rerun safely. It never unregisters a WSL distribution automatically.

## Diagnostic log

The elevated Windows installer writes its combined output to:

```text
%LOCALAPPDATA%\CoKernel\install.log
```

`install.cmd` prints the tail of this file on failure.

## WSL bootstrap steps

Linux bootstrap operations are executed as individually labeled WSL steps. A failure identifies the step name and exit code, for example:

```text
WSL step 'updating Ubuntu package metadata' failed for user 'root' with exit code 100.
```

Use the immediately preceding command output in `install.log` as the authoritative error detail.

Do not unregister the `CoKernel` distro as a first response. Fix the reported failing step, update the repository if a CoKernel change is available, and rerun `install.cmd`.
