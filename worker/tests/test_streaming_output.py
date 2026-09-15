"""Production output tests: order, bounded retention, interruption and live IPC."""
from __future__ import annotations

from contextlib import contextmanager
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time

import pytest
from IPython.core.interactiveshell import InteractiveShell

from cokernel_worker.execution import ExecutionEngine
from cokernel_worker.output_limits import OutputLimits, encode_json
from cokernel_worker.protocol import WorkerLoop, receive_frame, send_frame


@pytest.fixture(autouse=True)
def isolated_shell():
    InteractiveShell.clear_instance()
    yield
    InteractiveShell.clear_instance()


def record(source, *, limits=None, engine=None):
    frames = []
    worker = WorkerLoop("stream-test", engine=engine, output_limits=limits)
    # Round-trip real JSON: retaining user objects here would hide capture leaks.
    worker._send_message = lambda sock, frame: frames.append(json.loads(encode_json(frame)))
    worker._execute(None, "op", {"operation_id": "op", "source": source})
    check_lifecycle(frames)
    return worker, frames


def outputs(frames):
    return [f for f in frames if f.get("event") in {
        "stdout", "stderr", "display_data", "execute_result", "error"
    }]


def check_lifecycle(frames):
    frames = [f for f in frames if f.get("event") != "heartbeat"]
    assert frames[0]["event"] == "execution_started"
    assert frames[-2]["event"] == "execution_finished"
    assert frames[-1]["type"] == "response"
    out = outputs(frames)
    assert [f["payload"]["sequence"] for f in out] == list(range(1, len(out) + 1))
    finish = frames[-2]["payload"]
    result = frames[-1]["result"]
    assert finish["output_count"] == len(out)
    for field in ("status", "execution_count", "output_truncated", "output_omitted_bytes",
                  "output_truncation_reasons"):
        assert finish[field] == result[field], field
    for frame in out:
        if frame["event"] == "execute_result":
            assert frame["payload"]["execution_count"] == result["execution_count"]
    assert finish["output_truncation_reasons"] == sorted(set(finish["output_truncation_reasons"]))
    if not finish["output_truncated"]:
        assert finish["output_omitted_bytes"] == 0
        assert finish["output_truncation_reasons"] == []
    else:
        assert finish["output_truncation_reasons"]


def test_production_preserves_mixed_output_order_and_execution_count():
    _, frames = record("import sys\nfrom IPython.display import display\n"
                      "sys.stdout.write('a')\nsys.stderr.write('b')\n"
                      "display({'text/plain':'c'}, raw=True)\n"
                      "sys.stdout.write('d')\n42")
    assert [f['event'] for f in outputs(frames)] == [
        "stdout", "stderr", "display_data", "stdout", "execute_result"
    ]
    assert [f['payload'].get('text') for f in outputs(frames) if 'text' in f['payload']] == ['a','b','d']
    assert frames[-1]['result']['status'] == 'SUCCEEDED'


def test_explicit_flush_delivers_before_execute_returns():
    observed = []
    engine = ExecutionEngine()
    engine.shell.user_ns['observed'] = observed
    result = engine.execute("print('progress', end='', flush=True)\n"
                            "assert observed == [('stdout', 'progress')]",
                            output_sink=lambda e, p: observed.append((e, p.get('text'))))
    assert result.success, result.error
    assert result.stdout == '' and result.displays == [] and result.final_result is None


def test_binary_mime_and_metadata_are_normalized_before_live_send():
    _, frames = record("from IPython.display import display\n"
                      "display({'image/png': b'png', 'image/svg+xml': b'<svg/>'}, "
                      "raw=True, metadata={'tag': 1})")
    payload = outputs(frames)[0]['payload']
    assert payload['data'] == {'image/png':'cG5n', 'image/svg+xml':'<svg/>'}
    assert payload['metadata'] == {'tag': 1}


@pytest.mark.parametrize('bad', ["{'image/svg+xml': b'\\xff'}", "{'application/json': 1 << 80}",
                                "{'application/json': {1: 'lossy'}}", "{'application/json': float('nan')}"])
