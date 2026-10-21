"""Bounded static inspection of our solid-LZMA NSIS candidate; never executes it."""
import argparse
import binascii
import hashlib
import json
import lzma
import re
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / ".cache/tauri-target/release"
parser = argparse.ArgumentParser(description="Static inspection only; never executes the installer")
version=json.loads((ROOT/'src-tauri/tauri.conf.json').read_text('utf-8'))['version']
parser.add_argument("--package", type=Path, default=TARGET / f"bundle/nsis/AI-Native PDF Reader_{version}_x64-setup.exe")
parser.add_argument("--reference-package", type=Path, help="Also require the embedded reader to match this previous project candidate")
args = parser.parse_args()
cache = (ROOT / ".cache").resolve(strict=True)
assert cache.is_relative_to(ROOT.resolve(strict=True))
def cache_file(path):
    path = path.resolve(strict=True)
    assert path.is_relative_to(cache) and path.is_file()
    return path
PACKAGE = cache_file(args.package)
MARKER = b"__TAURI_BUNDLE_TYPE_VAR_"
def digest(data): return hashlib.sha256(data).hexdigest()
def pe(data):
    assert data[:2] == b"MZ"
    offset = struct.unpack_from("<I", data, 0x3c)[0]
    assert 0x40 <= offset <= len(data)-256 and data[offset:offset+4] == b"PE\0\0"
    machine = struct.unpack_from("<H", data, offset+4)[0]
    optional = offset+24
    magic = struct.unpack_from("<H", data, optional)[0]
    assert magic in (0x10b, 0x20b)
    directories = optional+(96 if magic == 0x10b else 112)
    certificate = struct.unpack_from("<II", data, directories+32)
    return {"machine": hex(machine), "certificateBytes": certificate[1]}
