"""Inspect the existing Writer contract using known synthetic bytes, not GUI."""
import io
import json
from pathlib import Path
import sys
from importlib import metadata

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "spikes/g1/scripts"))
from unicode_writer import complete_pdf
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, NumberObject, ArrayObject, FloatObject, TextStringObject

raw = (ROOT / "tests/work/g1/outputs/chinese-unicode-raw-2.pdf").read_bytes()
def changed(kind):
    writer = PdfWriter(io.BytesIO(raw), incremental=True)
    page = writer.pages[0]
    text = page["/Annots"][0].get_object()
    if kind == "rotation": page[NameObject("/Rotate")] = NumberObject(90)
    elif kind == "color":
        text[NameObject("/C")] = ArrayObject([FloatObject(.2), FloatObject(.4), FloatObject(.6)])
        text[NameObject("/DA")] = TextStringObject("/Helv 12 Tf .2 .4 .6 rg")
    elif kind == "small-box": text[NameObject("/Rect")] = ArrayObject([NumberObject(n) for n in [20, 20, 21, 21]])
    elif kind.startswith("size-"):
        text[NameObject("/DA")] = TextStringObject(f"/Helv {kind[5:]} Tf 0 g")
        text[NameObject("/Rect")] = ArrayObject([NumberObject(n) for n in [20, 20, 500, 300]])
    elif kind == "multiple-pages": writer.add_page(PdfReader(io.BytesIO(raw)).pages[0])
    elif kind == "multiple-text":
        duplicate = text.clone(writer, force_duplicate=True)
        duplicate[NameObject("/NM")] = TextStringObject("synthetic-second-text")
        page["/Annots"].append(duplicate.indirect_reference)
    result = io.BytesIO(); writer.write(result)
    data = result.getvalue()
    # Control modifications themselves must parse before probing the Writer.
    parsed = PdfReader(io.BytesIO(data), strict=True)
    assert parsed.pages[0].get_contents().get_data() == PdfReader(io.BytesIO(raw)).pages[0].get_contents().get_data()
    return data

cases = []
for kind in ["size-12", "size-18", "size-24", "small-box", "color", "rotation", "multiple-pages", "multiple-text"]:
    data = changed(kind)
    try:
        output, diagnostic = complete_pdf(data)
        reopened = PdfReader(io.BytesIO(output), strict=True)
        target = reopened.pages[0]["/Annots"][0].get_object()
        assert str(target["/Contents"]) == str(PdfReader(io.BytesIO(data)).pages[0]["/Annots"][0].get_object()["/Contents"])
        assert target["/AP"]["/N"]["/Subtype"] == "/Form"
        cases.append({"case": kind, "result": "supported-in-bounded-PoC", "incremental": output.startswith(data), "fontSize": diagnostic["fontSize"]})
    except ValueError as error:
        cases.append({"case": kind, "result": "refused", "reason": str(error)})
assert all(c["result"] == ("supported-in-bounded-PoC" if c["case"].startswith("size-") else "refused") for c in cases)
packages = []
for name in ["pypdf", "reportlab", "pillow", "charset-normalizer"]:
    dist = metadata.distribution(name)
    packages.append({"name": name, "version": dist.version, "license": dist.metadata.get("License-Expression") or dist.metadata.get("License"), "requirements": dist.requires, "filesBytes": sum(p.stat().st_size for f in dist.files or [] if (p := Path(dist.locate_file(f))).is_file())})
print(json.dumps({"scope": "existing unmodified Python PoC, synthetic in-memory controls", "cases": cases, "packages": packages, "productionRuntimeApproved": False}, ensure_ascii=False))