def test_bad_live_output_has_terminal_failure_and_same_engine_survives(bad):
    worker, frames = record(f"from IPython.display import display\nmarker = 41\ndisplay({bad}, raw=True)")
    assert frames[-1]['result']['status'] == 'FAILED'
    assert any(f['event'] == 'error' for f in outputs(frames))
    assert worker.engine.execute('marker + 1').final_result.data['text/plain'] == '42'


def test_rich_display_flood_does_not_retain_user_payloads_until_cell_end():
    worker, frames = record("from IPython.display import display\nimport weakref, gc\n"
        "class Payload(str): pass\nrefs = []\n"
        "for i in range(200):\n"
        "    data = Payload('x' * 16384)\n    refs.append(weakref.ref(data))\n"
        "    display({'text/plain': data}, raw=True)\n"
        "gc.collect()\nsurvivors = sum(ref() is not None for ref in refs)",
        limits=OutputLimits(max_event_bytes=4096, max_operation_bytes=8192, max_blob_bytes=2048))
    assert worker.engine.shell.user_ns['survivors'] <= 1
    assert frames[-1]['result']['output_truncated']
    assert sum(len(encode_json(f)) for f in outputs(frames)) <= 8192


@pytest.mark.parametrize('text', ['x', 'あ', '🌟', '\\', '\n', '\x00'])
def test_stream_flood_respects_wire_and_operation_limits(text):
    limits = OutputLimits(max_event_bytes=1200, max_operation_bytes=4000, max_blob_bytes=500)
    _, frames = record(f"import sys\nsys.stdout.write({text!r} * 100000)\nNone", limits=limits)
    assert frames[-1]['result']['output_truncated']
    assert frames[-1]['result']['output_omitted_bytes'] > 0
    assert all(len(encode_json(f)) <= 1200 for f in outputs(frames))
    assert sum(len(encode_json(f)) for f in outputs(frames)) <= 4000


def test_source_utf8_prefix_does_not_resume_after_partial_character():
    _, frames = record("import sys\nsys.stdout.write('あいう')\nsys.stdout.write('Z')\nNone",
                      engine=ExecutionEngine(max_stream_capture_bytes=7))
    assert ''.join(f['payload']['text'] for f in outputs(frames) if f['event'] == 'stdout') == 'あい'
    assert frames[-1]['result']['output_omitted_bytes'] == 4
    assert frames[-1]['result']['output_truncation_reasons'] == ['stream_capture_limit']


def test_zero_source_budget_reports_omission_without_empty_output_flood():
    _, frames = record("print('abc')", engine=ExecutionEngine(max_stream_capture_bytes=0))
    assert outputs(frames) == []
    assert frames[-1]['result']['output_omitted_bytes'] == 4


def test_production_does_not_call_user_deepcopy_hooks_for_rich_payload():
    worker, frames = record("from IPython.display import display\n"
        "class Text(str):\n"
        "    def __deepcopy__(self, memo): raise RuntimeError('deepcopy must not run')\n"
        "display({'text/plain': Text('safe')}, raw=True)")
    assert frames[-1]['result']['status'] == 'SUCCEEDED'
    assert outputs(frames)[0]['payload']['data']['text/plain'] == 'safe'


def test_closed_capture_references_cannot_publish_into_next_operation():
    events = []
    engine = ExecutionEngine()
    old_publisher = engine.shell.display_pub
    result = engine.execute("import sys\nsaved_stream=sys.stdout\nsaved_pub=get_ipython().display_pub\n"
                            "print('first')", output_sink=lambda e,p: events.append((e,p)))
    assert result.success
    before = len(events)
    engine.shell.user_ns['saved_stream'].write('late')
    engine.shell.user_ns['saved_pub'].publish({'text/plain': 'late'})
    assert len(events) == before
    assert engine.shell.display_pub is old_publisher


@pytest.mark.parametrize('source', ['42;', "import asyncio\nawait asyncio.sleep(0)\n42",
                                    "raise ValueError('boom')", 'if:'])
