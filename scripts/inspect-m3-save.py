"""Independent strict parsing of the latest disposable Windows save-lab run.

Uses the existing test-only Python environment, never the application runtime.
No mutation of PDFs, no arbitrary paths, no cleanup of recovery material.
"""
import hashlib
import io
import json
from pathlib import Path

from pypdf import PdfReader

PROJECT = Path(__file__).resolve().parent.parent
WORK = PROJECT / "tests/work/m3"
CONTROL = (PROJECT / "tests/fixtures/g1/freetext-control.pdf").read_bytes()
OUTPUT = CONTROL + b"\n% M3 controlled valid trailing comment\n"


def parsed(data):
    reader = PdfReader(io.BytesIO(data), strict=True)
    result = []
    for page in reader.pages:
        annotations = []
        for reference in page.get("/Annots", []):
            annotation = reference.get_object()
            appearance = annotation.get("/AP", {}).get("/N")
            annotations.append({
                "type": str(annotation.get("/Subtype")),
                "text": str(annotation.get("/Contents")),
                "rect": [float(v) for v in annotation.get("/Rect", [])],
                "appearance": hashlib.sha256(appearance.get_object().get_data()).hexdigest()
                if appearance else None,
            })
        result.append({
            "contents": hashlib.sha256(page.get_contents().get_data()).hexdigest(),
            "text": page.extract_text(), "annotations": annotations,
        })
    return result


expected = parsed(CONTROL)
assert any(page["annotations"] for page in expected), "control needs native annotations"
cases = []
for label in ["success", *["exit-" + stage for stage in
                         ["partial", "write", "flush", "prepared", "backup", "publish", "journal"]]]:
    roots = list(WORK.glob("save-" + label + "-*"))
    assert roots, label
    root = max(roots, key=lambda p: p.stat().st_mtime_ns)
    assert root.resolve().parent == WORK.resolve() and not root.is_symlink()
    original = (root / "original.pdf").read_bytes()
    assert original == (OUTPUT if label in ["success", "exit-publish", "exit-journal"] else CONTROL), label
    checked = []
    for name in ["original.pdf", "original.backup.pdf", "pending.pdf"]:
        path = root / name
        if not path.exists():
            continue
        assert not path.is_symlink()
        data = path.read_bytes()
        if data not in [CONTROL, OUTPUT]:
            assert label == "exit-partial" and name == "pending.pdf" and data == OUTPUT[:len(OUTPUT) // 2]
            continue  # Expected incomplete pending material, never a completed PDF.
        assert parsed(data) == expected, (label, name)
        checked.append({"file": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    cases.append({"case": label, "directory": root.name, "strictlyParsed": checked})

evidence = {"controlBytes": len(CONTROL), "outputBytes": len(OUTPUT),
            "pages": len(expected), "annotations": sum(len(page["annotations"]) for page in expected),
            "result": "Pass", "cases": cases}
(WORK / "save-lab-inspection.json").write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({key: value for key, value in evidence.items() if key != "cases"}, ensure_ascii=False))
print("Strictly parsed complete outputs:", sum(len(case["strictlyParsed"]) for case in cases))

# Replacement candidates deliberately include failed product requirements.
# Parsing complete recovery material is independent of replacement safety.
replacement_cases = []
for label in ["continuous", "race", "posix-race", *["exit-" + stage for stage in
               ["partial", "flushed", "prepared", "before-replace", "after-replace"]]]:
    root = max(WORK.glob("replace-" + label + "-*"), key=lambda p: p.stat().st_mtime_ns)
    assert root.resolve().parent == WORK.resolve() and not root.is_symlink()
    checked = []
    for path in root.glob("*.pdf"):
        assert not path.is_symlink()
        data = path.read_bytes()
        if not data.endswith(b"%%EOF\n") and b"%%EOF" not in data[-1024:]:
            assert label == "exit-partial" and path.name == "pending.pdf"
            continue
        assert parsed(data) == expected, (label, path.name)
        checked.append({"file": path.name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()})
    replacement_cases.append({"case": label, "directory": root.name,
                              "diagnostic": json.loads((root / "evidence.json").read_text()), "strictlyParsed": checked})
(WORK / "replacement-inspection.json").write_text(json.dumps({"materialParsing": "Pass",
    "productionSafety": "Fail", "cases": replacement_cases}, indent=2), encoding="utf-8")
print("Replacement material parsed:", sum(len(case["strictlyParsed"]) for case in replacement_cases))

# Reuse existing G1 appearance/object checker on the isolated desktop worker outputs.
import importlib.util
spec = importlib.util.spec_from_file_location("g1_inspector", PROJECT / "spikes/g1/scripts/inspect-writer-output.py")
inspector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inspector)
writer_cases = []
for record in json.loads((WORK / "writer-copy-evidence.json").read_text()):
    target = WORK / "writer-copies" / record["output"]
    assert target.parent == WORK / "writer-copies" and not target.is_symlink()
    data = target.read_bytes()
    assert hashlib.sha256(data).hexdigest() == record["sha256"]
    raw = (PROJECT / "tests/work/g1/outputs" / record["input"]).read_bytes()
    old, new = PdfReader(io.BytesIO(raw), strict=True), PdfReader(io.BytesIO(data), strict=True)
    assert len(new.pages) == len(old.pages) == 1
    assert new.pages[0].get_contents().get_data() == old.pages[0].get_contents().get_data()
    before, after = [r.get_object() for r in old.pages[0]["/Annots"]], [r.get_object() for r in new.pages[0]["/Annots"]]
    assert len(before) == len(after) == 1
    assert after[0]["/Subtype"] == "/FreeText"
    for key in ["/Contents", "/Rect"]:
        assert str(before[0][key]) == str(after[0][key])
    ap = inspector.appearance_summary(after[0], new)
    assert ap["present"] and ap["formStream"]
    assert ap["operators"].get("Tj", 0) + ap["operators"].get("TJ", 0) > 0
    assert ap["operators"].get("Do", 0) == 0, "appearance must use native text"
    assert any(font["embedded"] and font["hasToUnicode"] for font in ap["fonts"])
    writer_cases.append({"file": target.name, "bytes": len(data), "contents": str(after[0]["/Contents"]),
                         "rect": [float(v) for v in after[0]["/Rect"]], "appearance": ap})
(WORK / "writer-copy-inspection.json").write_text(json.dumps({"objectChecks": "Pass", "GUI": "Not run",
    "Edge": "Not run", "cases": writer_cases}, ensure_ascii=False, indent=2), encoding="utf-8")
print("Unicode worker outputs checked:", len(writer_cases))
