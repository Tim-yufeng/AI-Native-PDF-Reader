"""Independent, bounded incremental-PDF controls. Never a production Writer."""
import io
import json
import logging
from pathlib import Path
import re
import sys
from contextlib import contextmanager

from pypdf import PdfReader, PdfWriter, __version__ as pypdf_version
from pypdf.generic import (NameObject as N, NumberObject as I, FloatObject as F,
    ArrayObject as A, DictionaryObject as D, TextStringObject as T, IndirectObject)
from reportlab.pdfgen import canvas
from reportlab.pdfbase import pdfmetrics

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "tests/work/security-close/round2"
sys.path.insert(0, str(ROOT / "spikes/g1/scripts"))
from unicode_writer import _load_font, _font_signature, FONT_ALIAS

@contextmanager
def strict_logs():
    buffer = io.StringIO(); handler = logging.StreamHandler(buffer)
    logger = logging.getLogger("pypdf"); logger.addHandler(handler)
    try:
        yield
        assert not buffer.getvalue(), "PDF parser attempted repair/warned: " + buffer.getvalue()
    finally:
        logger.removeHandler(handler)

def last_xref(data):
    match = re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z", data)
    assert match, "complete terminal startxref/EOF required"
    return int(match[1])

def recovery_read(data):
    """Strict diagnostic reader, with a checked valid nonzero subsection case.

    pypdf warns even for some valid controls with nonzero xref subsections.
    Permit that one diagnostic only when every live entry points to its actual
    matching object header; unsupported compressed-object entries refuse.
    """
    output = io.StringIO()
    handler = logging.StreamHandler(output)
    logger = logging.getLogger("pypdf")
    logger.addHandler(handler)
    try:
        reader = PdfReader(io.BytesIO(data), strict=True)
        assert not reader.xref_objStm
        for generation, entries in reader.xref.items():
            for number, offset in entries.items():
                if number == 0 or generation == 65535:
                    continue
                assert re.match(fr"{number}\s+{generation}\s+obj\b".encode(), data[offset:]), "xref requires object correction"
        assert reader.pages and all(p.get_contents() is not None for p in reader.pages)
        allowed = "Xref table not zero-indexed. ID numbers for objects will be corrected."
        messages = output.getvalue().splitlines()
        assert all(m == allowed for m in messages), "unexpected parser diagnostic"
        return len(reader.pages), messages
    finally:
        logger.removeHandler(handler)

def shape(reader):
    def fonts(resources):
        return {str(name): {"reference": [ref.idnum,ref.generation], "signature": _font_signature(ref)}
                for name,ref in resources.get("/Font", {}).items() if isinstance(ref,IndirectObject)}
    return [{"body": p.get_contents().get_data().hex(), "media": list(p.mediabox),
        "crop": list(p.cropbox), "rotation": p.rotation,
        "pageFonts": fonts(p["/Resources"]),
        "nonTargetAnnotations": [_font_signature({k:v for k,v in a.get_object().items() if k != "/P"})
                                 for a in p.get("/Annots", []) if a.get_object()["/Subtype"] != "/FreeText"],
        "freeText": [{"contents": str(a.get_object()["/Contents"]),"rect":list(a.get_object()["/Rect"]),
                      "fonts": fonts(a.get_object()["/AP"]["/N"]["/Resources"])}
                     for a in p.get("/Annots", []) if a.get_object()["/Subtype"] == "/FreeText"],
        "annotations": [(str(a.get_object().get("/NM")), str(a.get_object().get("/Subtype")))
                        for a in p.get("/Annots", [])]} for p in reader.pages]