def test_live_execution_preserves_ipython_semantics(source):
    worker, frames = record(source)
    if 'raise' in source or source == 'if:':
        assert frames[-1]['result']['status'] == 'FAILED'
        assert [f['event'] for f in outputs(frames)] == ['error']
    elif source == '42;':
        assert outputs(frames) == []
    else:
        assert outputs(frames)[-1]['payload']['data']['text/plain'] == '42'
        assert worker.engine.shell.user_ns['_'] == 42


@pytest.mark.parametrize('source', ["from IPython.display import clear_output\nclear_output()",
    "from IPython.display import display, update_display\n"
    "display('a', display_id='test')\nupdate_display('b', display_id='test')"])
def test_unsupported_display_mutations_fail_explicitly_without_ansi(source):
    worker, frames = record(source)
    assert frames[-1]['result']['status'] == 'FAILED'
    assert not any('\x1b' in f['payload'].get('text','') for f in outputs(frames))
    assert worker.engine.execute('40+2').success


def test_threaded_live_writes_keep_sequence_and_terminal_accounting():
    _, frames = record("import threading, sys\n"
        "def emit():\n    for _ in range(40): sys.stdout.write('x')\n"
        "threads = [threading.Thread(target=emit) for _ in range(4)]\n"
        "for t in threads: t.start()\nfor t in threads: t.join()")
    assert ''.join(f['payload']['text'] for f in outputs(frames)) == 'x' * 160


@pytest.mark.skipif(os.name != 'posix', reason='SIGINT is a Linux/WSL worker contract')
def test_sigint_between_frame_write_and_sequence_commit_is_deferred():
    frames = []
    worker = WorkerLoop('sigint')
    fired = False
    previous = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGINT, signal.default_int_handler)
    def send(sock, frame):
        nonlocal fired
        frames.append(json.loads(encode_json(frame)))
        if frame.get('event') == 'stdout' and not fired:
            fired = True
            signal.raise_signal(signal.SIGINT)
    worker._send_message = send
    try:
        worker._execute(None, 'op', {'operation_id':'op', 'source':"marker=41\nprint('interrupt me')"})
        assert signal.getsignal(signal.SIGINT) is signal.default_int_handler
    finally:
        signal.signal(signal.SIGINT, previous)
    check_lifecycle(frames)
    assert frames[-1]['result']['status'] == 'FAILED'
    assert outputs(frames)[-1]['payload']['ename'] == 'KeyboardInterrupt'
    assert worker.engine.execute('marker+1').final_result.data['text/plain'] == '42'


@contextmanager
def real_worker():
    if not hasattr(socket, 'AF_UNIX'):
        pytest.skip('AF_UNIX worker requires Linux/WSL')
    with tempfile.TemporaryDirectory(prefix='ck-live-') as root:
        path = str(Path(root) / 'worker.sock')
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(path)
            listener.listen(1)
            listener.settimeout(10)
            env = dict(os.environ)
            source_root = str(Path(__file__).resolve().parents[1] / 'src')
            env['PYTHONPATH'] = source_root + os.pathsep + env.get('PYTHONPATH','')
            with subprocess.Popen([sys.executable, '-m', 'cokernel_worker', '--socket', path,
                                   '--session-id', 'real-live'], env=env,
                                  stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE) as process:
                try:
                    conn, _ = listener.accept()
                    with conn:
                        conn.settimeout(10)
                        ready = receive_frame(conn)
                        assert ready['payload']['pid'] == process.pid
                        request(conn, 'handshake', 'handshake', {})
                        handshake = read_until_response(conn)[-1]['result']
                        assert handshake['protocol'] == 1
                        assert 'execute' in handshake['capabilities']
                        yield conn, process, Path(root)
                finally:
                    if process.poll() is None:
                        process.terminate()
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=3)


def request(conn, rid, method, payload):
    send_frame(conn, {'protocol':1,'type':'request','session_id':'real-live',
                     'id':rid,'method':method,'payload':payload})