def unpack(bundle):
    assert 512 < len(bundle) < 64*1024*1024
    offset = bundle.find(b"\xef\xbe\xad\xdeNullsoftInst")-4
    assert offset >= 512 and offset % 512 == 0
    flags, signature, a, b, c, header_length, following = struct.unpack_from("<7I", bundle, offset)
    assert signature == 0xDEADBEEF and flags == 0 and offset+following == len(bundle)
    assert binascii.crc32(bundle[512:-4]) == struct.unpack_from("<I", bundle, len(bundle)-4)[0]
    compressed = bundle[offset+28:-4]
    prop = compressed[0]; dictionary = int.from_bytes(compressed[1:5], "little")
    assert 0 <= prop < 225 and dictionary <= 16*1024*1024
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=[{
        "id": lzma.FILTER_LZMA1, "lc": prop % 9, "lp": prop//9 % 5,
        "pb": prop//45, "dict_size": dictionary}])
    raw = decoder.decompress(compressed[5:], max_length=128*1024*1024)
    assert decoder.eof and not decoder.unused_data
    blocks = []; pos = 0
    while pos < len(raw):
        assert pos+4 <= len(raw) and len(blocks) < 1024
        size = struct.unpack_from("<I", raw, pos)[0]
        assert size <= len(raw)-pos-4
        blocks.append(raw[pos+4:pos+4+size]); pos += 4+size
    assert len(blocks[0]) == header_length
    return blocks

bundle = PACKAGE.read_bytes(); blocks = unpack(bundle)
# A corrupted/truncated package must fail inspection, not be called valid.
for damaged in [bundle[:-1], bundle[:len(bundle)//2], bundle[:600]+bytes([bundle[600]^1])+bundle[601:]]:
    try: unpack(damaged)
    except (AssertionError, struct.error, lzma.LZMAError): pass
    else: raise AssertionError("corrupt package accepted")
exe = (TARGET / "ai-native-pdf-reader.exe").read_bytes()
assert exe.count(MARKER+b"UNK") + exe.count(MARKER+b"NSS") == 1
embedded = [block for block in blocks if len(block) == len(exe) and block.startswith(b"MZ")]
assert len(embedded) == 1
main = embedded[0]
assert main.count(MARKER+b"NSS") == 1
assert main.replace(MARKER+b"NSS", MARKER+b"UNK") == exe.replace(MARKER+b"NSS", MARKER+b"UNK")
reference_unchanged = None
if args.reference_package is not None:
    previous = unpack(cache_file(args.reference_package).read_bytes())
    previous_main = [block for block in previous if len(block) == len(exe) and block.startswith(b"MZ")]
    assert previous_main == [main]
    reference_unchanged = True
assert pe(main)["machine"] == "0x8664" and pe(bundle)["certificateBytes"] == 0
plugins = {}
for candidate in (ROOT / ".cache/tauri-target/.tauri/NSIS/Plugins").rglob("*.dll"):
    plugins[digest(candidate.read_bytes())] = candidate.name
worker=ROOT/'.cache/python-worker'
manifest=json.loads((worker/'manifest.json').read_text('utf-8'))
runtime_lock=json.loads((ROOT/'worker/runtime-lock.json').read_text('utf-8'))
assert runtime_lock['manifestSha256'].encode('ascii') in main, 'compiled worker manifest identity differs from packaged resources'
inventory={item['path']:item for item in manifest['files']}
inventory['manifest.json']={'path':'manifest.json','bytes':(worker/'manifest.json').stat().st_size,'sha256':digest((worker/'manifest.json').read_bytes())}
worker_digests={}
for name,item in inventory.items():
    content=(worker/name).read_bytes()
    assert len(content)==item['bytes'] and digest(content)==item['sha256']
    worker_digests.setdefault(item['sha256'],[]).append(name)
resources = []
for index, block in enumerate(blocks):
    kind = "NSIS header" if index == 0 else "installer/uninstaller/UI data"
    if block == main: kind = "ai-native-pdf-reader.exe"
    elif digest(block) in plugins: kind = plugins[digest(block)]
    elif digest(block) in worker_digests: kind='unicode-worker/'+ '|'.join(worker_digests[digest(block)])
    resources.append({"kind": kind, "bytes": len(block), "sha256": digest(block)})
assert sum(r["kind"] == "ai-native-pdf-reader.exe" for r in resources) == 1
assert set(worker_digests).issubset({digest(block) for block in blocks}), 'worker resource missing/changed in installer'
header = blocks[0]
for value in ["AI-Native PDF Reader", "Software\\Classes\\.pdf\\OpenWithProgids", "io.github.ainativepdfreader.PDF",
              "GetFileAttributesW", "FindFirstFileW", "FindNextFileW", "Windows error:"]:
    assert value.encode("utf-16-le") in header
script = (TARGET / "nsis/x64/installer.nsi").read_text("utf-8-sig")
assert "{{" not in script and 'RequestExecutionLevel user' in script
assert not re.search(r'^\s*GetFullPathName\s',script,re.MULTILINE), 'fresh-directory NSIS regression'
assert 'kernel32::GetFullPathNameW(w "$GuardTarget", i 1024, w .r7, p 0) i .r5 ?e' in script or 'kernel32::GetFullPathNameW(w "$GuardTarget", i ${NSIS_MAX_STRLEN}, w .r7, p 0) i .r5 ?e' in script
assert 'Step: $InstallStage' in script and 'Path: $InstallPath' in script
assert '${If} $0 <> 0x10' in script
assert not re.search(r'^\s*\$\{(?:If|AndIf|OrIf)\}\s+\S+\s+(?:==|!=)\s+0x[\da-f]+\b',script,re.MULTILINE|re.IGNORECASE)
assert '${If} $0 == $1' in script and "RMDir /r" not in script
assert "UserChoice" not in "\n".join(line for line in script.splitlines() if not line.lstrip().startswith(";"))
assert script.count("  File /oname=") == 1
installed={}
for target,source in re.findall(r'^\s*File /a "/oname=([^"\r\n]+)" "([^"\r\n]+)"$',script,re.MULTILINE):
    target=target.replace('\\','/');assert target.startswith('unicode-worker/')
    relative=target[len('unicode-worker/'):]
    assert relative in inventory and relative not in installed
    expected=(worker/relative).resolve(strict=True)
    assert Path(source).resolve(strict=True)==expected
    installed[relative]=source
assert set(installed)==set(inventory)
guarded=re.findall(r'^\s*!insertmacro BeginGuardedWrite "([^"\r\n]+)"$',script,re.MULTILINE)
guarded=[re.sub(r'\\+', '/', value) for value in guarded]
guarded_worker={value[len('$INSTDIR/unicode-worker/'):] for value in guarded if value.startswith('$INSTDIR/unicode-worker/')}
assert guarded_worker==set(inventory), 'runtime writes lack finite leaf guards'
assert len(guarded)==len(inventory)+4
assert script.count('  !insertmacro EndGuardedWrite')==len(guarded)
assert 'i 0x81, i 3, p 0, i 1, i 0x00200000' in script
assert 'Call PinDirectoryTree' in script and 'Call un.PinDirectoryTree' in script
assert '!define MUI_CUSTOMFUNCTION_ABORT ReleaseAllGuards' in script
assert '!define MUI_CUSTOMFUNCTION_UNABORT un.ReleaseAllGuards' in script
uninstall=script.split('Section "Uninstall"',1)[1]
delete_arguments=re.findall(r'^\s*Delete "([^"\r\n]+)"$',uninstall,re.MULTILINE)
normalized=[re.sub(r'\\+', '/', value) for value in delete_arguments]
removed={value[len('$INSTDIR/unicode-worker/'):] for value in normalized if value.startswith('$INSTDIR/unicode-worker/')}
assert removed==set(inventory), 'uninstall is not the fixed resource allowlist'
assert 'Call CheckResourceDirectory' in script and 'Call un.CheckResourceDirectory' in uninstall
directory_check = script.split("Function CheckInstallDir")[1].split("FunctionEnd")[0]
assert "kernel32::GetLastError" not in directory_check
for api, register in [("GetFileAttributesW", "$4"), ("FindFirstFileW", "$2"), ("FindNextFileW", "$2")]:
    lines = directory_check.splitlines()
    index = next(i for i, line in enumerate(lines) if f"kernel32::{api}(" in line)
    assert lines[index].rstrip().endswith("?e'") and lines[index+1].strip() == f"Pop {register}"
print(json.dumps({"status": "PASS", "scope": "static only; not installation/uninstall proof",
    "path": str(PACKAGE), "bytes": len(bundle), "sha256": digest(bundle),
    "installerPe": pe(bundle), "embeddedMainPe": pe(main), "embeddedMainMatchesBuildExceptBundleMarker": True,
    "referenceEmbeddedMainUnchanged": reference_unchanged,
    "crc": True, "corruptCandidatesRefused": True, "resources": resources,
    "writerRuntimeIncluded": True,"workerFiles":len(inventory),"workerContentMatchesManifest":True,
    "uninstallExactResourceAllowlist":True,"unknownFilesAndUserDataRetained":True,
    "finiteWritesHaveContinuousGuards":True,"atomicInstallationProved":False,
    "isolationAcceptance":"separate; static package does not establish process permissions",
    "systemChanges": 0}, ensure_ascii=False))
