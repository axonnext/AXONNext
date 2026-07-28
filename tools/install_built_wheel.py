"""Install the single wheel produced by the platform matrix."""

from pathlib import Path
import subprocess
import sys


root = Path(__file__).resolve().parents[1]
wheels = sorted((root / "dist").glob("*.whl"))
if len(wheels) != 1:
    raise SystemExit("expected exactly one wheel, found %d" % len(wheels))
raise SystemExit(subprocess.call([
    sys.executable, "-m", "pip", "install", "--force-reinstall",
    str(wheels[0]),
]))
