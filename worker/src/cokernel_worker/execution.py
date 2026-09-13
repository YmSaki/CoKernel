from __future__ import annotations

from dataclasses import dataclass, field
import traceback
from typing import Any

from IPython.core.interactiveshell import ExecutionResult, InteractiveShell
from IPython.utils.capture import RichOutput, capture_output


@dataclass(slots=True)
class MimeBundle:
    data: dict[str, Any]
    metadata: dict[str, Any] = field(default_factory=dict)
    transient: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_rich_output(cls, output: RichOutput) -> "MimeBundle":
        return cls(
            data=dict(output.data),
            metadata=dict(output.metadata or {}),
            transient=dict(getattr(output, "transient", None) or {}),
        )


@dataclass(slots=True)
class ExecutionError:
    name: str
    value: str
    traceback: list[str]


@dataclass(slots=True)
class ExecutionOutcome:
    success: bool
    execution_count: int | None
    stdout: str
    stderr: str
    displays: list[MimeBundle]
    final_result: MimeBundle | None
    error: ExecutionError | None


class ExecutionEngine:
    """Persistent IPython execution state for exactly one CoKernel Session.

    Production creates one engine inside one supervised worker process. The
    Session Supervisor, not this class, provides concurrency control and crash
    containment.
    """

    def __init__(self, shell: InteractiveShell | None = None) -> None:
        self.shell = shell or InteractiveShell.instance()
        self.shell.autoawait = True

    def execute(self, source: str, *, cell_id: str | None = None) -> ExecutionOutcome:
        """Execute one cell using IPython semantics and capture notebook output."""

        with capture_output(stdout=True, stderr=True, display=True) as captured:
            result = self.shell.run_cell(
                source,
                store_history=True,
                silent=False,
                cell_id=cell_id,
            )

        return self._outcome(result, captured.stdout, captured.stderr, captured.outputs)

    def reset(self) -> None:
        """Discard user namespace/history while keeping the worker process alive."""

        self.shell.reset(new_session=True)

    def _outcome(
        self,
        result: ExecutionResult,
        stdout: str,
        stderr: str,
        outputs: list[RichOutput],
    ) -> ExecutionOutcome:
        error = result.error_before_exec or result.error_in_exec
        final_result: MimeBundle | None = None

        if error is None and result.result is not None:
            data, metadata = self.shell.display_formatter.format(result.result)
            final_result = MimeBundle(data=dict(data), metadata=dict(metadata or {}))

        execution_error: ExecutionError | None = None
        if error is not None:
            execution_error = ExecutionError(
                name=type(error).__name__,
                value=str(error),
                traceback=traceback.format_exception(type(error), error, error.__traceback__),
            )

        return ExecutionOutcome(
            success=error is None,
            execution_count=getattr(result, "execution_count", None),
            stdout=stdout,
            stderr=stderr,
            displays=[MimeBundle.from_rich_output(output) for output in outputs],
            final_result=final_result,
            error=execution_error,
        )
