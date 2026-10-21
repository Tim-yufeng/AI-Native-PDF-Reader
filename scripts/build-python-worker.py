"""Project-local M8 candidate sidecar, assembled from pinned existing files.

No download, pip, installer, profile creation, registry or user environment edits.
Only an explicitly named fresh directory inside .cache may be created.
"""
import hashlib
import importlib.metadata as metadata
import json
from pathlib import Path
import shutil
import sys
import zipfile

ROOT = Path(__file__).resolve().parent.parent
DEST = ROOT / ".cache/python-worker"
native_entry=ROOT/'.cache/worker-entry/python.exe'
native_identity=json.loads((ROOT/'.cache/worker-entry/identity.json').read_text('utf-8'))
assert native_identity['schema']==1
assert native_identity['sourceSha256']==hashlib.sha256((ROOT/'worker/windows_entry.c').read_bytes()).hexdigest()
assert native_identity['exeSha256']==hashlib.sha256(native_entry.read_bytes()).hexdigest()
dependency_lock=json.loads((ROOT/'scripts/python-worker-dependencies.json').read_text('utf-8'))
assert dependency_lock['schema']==1
EXPECTED=dependency_lock['packages']
wheel_lock=dependency_lock['pypdfWheel']
wheel=ROOT/'.cache/python-wheels'/wheel_lock['filename']
assert wheel.is_file() and not wheel.is_symlink() and wheel.resolve()==wheel
wheel_bytes=wheel.read_bytes()
assert len(wheel_bytes)==wheel_lock['bytes'] and hashlib.sha256(wheel_bytes).hexdigest()==wheel_lock['sha256']
with zipfile.ZipFile(wheel) as archive:
    pypdf_files={name:archive.read(name) for name in archive.namelist()}
assert f'pypdf-{EXPECTED["pypdf"]}.dist-info/METADATA' in pypdf_files
assert f'Version: {EXPECTED["pypdf"]}\n'.encode() in pypdf_files[f'pypdf-{EXPECTED["pypdf"]}.dist-info/METADATA']
for name in pypdf_files:
    parts=Path(name).parts
    assert name.replace('\\','/')==name and not Path(name).is_absolute() and all(part not in {'','..','.'} for part in parts)
    assert parts[0] in {'pypdf',f'pypdf-{EXPECTED["pypdf"]}.dist-info'}
    assert not name.endswith(('.pyc','.pyd','.dll','.exe','.pth'))
if sys.version_info[:3] != (3,12,14):
    raise SystemExit("Existing CPython version differs; no automatic installation")
runtime = Path(sys.base_prefix)
for name,version in EXPECTED.items():
    if name!='pypdf' and metadata.version(name)!=version:
        raise SystemExit("Pinned dependency differs")
refresh = sys.argv[1:] == ["--refresh"]
if sys.argv[1:] and not refresh:
    raise SystemExit("Only the fixed prototype refresh option is supported")
if DEST.exists():
    if not refresh or DEST.is_symlink() or DEST.resolve() != DEST:
        raise SystemExit("Prototype already exists; explicit controlled refresh required")
    prior = json.loads((DEST/"manifest.json").read_text("utf-8"))
    allowed = {f["path"] for f in prior["files"]} | {"manifest.json"}
    actual = {p.relative_to(DEST).as_posix() for p in DEST.rglob("*") if p.is_file()}
    if prior["schema"] != 1 or allowed != actual:
        raise SystemExit("Unexpected cache contents; retain and refuse refresh")
    for entry in prior["files"]:
        p=DEST/entry["path"]
        if p.is_symlink() or p.resolve()!=p or hashlib.sha256(p.read_bytes()).hexdigest()!=entry["sha256"]:
            raise SystemExit("Previous artifact changed; retain and refuse refresh")
    # A verified prior generated package may contain the replaced pypdf version.
    # Prune only authenticated superseded pypdf files, never any user directory.
    required={'site-packages/'+name for name in pypdf_files}
    stale=[]
    for entry in prior['files']:
        relative=entry['path']
        if (relative.startswith('site-packages/pypdf/') or relative.startswith('site-packages/pypdf-')) and relative not in required:
            target=DEST/relative
            assert target.resolve()==target and target.is_relative_to(DEST)
            stale.append(target)
    for target in stale:target.unlink()
    for directory in sorted({parent for target in stale for parent in target.parents if parent!=DEST and parent.is_relative_to(DEST)},key=lambda p:len(p.parts),reverse=True):
        if directory.is_dir() and not any(directory.iterdir()):directory.rmdir()
def copy(source, target):
    if source.is_symlink() or not source.is_file() or source.resolve() != source:
        raise ValueError("linked source refused")
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists() and target.read_bytes() == source.read_bytes():
        return
    shutil.copyfile(source, target)

DEST.mkdir(parents=True, exist_ok=refresh)
copy(native_entry,DEST/'python.exe')
for name in ["python312.dll","python3.dll","vcruntime140.dll","vcruntime140_1.dll","LICENSE.txt"]:
    copy(runtime/name, DEST/name)
for name in ["_decimal.pyd","_socket.pyd","select.pyd","unicodedata.pyd","pyexpat.pyd"]:
    copy(runtime/"DLLs"/name, DEST/name)
