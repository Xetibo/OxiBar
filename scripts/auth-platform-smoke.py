"""Exercise Cage GLES2 against system EGL drivers inside an owned compositor."""
import argparse
import collections
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("cage")
    parser.add_argument("--egl-vendor")
    args = parser.parse_args()
    if os.environ.get("OXIBAR_TEST_ISOLATED") != "1" or os.environ.get("OXIBAR_TEST_PRIVATE_WAYLAND") != "1":
        raise SystemExit("Run through scripts/run-isolated.py --parent")
    env = dict(os.environ, WLR_BACKENDS="wayland", WLR_RENDERER="gles2", WLR_RENDERER_ALLOW_SOFTWARE="1", LIBGL_ALWAYS_SOFTWARE="1")
    for key in ("LD_LIBRARY_PATH", "__EGL_VENDOR_LIBRARY_FILENAMES", "__EGL_VENDOR_LIBRARY_DIRS", "GBM_BACKENDS_PATH", "GBM_BACKEND", "LIBGL_DRIVERS_PATH", "WAYLAND_SOCKET"):
        env.pop(key, None)
    if args.egl_vendor:
        env["__EGL_VENDOR_LIBRARY_FILENAMES"] = str(Path(args.egl_vendor).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix="oxi-egl-") as temporary:
        marker = Path(temporary) / "compositor-ready"
        child = f"from pathlib import Path; import time; Path({str(marker)!r}).touch(); time.sleep(30)"
        logs = collections.deque(maxlen=200)
        process = subprocess.Popen([args.cage, "-s", "--", sys.executable, "-c", child], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, start_new_session=True)
        def drain():
            for line in process.stdout:
                logs.append(line)
        threading.Thread(target=drain, daemon=True).start()
        try:
            deadline = time.monotonic() + 12
            while not marker.exists():
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise AssertionError("GLES2 compositor did not start\n" + "".join(logs))
                time.sleep(.025)
            time.sleep(.25)
            assert process.poll() is None, "".join(logs)
            print(json.dumps(dict(owned_cage_gles2_context=True, egl_vendor=args.egl_vendor or "system default", cage=args.cage)), flush=True)
        finally:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()


if __name__ == "__main__":
    main()
