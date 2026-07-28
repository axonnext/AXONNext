"""Run the portable AXON 2026 release gate and record machine evidence."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import sysconfig
import time


ROOT = Path(__file__).resolve().parents[1]


def run(name, command):
    started = time.monotonic()
    environment = os.environ.copy()
    source = str(ROOT / "lib")
    existing = environment.get("PYTHONPATH")
    environment["PYTHONPATH"] = (
        source if not existing else source + os.pathsep + existing)
    if os.name != "nt":
        configured = environment.get("CC") or sysconfig.get_config_var("CC") or ""
        compiler = configured.split()[0] if configured.split() else ""
        if compiler and shutil.which(compiler) is None and shutil.which("cc"):
            environment["CC"] = "cc"
    process = subprocess.run(
        command, cwd=ROOT, env=environment, text=True, capture_output=True,
        check=False)
    duration = time.monotonic() - started
    if process.stdout:
        print(process.stdout, end="")
    if process.stderr:
        print(process.stderr, end="", file=sys.stderr)
    return {
        "name": name,
        "command": command,
        "returncode": process.returncode,
        "duration_seconds": round(duration, 6),
        "stdout_tail": process.stdout[-8000:],
        "stderr_tail": process.stderr[-8000:],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--fuzz-iterations", type=int, default=None)
    arguments = parser.parse_args()

    compile_targets = [
        target for target in (
            "lib", "bin", "examples", "setup.py", "test_all.py", "tools")
        if (ROOT / target).exists()
    ]
    commands = [
        ("build-extensions", [
            sys.executable, "setup.py", "build_ext", "--inplace",
        ]),
        ("compileall", [
            sys.executable, "-m", "compileall", "-q",
        ] + compile_targets),
        ("unittest", [sys.executable, "test_all.py"]),
        ("readme-doctest", [sys.executable, "-m", "doctest", "README.rst"]),
        ("conformance-json", [
            sys.executable, "-c",
            "import json,pathlib; [json.loads(p.read_text(encoding='utf-8')) "
            "for p in pathlib.Path('conformance').glob('*.json')]",
        ]),
    ]
    if arguments.fuzz_iterations is not None:
        commands.append(("extended-fuzz", [
            sys.executable, "tools/run_fuzz2026.py",
            "--iterations", str(arguments.fuzz_iterations),
        ]))

    results = [run(name, command) for name, command in commands]
    report = {
        "format": 1,
        "edition": "AXON 2026",
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "system": platform.system(),
        "machine": platform.machine(),
        "python_implementation": platform.python_implementation(),
        "python_version": platform.python_version(),
        "executable": sys.executable,
        "commands": results,
        "passed": all(item["returncode"] == 0 for item in results),
    }
    if arguments.evidence:
        destination = arguments.evidence
        if not destination.is_absolute():
            destination = ROOT / destination
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(
            json.dumps(report, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8")
        print("evidence:", destination)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