def validate(old, new):
    assert new.startswith(old) and len(new) > len(old)
    offset = last_xref(new); assert len(old) <= offset < len(new)
    match = re.match(rb"(\d+)\s+(\d+)\s+obj", new[offset:]); assert match
    with strict_logs():
        before = PdfReader(io.BytesIO(old), strict=True)
        after = PdfReader(io.BytesIO(new), strict=True)
        declared = tuple(map(int, re.match(rb"%PDF-(\d)\.(\d)", new).groups()))
        catalog = after.trailer["/Root"].get("/Version")
        effective = max(declared, tuple(map(int, str(catalog)[1:].split(".")))) if catalog else declared
        assert effective >= (1,5), "xref streams require PDF 1.5+; parser acceptance alone is insufficient"
        xref = after.get_object(IndirectObject(int(match[1]), int(match[2]), after))
        assert xref["/Type"] == "/XRef" and int(xref["/Prev"]) == last_xref(old)
        root, prior_root = xref.raw_get("/Root"), before.trailer.raw_get("/Root")
        assert (root.idnum, root.generation) == (prior_root.idnum, prior_root.generation)
        widths = [int(v) for v in xref["/W"]]; assert len(widths) == 3 and all(0 <= w <= 8 for w in widths)
        entries = xref.get_data(); cursor = 0; indexes = list(xref["/Index"])
        assert len(indexes) % 2 == 0
        count = 0
        for start, length in zip(indexes[::2], indexes[1::2]):
            assert 0 <= start and 0 <= length and start + length <= xref["/Size"]
            for number in range(start, start + length):
                fields = []
                for width in widths:
                    fields.append(int.from_bytes(entries[cursor:cursor+width], "big")); cursor += width
                kind = fields[0] if widths[0] else 1
                assert kind == 1, "these controls use uncompressed indirect objects"
                assert len(old) <= fields[1] < offset + len(match[0])
                assert re.match(fr"{number}\s+{fields[2]}\s+obj\b".encode(), new[fields[1]:])
                count += 1
        assert cursor == len(entries)
        assert shape(before) == shape(after), "page body/boxes/rotation/annotation identities changed"
        for page in after.pages:
            for annotation in page.get("/Annots", []):
                obj = annotation.get_object()
                if obj["/Subtype"] == "/FreeText":
                    form = obj["/AP"]["/N"]
                    assert form["/Subtype"] == "/Form" and b"Tj" in form.get_data()
                    assert not any(k in form.get_data() for k in (b" Do", b" BI"))
                    font = next(v.get_object() for v in form["/Resources"]["/Font"].values()
                                if v.get_object().get("/Subtype") == "/TrueType")
                    assert "/ToUnicode" in font and "/FontFile2" in font["/FontDescriptor"]
        self_present = any(start <= int(match[1]) < start + length
                           for start, length in zip(indexes[::2], indexes[1::2]))
        return {"sourceBytes": len(old), "outputBytes": len(new), "oldXref": last_xref(old),
                "newXref": offset, "checkedNewEntries": count, "pages": len(after.pages),
                "xrefSelfEntryPresent": self_present,
                "strictStandardSelfEntryPass": self_present,
                "fullPdfStandardConformance": "Not established"}

