"""Disposable framed process for Host failure/concurrency tests; no execution engine."""
import json
import hashlib
import os
from pathlib import Path
import struct
import sys
import time

mode, directory = sys.argv[1:]
directory = Path(directory)
with (directory / "starts").open("a") as handle:
    handle.write(str(os.getpid()) + "\n")
pending = None
sequence = 0
tunnel = dict(state="DISABLED", desired_enabled=False, applied_epoch=0, generation=0, pid=None, consecutive_failures=0)
credential_fingerprint = None


def send(value):
    data = json.dumps(value).encode()
    sys.stdout.buffer.write(struct.pack(">I", len(data)) + data)
    sys.stdout.buffer.flush()


def response(request, result):
    return {"protocol": 1, "request_id": request["request_id"], "ok": True, "result": result}


while True:
    length = sys.stdin.buffer.read(4)
    if not length:
        break
    request = json.loads(sys.stdin.buffer.read(struct.unpack(">I", length)[0]))
    method = request["method"]
    if method == "bridge.hello":
        if mode == "hang":
            time.sleep(30)
        if mode == "exit":
            sys.exit(3)
        if mode == "oversized":
            sys.stdout.buffer.write(struct.pack(">I", 0xFFFFFFFF))
            sys.stdout.buffer.flush()
            continue
        hello = response(request, {"selected_protocol": 2 if mode == "mismatch" else 1,
                                   "runtime_product_version": "fake-1", "capabilities": ["echo"] + (["linux-metrics"] if mode.startswith("metrics-") else [])})
        if mode == "bad-protocol":
            hello["protocol"] = 2
        if mode.startswith("early-"):
            for index in range(33 if mode == "early-flood" else 2):
                sequence += 1
                send({"protocol": 2 if mode == "early-protocol" else 1,
                      "event": "mcp.state_changed", "sequence": 1 if mode == "early-order" else sequence,
                      "payload": {"state": "STARTING"}})
        send(hello)
    elif method == "runtime.get_linux_metrics" and mode.startswith("metrics-"):
        if mode == "metrics-silent":
            continue  # Leave one reply pending, but keep control responsive.
        send(response(request, {"invalid": "not-a-resource-snapshot"}))
    elif method == "tunnel.start":
        params=request['params']
        fingerprint=hashlib.sha256(params['runtime_key'].encode()).hexdigest()
        if credential_fingerprint != fingerprint or not tunnel['desired_enabled']:
            tunnel.update(state="CONNECTED",desired_enabled=True,applied_epoch=tunnel['applied_epoch']+1,generation=tunnel['generation']+1,pid=os.getpid())
            credential_fingerprint=fingerprint
            with (directory/'handoffs.jsonl').open('a',encoding='utf-8') as handle:
                handle.write(json.dumps(dict(runtime_pid=os.getpid(),tunnel_id=params['tunnel_id'],fingerprint=fingerprint))+"\n")
        send(response(request,dict(accepted=True,epoch=tunnel['applied_epoch'])))
    elif method == "tunnel.stop":
        tunnel.update(state="STOPPING" if mode=='tunnel-stop-pending' else "DISABLED",desired_enabled=False,applied_epoch=tunnel['applied_epoch']+1,pid=os.getpid() if mode=='tunnel-stop-pending' else None)
        send(response(request,dict(accepted=True,epoch=tunnel['applied_epoch'])))
    elif method == "tunnel.restart":
        tunnel.update(applied_epoch=tunnel['applied_epoch']+1,generation=tunnel['generation']+1)
        send(response(request,dict(accepted=True,epoch=tunnel['applied_epoch'])))
    elif method == "runtime.get_tunnel_status":
        send(response(request,tunnel if mode!='tunnel-malformed' else dict(tunnel,state="CONNECTED",pid=None)))
    elif method == "bridge.shutdown":
        if mode == "refuse-shutdown":
            send({"protocol": 1, "request_id": request["request_id"], "ok": False,
                  "error": {"code": "CK-RUN-SHUTDOWN", "component": "runtime",
                            "summary": "unsaved document", "action": "resolve conflict", "detail": None}})
            continue
        if mode == "bad-shutdown":
            sys.exit(9)
        send(response(request, {"stopped": True}))
        break
    elif method == "crash":
        sys.exit(12)
    elif method == "allow-shutdown":
        mode = "normal"
        send(response(request, {}))
    elif mode == "import-refuse" and method.startswith("notebooks."):
        with (directory / "import-requests.jsonl").open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(request) + "\n")
        if method == "notebooks.begin_import":
            send(response(request, {"import_id":"ff253fd9-c275-4bdb-9cf2-4dab6bf12be0", "accepted_chunk_size":262144}))
        elif method == "notebooks.import_chunk":
            send({"protocol":1,"request_id":request["request_id"],"ok":False,
                  "error":{"code":"CK-NB-IMPORT","component":"notebook","summary":"test write failure","action":"retry after inspection","detail":None}})
        else:
            send(response(request, {"cancelled":True}))
    elif method == "hang":
        with (directory / "mutations").open("a") as handle:
            handle.write("executed\n")
        time.sleep(30)
    elif method == "slow" or (method == "packages.sync" and request["params"].get("test_slow")):
        (directory / "accepted").write_text("yes")
        time.sleep(0.2)
        (directory / "completed").write_text("yes")
        send(response(request, request["params"]))
    elif method == "packages.sync" and request["params"].get("test_hold"):
        (directory / "accepted").write_text("yes")
        deadline = time.monotonic() + 5
        while not (directory / "release").exists() and time.monotonic() < deadline:
            time.sleep(0.005)
        send(response(request, request["params"]))
    elif method == "pair":
        if pending is None:
            pending = request
        else:
            send(response(request, request["params"]))
            send(response(pending, pending["params"]))
            pending = None
    elif method == "wrong-id":
        send({**response(request, {}), "request_id": "not-issued"})
    elif method == "event":
        sequence = 1 if mode == "early-reset" else sequence + 1
        send({"protocol": 1, "event": "session.state_changed", "sequence": sequence, "payload": request["params"]})
        send(response(request, {}))
    elif method == "projects.list" and request["params"].get("test_events"):
        for index in range(64):
            sequence += 1
            send({"protocol":1,"event":"session.output","sequence":sequence,"payload":{"index":index}})
        send(response(request, {"events":64}))
    elif method == "stderr":
        sys.stderr.write("test-only-secret" * 10000)
        sys.stderr.flush()
        send(response(request, {}))
    elif method == "environment":
        send(response(request, {key: os.environ.get(key) for key in ["WSLENV", "COKERNEL_TEST_SECRET"]}))
    else:
        send(response(request, request["params"]))
(directory / "exited").write_text("yes")
