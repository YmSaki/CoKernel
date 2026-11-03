from pathlib import Path

from cokernel_worker import __main__ as cli


def test_configured_profile_exists_before_worker_initialization(monkeypatch, tmp_path):
    profile = tmp_path / "project" / ".cokernel" / "ipython"
    monkeypatch.setenv("IPYTHONDIR", str(profile))
    monkeypatch.setattr("sys.argv", ["cokernel-worker", "--socket", "test.sock", "--session-id", "test"])

    def connect(socket, session):
        assert (socket, session) == ("test.sock", "test")
        assert Path(profile).is_dir()
        (profile / "proof").write_text("workload can write", encoding="utf-8")

    monkeypatch.setattr(cli, "connect_and_run", connect)
    assert cli.main() == 0
    assert (profile / "proof").is_file()