def prepare():
    for directory in (WORK, *WORK.parents):
        if directory.exists():
            assert not directory.is_symlink() and directory.resolve() == directory, "linked fixture directory refused"
    WORK.mkdir(parents=True, exist_ok=True)
    font, _ = _load_font()
    text = "中文 α + β ≤ 10"
    buffer = io.BytesIO(); c = canvas.Canvas(buffer, invariant=1, pdfVersion=(1,7))
    for index, dimensions in enumerate([(612, 792), (595, 842), (792, 612), (400, 600)]):
        c.setPageSize(dimensions); c.setFont("Courier", 14); c.drawString(30, 100, f"ORIGINAL PAGE {index+1}")
        c.setFont("Times-Roman", 12); c.drawString(30, 80, "Mixed original fonts remain untouched")
        c.setFont(FONT_ALIAS, 14); c.drawString(30, 60, text); c.showPage()
    c.save()
    writer = PdfWriter(io.BytesIO(buffer.getvalue()))
    writer.pdf_header = "%PDF-1.7"
    for index, page in enumerate(writer.pages):
        page[N("/Rotate")] = I(index * 90)
        page[N("/CropBox")] = A([I(10), I(10), I(int(page.mediabox.width)-10), I(int(page.mediabox.height)-10)])
        page[N("/Annots")] = A()
        mark = D({N("/Type"): N("/Annot"), N("/Subtype"): N("/Highlight"),
                  N("/NM"): T(f"existing-{index}"), N("/Rect"): A(map(I, [30, 95, 190, 115])),
                  N("/QuadPoints"): A(map(I, [30, 115, 190, 115, 30, 95, 190, 95])),
                  N("/C"): A([F(1), F(1), F(0)])})
        page["/Annots"].append(writer._add_object(mark))
        for part in range(2):
            box = [40, 180+part*85, 350, 240+part*85]
            local = io.BytesIO(); appearance = canvas.Canvas(local, pagesize=(310, 60), invariant=1)
            allocation = appearance.beginText(); allocation.setFont(FONT_ALIAS, 14)
            allocation.textOut("".join(sorted(set(text))))
            appearance.setFont(FONT_ALIAS, 14); appearance.drawString(4, 36, text); appearance.save()
            generated = PdfReader(io.BytesIO(local.getvalue())).pages[0]
            from pypdf.generic import DecodedStreamObject
            form = DecodedStreamObject(); form.set_data(generated.get_contents().get_data())
            form.update({N("/Type"): N("/XObject"), N("/Subtype"): N("/Form"),
                N("/BBox"): A(map(I, [0, 0, 310, 60])), N("/Matrix"): A(map(I, [1, 0, 0, 1, 0, 0])),
                N("/Resources"): generated["/Resources"].clone(writer)})
            annot = D({N("/Type"): N("/Annot"), N("/Subtype"): N("/FreeText"),
                N("/NM"): T(f"text-{index}-{part}"), N("/Contents"): T(text),
                N("/Rect"): A(map(I, box)), N("/DA"): T("/G1Unicode 14 Tf 0 g"),
                N("/AP"): D({N("/N"): writer._add_object(form)})})
            page["/Annots"].append(writer._add_object(annot))
    original = io.BytesIO(); writer.write(original); old = original.getvalue()
    update = PdfWriter(io.BytesIO(old), incremental=True)
    for index, page in enumerate(update.pages):
        for annotation in page["/Annots"]:
            target = annotation.get_object()
            if target["/Subtype"] != "/FreeText": continue
            # Reuse original embedded font and geometry. Only vector appearance
            # colour/size and matching metadata change; not our production PoC.
            size = 12 + index * 2
            target[N("/DA")] = T(f"/G1Unicode {size} Tf .15 .3 .6 rg")
            form = target["/AP"]["/N"]
            stream = form.get_data().replace(b"14 Tf", f"{size} Tf".encode())
            form.set_data(b".15 .3 .6 rg\n" + stream)
    out = io.BytesIO(); update.write(out); new = out.getvalue()
    result = validate(old, new)
    for name, data in [("complex-v17-source.pdf", old), ("complex-v17-update.pdf", new)]:
        destination = WORK / name
        if destination.exists(): assert destination.read_bytes() == data
        else: destination.write_bytes(data)
    # Corrupt trailer references and non-updates are refused, not repaired.
    rejected = 0
    start = last_xref(new)
    prev = str(last_xref(old)).encode()
    bad = new[:start] + new[start:].replace(b"/Prev " + prev, b"/Prev " + b"0"*len(prev), 1)
    for damaged in [old+b"\n% not an incremental update\n", new[:-8], bad, new.replace(b"%PDF-1.7",b"%PDF-1.3",1)]:
        try: validate(old.replace(b"%PDF-1.7",b"%PDF-1.3",1) if damaged.startswith(b"%PDF-1.3") else old, damaged)
        except Exception:
            # All failures are test diagnostics, no damaged bytes get saved.
            rejected += 1
        else: raise AssertionError("damaged candidate accepted")
    metrics = {ch: {"glyph": font.face.charToGlyph.get(ord(ch), 0),
                   "width1000": font.face.charWidths.get(ord(ch), 0)} for ch in sorted(set(text))}
    (WORK / "font-oracle.json").write_text(json.dumps({"text": text, "metrics": metrics}), "utf-8")
    print(json.dumps({"structure": result, "negativeControlsRefused": rejected,
        "scope": "independent synthetic controls, not expanded production Writer",
        "pages": [{k: v for k, v in p.items() if k in {"media","crop","rotation","annotations"}}
                  for p in shape(PdfReader(io.BytesIO(old)))]}))

