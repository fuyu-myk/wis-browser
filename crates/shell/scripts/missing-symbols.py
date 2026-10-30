"""Lists the symbols a Mach-O binary binds strongly that this macOS lacks. CI
runs it on the oldest macOS Wis supports.

    python3 -I crates/shell/scripts/missing-symbols.py target/debug/wis-browser [arch]
"""
import ctypes
import os
import re
import subprocess
import sys

binary = sys.argv[1]
arch = sys.argv[2] if len(sys.argv) > 2 else os.uname().machine

libs = []
for line in subprocess.run(["otool", "-arch", arch, "-L", binary], capture_output=True, text=True, check=True).stdout.splitlines()[1:]:
    path = line.strip().split(" (compatibility")[0]
    if path:
        libs.append(path)


def short(path):
    name = os.path.basename(path)
    name = re.sub(r"\.dylib$", "", name)
    return re.sub(r"\.[A-Z0-9]+$", "", name) if name.startswith("lib") else name


def resolved(path):
    return path.replace("@rpath/", "/usr/lib/swift/")


by_short = {}
for path in libs:
    by_short.setdefault(short(path), path)

dl = ctypes.CDLL(None)
dl.dlsym.restype = ctypes.c_void_p
dl.dlsym.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
handles = {}
missing_libs = set()
missing = []
checked = 0
out = subprocess.run(["nm", "-arch", arch, "-m", "-u", binary], capture_output=True, text=True, check=True).stdout
for line in out.splitlines():
    m = re.match(r"\s*\(undefined\) (weak )?external (\S+) \(from (\S+)\)", line)
    if not m or m.group(1) or m.group(2) == "dyld_stub_binder":
        continue
    symbol, lib = m.group(2), m.group(3)
    path = by_short.get(lib)
    if path is None:
        missing.append(f"{lib}: {symbol} (library not in otool -L)")
        continue
    if path not in handles:
        try:
            handles[path] = ctypes.CDLL(resolved(path), mode=os.RTLD_LAZY)._handle
        except OSError:
            handles[path] = None
            missing_libs.add(path)
    checked += 1
    if handles[path] is None:
        continue
    if not dl.dlsym(handles[path], symbol[1:].encode()):
        missing.append(f"{lib}: {symbol}")

print(f"{checked} strong symbols checked across {len(by_short)} libraries")
for path in sorted(missing_libs):
    print(f"MISSING LIBRARY {path}")
for entry in missing:
    print(f"MISSING {entry}")
sys.exit(1 if missing or missing_libs else 0)
