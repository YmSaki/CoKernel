"""Compare recorded collector/Host observations to independent OS/NVIDIA queries."""
import json
from pathlib import Path

root = Path(__file__).resolve().parent
prefix = "phase5-metrics-20260922."


def objects(suffix):
    return [json.loads(line) for line in (root / (prefix + suffix)).read_text(
        encoding="utf-8-sig").splitlines() if line.startswith("{")]


refs = {name: objects(name + "-reference.json")[0] for name in ("windows", "linux")}
samples = {name: objects(name + ".txt")[0] for name in refs}
host_records = objects("real.txt")
aggregate = next(row["resource_snapshot"] for row in host_records if "resource_snapshot" in row)
assert aggregate["runtime_state"] == "HEALTHY"
assert aggregate["runtime_generation"] >= 1
assert aggregate["linux_unavailable"] is None
assert aggregate["storage_scope"] == {"host": "HOST_STATE_VOLUME", "linux": "MANAGED_PROJECTS_FILESYSTEM"}
for name in refs:
    total_key = "physical_memory_total_bytes" if name == "windows" else "linux_memory_total_bytes"
    ref = refs[name]
    uuid, gpu_name, mib = (field.strip() for field in ref["gpu"].split(","))
    assert ref[total_key] > 0
    for sample in (samples[name], aggregate["host" if name == "windows" else "linux"]):
        assert sample["sequence"] >= 2 and sample["stale"] is False
        assert 0 <= sample["age_ms"] <= 15000 and sample["collecting"] is True
        resource = sample["resources"]
        assert resource["source"] == ("WINDOWS_HOST" if name == "windows" else "LINUX_RUNTIME")
        assert resource["memory"]["value"]["total_bytes"] == ref[total_key]
        assert 0 <= resource["cpu_percent"]["value"] <= 100
        assert resource["storage"]["value"]["total_bytes"] > 0
        gpu = next(gpu for gpu in resource["gpus"]["value"] if gpu["uuid"] == uuid)
        assert gpu["name"] == gpu_name
        assert gpu["memory_total_bytes"]["value"] == int(mib) * 1048576
        assert resource["process_gpu_attribution"] == {"value": None, "unavailable": "NOT_SUPPORTED"}
print("PASS: independent Windows CIM/Linux procfs RAM and NVIDIA UUID/VRAM agree with both collectors and actual Host aggregation; sources and unavailable attribution preserved")