if __name__ == "__main__":
    if "--profile-child" in sys.argv:
        import ctypes
        from ctypes import wintypes
        from unicode_writer import complete_pdf
        class Memory(ctypes.Structure):
            _fields_ = [("cb",wintypes.DWORD),("faults",wintypes.DWORD)]+[(n,ctypes.c_size_t) for n in
                ["peakWorking", "working", "peakPool", "pool", "peakNonPool", "nonPool", "pagefile", "peakPagefile"]]
        output, diagnostic = complete_pdf((ROOT/"tests/work/g1/outputs/chinese-unicode-raw-2.pdf").read_bytes())
        memory=Memory();memory.cb=ctypes.sizeof(memory)
        query=ctypes.WinDLL("psapi").GetProcessMemoryInfo
        query.argtypes=[wintypes.HANDLE,ctypes.POINTER(Memory),wintypes.DWORD]
        assert query(wintypes.HANDLE(-1),ctypes.byref(memory),memory.cb)
        print(json.dumps({"outputBytes":len(output),"peakWorkingBytes":memory.peakWorking}))
    elif "--profile" in sys.argv:
        import subprocess, time
        runs=[]
        for _ in range(3):
            start=time.perf_counter()
            result=subprocess.run([sys.executable,"-I","-B","-X","utf8",str(Path(__file__).resolve()),"--profile-child"],
                                  capture_output=True,check=True,cwd=ROOT)
            runs.append({"processSeconds":time.perf_counter()-start,**json.loads(result.stdout)})
        print(json.dumps({"scope":"existing Python distribution; not a packaged sidecar", "runs":runs}))
    elif "--saved" in sys.argv:
        old = (WORK / "complex-v17-source.pdf").read_bytes()
        records = []
        for path in (ROOT / "tests/work/m3").glob("append-complex-v17-*/original.pdf"):
            records.append({"path": str(path), **validate(old, path.read_bytes())})
        assert records
        print(json.dumps({"saved": records}))
    elif "--writer-standard" in sys.argv:
        # A minimal serializer check, separate from parser/renderer tolerance.
        # ISO 32000-1 7.5.8.3 requires an entry for the indirect xref stream.
        # No library patch, repair, sidecar startup or source overwrite.
        assert re.fullmatch(r"\d+\.\d+\.\d+", pypdf_version)
        source = (ROOT / "tests/work/writer/matrix-source.pdf").read_bytes()
        writer = PdfWriter(io.BytesIO(source), incremental=True)
        writer.pages[0][N("/Rotate")] = I(90)
        buffer = io.BytesIO()
        writer.write(buffer)
        result = buffer.getvalue()
        offset = last_xref(result)
        header = result[offset:result.index(b"stream", offset)]
        number = int(re.match(rb"(\d+)\s+0\s+obj", header)[1])
        indexes = list(map(int, re.search(rb"/Index\s*\[(.*?)\]", header, re.S)[1].split()))
        present = any(start <= number < start + length for start, length in zip(indexes[::2], indexes[1::2]))
        report = {"version": pypdf_version, "sourceBytes": len(source), "outputBytes": len(result),
            "xrefObject": number, "index": indexes, "xrefSelfEntryPresent": present,
            "hybridTrailer": b"/XRefStm" in header, "strictStandardSelfEntryPass": present,
            "standard": "ISO 32000-1 7.5.8.3", "ctrlSPass": False}
        destination = ROOT / "tests/work/security-close" / ("pypdf" + pypdf_version.replace(".", "") + "-self-entry")
        destination.with_suffix(".pdf").write_bytes(result)
        destination.with_suffix(".json").write_text(json.dumps(report), "utf-8")
        print(json.dumps(report))
    elif "--recovery" in sys.argv:
        # Inspect real forced-termination outputs without repairing or writing
        # any PDF. Readable paths and complete recovery materials are distinct.
        records = []
        for evidence in (ROOT / "tests/work/m3").glob("append-save-*/terminated.json"):
            root = evidence.parent
            fact = json.loads(evidence.read_text("utf-8"))
            current = (root / "original.pdf").read_bytes()
            old = (root / "verified-old.pdf").read_bytes()
            new = (root / "verified-new.pdf").read_bytes()
            assert new.startswith(old) and new.startswith(current)
            for material in (old, new):
                recovery_read(material)
            try:
                pages, messages = recovery_read(current)
                readable = True
            except Exception:
                readable = False
                pages = None
                messages = []
            records.append({"path": str(root), "stage": fact["stage"],
                "originalStrictReadable": readable, "originalPages": pages,
                "verifiedNonzeroSubsectionDiagnostic": bool(messages),
                "completeRecoveryMaterialsStrictReadable": True,
                "originalBytes": len(current), "oldBytes": len(old), "newBytes": len(new),
                "sourceUnchangedByRecovery": fact["sourceUnchangedByRecovery"],
                "powerLossTest": False, "ctrlSPass": False})
        assert records and {r["stage"] for r in records} == {"prepared", "partial", "trailer", "appended", "committed"}
        print(json.dumps({"forcedTermination": records, "scope": "independent strict parser, no source repair"}))
    else:
        prepare()