# Do not ship development tools, tkinter, pip, unused SSL/crypto, SQLite or ctypes.
excluded = {"site-packages","test","tests","idlelib","tkinter","ensurepip","venv","unittest","turtledemo"}
with zipfile.ZipFile(DEST/"python312.zip", "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for source in sorted((runtime/"Lib").rglob("*.py")):
        relative = source.relative_to(runtime/"Lib")
        if not set(relative.parts) & excluded:
            archive.write(source, str(relative).replace("\\","/"))
# _pth disables registry/env/user-site imports; site is needed for wheel metadata.
(DEST/"python312._pth").write_text("python312.zip\n.\nsite-packages\nworker\nimport site\n", encoding="ascii")
site = runtime/"Lib/site-packages"
package_dirs = ["reportlab","PIL","charset_normalizer"]
for dirname in package_dirs:
    for source in sorted((site/dirname).rglob("*")):
        if not source.is_file() or "__pycache__" in source.parts or source.suffix == ".pyc":
            continue
        relative = source.relative_to(site)
        # ReportLab optional bundled fonts include unrelated license obligations.
        if dirname == "reportlab" and "fonts" in relative.parts:
            continue
        copy(source, DEST/"site-packages"/relative)
for name in EXPECTED:
    if name=='pypdf':continue
    dist = metadata.distribution(name)
    for file in dist.files:
        relative = Path(str(file))
        if relative.parts[0].endswith(".dist-info") and relative.suffix != ".pyc":
            copy(Path(dist.locate_file(file)), DEST/"site-packages"/relative)
for name,data in pypdf_files.items():
    target=DEST/'site-packages'/name
    target.parent.mkdir(parents=True,exist_ok=True)
    target.write_bytes(data)
for name in ["main.py","engine.py","pdf_objects.py","standards.py","incremental_table.py","isolation.py","font.json"]:
    copy(ROOT/"worker"/name, DEST/"worker"/name)
for name in ["NotoSansSC-VF.ttf","LICENSE"]:
    copy(ROOT/"worker/fonts"/name, DEST/"worker/fonts"/name)
notice = """M8 local validation candidate; public release not approved.
CPython 3.12.14: LICENSE.txt (PSF license and included notices).
Project-owned fixed native entry: source repository worker/windows_entry.c; Python DLL/ABI unchanged.
The entry is compiled /MT with the existing MSVC and statically links its CRT.
Static CRT redistribution also remains subject to Microsoft license terms.
pypdf 6.20.1: BSD-3-Clause, fixed official wheel dist-info/licenses/LICENSE.
ReportLab 4.4.9: BSD license, wheel dist-info licenses; optional fonts excluded.
Pillow 12.3.0: MIT-CMU and bundled third-party notices, wheel dist-info/licenses.
charset-normalizer 3.5.2: MIT, wheel dist-info/licenses.
NotoSansSC-VF: SIL OFL-1.1, worker/fonts/LICENSE; unmodified pinned font.
Microsoft VC runtime DLLs are unmodified app-local files, not a system installer.
Redistribution is conditional on valid Visual Studio licensing and its terms;
the source signature/version checks do not by themselves grant distribution rights.
See https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files
and https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution
No installer downloads or modifies a global VC runtime, Python or system font.
No MuPDF/PyMuPDF/AGPL engine, system fonts, private documents or credentials.
"""
(DEST/"NOTICE.txt").write_text(notice,encoding="utf-8")
inventory=[]
for source in sorted(DEST.rglob("*")):
    if source.is_file() and source.name != "manifest.json":
        data=source.read_bytes()
        inventory.append({"path":source.relative_to(DEST).as_posix(),"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()})
manifest={"schema":1,"python":"3.12.14","packages":EXPECTED,"files":inventory,"scope":"M8 validation candidate; final distribution not approved"}
(DEST/"manifest.json").write_text(json.dumps(manifest,indent=2),encoding="utf-8")
# The reviewed runtime identity is an explicit project artifact, never a user
# environment setting. A fresh checkout with different newlines needs this lock.
runtime_lock={"schema":1,"manifestSha256":hashlib.sha256((DEST/"manifest.json").read_bytes()).hexdigest(),
              "nativeEntry":native_identity,"scope":"M8 project validation candidate; not release approved"}
(ROOT/"worker/runtime-lock.json").write_text(json.dumps(runtime_lock,indent=2)+"\n",encoding="utf-8")
archive=DEST.with_suffix(".zip")
if archive.exists() and not refresh:
    raise SystemExit("Prototype archive exists; refuse overwrite")
with zipfile.ZipFile(archive,"w",compression=zipfile.ZIP_DEFLATED,compresslevel=9) as bundle:
    for source in sorted(DEST.rglob("*")):
        if source.is_file():
            bundle.write(source,source.relative_to(DEST).as_posix())
print(json.dumps({"status":"PASS","path":str(DEST),"files":len(inventory)+1,
    "directoryBytes":sum(f.stat().st_size for f in DEST.rglob('*') if f.is_file()),
    "archiveBytes":archive.stat().st_size,"archiveSha256":hashlib.sha256(archive.read_bytes()).hexdigest(),
    "installed":False,"releaseApproved":False}))
