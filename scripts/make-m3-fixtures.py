"""Small disposable M3 controls; existing Python is a test tool, not app runtime."""
import hashlib
import json
from pathlib import Path
from pypdf import PdfReader, PdfWriter

root = Path(__file__).resolve().parent.parent
target = root / "tests/work/m3/fixtures"
target.mkdir(parents=True, exist_ok=True)
source = root / "tests/work/m2-b/gui/reading-120.pdf"
before = hashlib.sha256(source.read_bytes()).hexdigest()
reader = PdfReader(source, strict=True)
records = []
for filename, encrypted in [("bookmarks.pdf", False), ("password.pdf", True)]:
    writer = PdfWriter()
    for page in reader.pages[:3]:
        writer.add_page(page)
    parent = writer.add_outline_item("Chapter one", 0)
    writer.add_outline_item("Page three", 2, parent=parent)
    if encrypted:
        writer.encrypt("m3-test", "owner-m3-only", algorithm="AES-128")
    path = target / filename
    with path.open("xb") as stream:
        writer.write(stream)
    check = PdfReader(path, strict=True)
    if encrypted:
        assert check.decrypt("wrong") == 0
        assert check.decrypt("m3-test") != 0
    assert len(check.pages) == 3
    assert len(check.outline) == 2
    assert "Reading page 3" in check.pages[2].extract_text()
    records.append({"name": filename, "bytes": path.stat().st_size,
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
assert hashlib.sha256(source.read_bytes()).hexdigest() == before
(target / "manifest.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
print(json.dumps(records))
