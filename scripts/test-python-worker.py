"""Project-local dependency test runner; never changes the Python installation."""
import hashlib
import json
from pathlib import Path
import runpy
import sys
ROOT = Path(__file__).resolve().parent.parent
lock = json.loads((ROOT/"scripts/python-worker-dependencies.json").read_text("utf-8"))["pypdfWheel"]
wheel = ROOT/".cache/python-wheels"/lock["filename"]
data = wheel.read_bytes()
assert len(data) == lock["bytes"] and hashlib.sha256(data).hexdigest() == lock["sha256"]
sys.path.insert(0, str(wheel))
sys.argv = [str(ROOT/"tests/worker/test_engine.py"), "-v"]
runpy.run_path(sys.argv[0], run_name="__main__")