def read_until_response(conn):
    frames=[]
    deadline = time.monotonic() + 10
    while True:
        assert time.monotonic() < deadline, "worker response deadline exceeded"
        frame=receive_frame(conn)
        assert frame is not None
        frames.append(frame)
        if frame['type']=='response': return frames


def test_real_worker_stream_arrives_before_cell_can_finish():
    with real_worker() as (conn, process, root):
        release = root / 'release'
        request(conn, 'op', 'execute', {'operation_id':'op','source':
            "import pathlib, time\nprint('progress', flush=True)\n"
            f"while not pathlib.Path({str(release)!r}).exists(): time.sleep(0.01)\n"
            "marker=41\nmarker+1"})
        seen=[]
        deadline = time.monotonic() + 10
        while True:
            assert time.monotonic() < deadline, "no live stream before deadline"
            frame=receive_frame(conn)
            seen.append(frame)
            assert frame['type'] != 'response', 'cell completed before release'
            if frame.get('event')=='stdout': break
        assert process.poll() is None and not release.exists()
        release.touch()
        seen += read_until_response(conn)
        check_lifecycle(seen)
        assert seen[-1]['result']['status']=='SUCCEEDED'
        request(conn,'inspect','get_variable',{'name':'marker'})
        assert read_until_response(conn)[-1]['result']['value']==41
        request(conn,'shutdown','shutdown',{})
        assert read_until_response(conn)[-1]['result']['shutdown'] is True
        assert process.wait(timeout=3)==0


@pytest.mark.skipif(os.name != 'posix', reason='SIGINT is a Linux/WSL worker contract')
def test_real_worker_interrupts_after_streaming_and_reuses_same_namespace():
    with real_worker() as (conn, process, root):
        request(conn,'op','execute',{'operation_id':'op','source':
            "import time\nmarker=41\nprint('ready',flush=True)\nwhile True: time.sleep(0.01)"})
        frames=[]
        deadline = time.monotonic() + 10
        while True:
            assert time.monotonic() < deadline, "no live stream before deadline"
            f=receive_frame(conn); frames.append(f)
            if f.get('event')=='stdout': break
        process.send_signal(signal.SIGINT)
        frames += read_until_response(conn)
        check_lifecycle(frames)
        assert frames[-1]['result']['status']=='FAILED'
        request(conn,'op2','execute',{'operation_id':'op2','source':'marker+1'})
        survivor=read_until_response(conn)
        check_lifecycle(survivor)
        assert outputs(survivor)[0]['payload']['data']['text/plain']=='42'
        assert process.poll() is None


def test_wire_truncation_does_not_resume_stream_after_omitted_bytes():
    text = "abcd界" * 5000
    _, frames = record(f"import sys\nsys.stdout.write({text!r})\nsys.stdout.write('TAIL')\nNone",
        limits=OutputLimits(max_event_bytes=600, max_operation_bytes=5000, max_blob_bytes=128))
    emitted = "".join(f["payload"]["text"] for f in outputs(frames) if f["event"] == "stdout")
    assert emitted and text.startswith(emitted)
    assert "TAIL" not in emitted
    assert frames[-1]["result"]["output_omitted_bytes"] == len((text + "TAIL").encode()) - len(emitted.encode())


def test_real_worker_flush_output_survives_abrupt_process_exit():
    with real_worker() as (conn, process, root):
        request(conn, 'crash', 'execute', {'operation_id': 'crash', 'source':
            "import os\nprint('before-crash', flush=True)\nos._exit(23)"})
        frames = []
        deadline = time.monotonic() + 10
        while True:
            assert time.monotonic() < deadline, "crashing worker failed to exit"
            frame = receive_frame(conn)
            if frame is None:
                break
            frames.append(frame)
        assert process.wait(timeout=3) == 23
        assert ''.join(f['payload']['text'] for f in frames if f.get('event') == 'stdout') == 'before-crash\n'
        assert not any(f.get('event') == 'execution_finished' for f in frames)
