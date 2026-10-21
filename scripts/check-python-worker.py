"""Static checks only; does not install, create a profile or launch the worker."""
import hashlib
import json
from pathlib import Path
import struct
import zipfile

def imports(data):
    """Read bounded normal and delay-import tables, without loading PE code."""
    offset=struct.unpack_from('<I',data,0x3c)[0];optional=offset+24
    assert struct.unpack_from('<H',data,optional)[0]==0x20b
    directory=optional+112
    section_start=optional+struct.unpack_from('<H',data,offset+20)[0]
    sections=[]
    for index in range(struct.unpack_from('<H',data,offset+6)[0]):
        start=section_start+index*40
        virtual_size,rva,size,raw=struct.unpack_from('<4I',data,start+8)
        sections.append((rva,max(size,virtual_size),raw,size))
    def file_offset(rva):
        for base,size,raw,raw_size in sections:
            if base<=rva<base+size:
                location=raw+rva-base
                assert rva-base<raw_size and location<len(data)
                return location
        raise AssertionError('unmapped PE import')
    def name(rva):
        start=file_offset(rva);end=data.find(b'\0',start,start+256)
        assert end>start
        value=data[start:end].decode('ascii').lower()
        assert value.endswith('.dll') and '/' not in value and '\\' not in value
        return value
    result=set()
    for entry,width,is_delay in [(1,20,False),(13,32,True)]:
        rva,size=struct.unpack_from('<II',data,directory+8*entry)
        if not rva:continue
        assert 0<size<65536
        start=file_offset(rva);terminated=False
        for current in range(start,start+size,width):
            descriptor=struct.unpack_from('<'+('8I' if is_delay else '5I'),data,current)
            if not any(descriptor):terminated=True;break
            if is_delay:assert descriptor[0]==1
            result.add(name(descriptor[1] if is_delay else descriptor[3]))
        assert terminated
    return result
ROOT=Path(__file__).resolve().parent.parent
BASE=ROOT/".cache/python-worker"
manifest_bytes=(BASE/"manifest.json").read_bytes()
lock=json.loads((ROOT/"worker/runtime-lock.json").read_text("utf-8"))
assert hashlib.sha256(manifest_bytes).hexdigest()==lock["manifestSha256"]
manifest=json.loads(manifest_bytes)
dependency_lock=json.loads((ROOT/'scripts/python-worker-dependencies.json').read_text('utf-8'))
assert manifest['packages']==dependency_lock['packages'] and manifest['python']==dependency_lock['python']
expected={item["path"] for item in manifest["files"]}|{"manifest.json"}
actual={p.relative_to(BASE).as_posix() for p in BASE.rglob("*") if p.is_file()}
assert actual==expected
native_imports={}
for item in manifest["files"]:
    p=BASE/item["path"];assert p.is_file() and not p.is_symlink() and p.resolve()==p
    data=p.read_bytes();assert len(data)==item["bytes"] and hashlib.sha256(data).hexdigest()==item["sha256"]
    if p.suffix in {".exe",".dll",".pyd"}:
        assert data[:2]==b'MZ';offset=struct.unpack_from('<I',data,0x3c)[0]
        assert data[offset:offset+4]==b'PE\0\0' and struct.unpack_from('<H',data,offset+4)[0]==0x8664
        native_imports[item['path']]=sorted(imports(data))
local_dlls={Path(name).name.lower() for name in expected if name.lower().endswith('.dll')}
os_dlls={'kernel32.dll','advapi32.dll','user32.dll','gdi32.dll','ws2_32.dll','ole32.dll','oleaut32.dll','shell32.dll','shlwapi.dll','bcrypt.dll','version.dll','ntdll.dll','rpcrt4.dll','secur32.dll','crypt32.dll','comdlg32.dll','winmm.dll','ucrtbase.dll','iphlpapi.dll','psapi.dll'}
for names in native_imports.values():
    assert all(name in local_dlls or name in os_dlls or name.startswith(('api-ms-win-','ext-ms-win-')) for name in names), names
assert native_imports['python.exe']==['kernel32.dll'], 'entry must not implicitly import Python/VC DLLs'
assert lock['nativeEntry']['exeSha256']==hashlib.sha256((BASE/'python.exe').read_bytes()).hexdigest()
assert lock['nativeEntry']['sourceSha256']==hashlib.sha256((ROOT/'worker/windows_entry.c').read_bytes()).hexdigest()
for filename in ["engine.py","main.py","pdf_objects.py","standards.py","incremental_table.py","isolation.py","font.json"]:
    assert (BASE/"worker"/filename).read_bytes()==(ROOT/"worker"/filename).read_bytes()
assert not any("DarkGarden" in f or f.startswith("site-packages/reportlab/fonts/") or "fonttools" in f or "fitz" in f.lower() for f in expected)
assert (BASE/"LICENSE.txt").stat().st_size>10000
for name in ["pypdf-6.20.1.dist-info","reportlab-4.4.9.dist-info","pillow-12.3.0.dist-info","charset_normalizer-3.5.2.dist-info"]:
    assert any("license" in p.name.lower() for p in (BASE/"site-packages"/name).rglob('*') if p.is_file())
assert (BASE/"worker/fonts/LICENSE").is_file() and 'SIL OPEN FONT LICENSE' in (BASE/"worker/fonts/LICENSE").read_text('utf-8')
assert (BASE/"python312._pth").read_text('ascii')=="python312.zip\n.\nsite-packages\nworker\nimport site\n"
wheel=ROOT/'.cache/python-wheels'/dependency_lock['pypdfWheel']['filename']
assert hashlib.sha256(wheel.read_bytes()).hexdigest()==dependency_lock['pypdfWheel']['sha256']
with zipfile.ZipFile(wheel) as archive:
    assert {name[len('site-packages/'):] for name in expected if name.startswith(('site-packages/pypdf/','site-packages/pypdf-'))}==set(archive.namelist())
    for name in archive.namelist():assert (BASE/'site-packages'/name).read_bytes()==archive.read(name)
bundle=BASE.with_suffix('.zip')
with zipfile.ZipFile(bundle) as z:
    assert set(z.namelist())==expected and z.testzip() is None
    for name in z.namelist(): assert z.read(name)==(BASE/name).read_bytes()
report={"status":"PASS","scope":"static local prototype only; no installation/isolation acceptance",
    "files":len(expected),"directoryBytes":sum(p.stat().st_size for p in BASE.rglob('*') if p.is_file()),
    "archiveBytes":bundle.stat().st_size,"archiveSha256":hashlib.sha256(bundle.read_bytes()).hexdigest(),
    "noticesIncluded":True,"agplEngineIncluded":False,"optionalReportlabFontsExcluded":True,
    "nativeImportInventory":native_imports,"appLocalVcRuntime":True,
    "finalVcRedistributionReviewPending":True,"productionEnabled":False}
(ROOT/"tests/work/writer/package.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print(json.dumps(report))
