"""Test-only binary adapter to the unchanged G1 Unicode PoC. No path input."""
import io
import json
from pathlib import Path
import struct
import sys

from pypdf import PdfReader

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "spikes/g1/scripts"))
from unicode_writer import complete_pdf

LIMIT = 1024 * 1024


def validate_copy(source, candidate):
    if not 8 <= len(source) <= LIMIT or not 8 <= len(candidate) <= LIMIT:
        raise ValueError("Test copy exceeds bounded size")
    old = PdfReader(io.BytesIO(source), strict=True)
    new = PdfReader(io.BytesIO(candidate), strict=True)
    if old.is_encrypted or new.is_encrypted or len(old.pages) != 1 or len(new.pages) != 1:
        raise ValueError("Test writer accepts one unencrypted page")
    before, after = old.pages[0], new.pages[0]
    body = lambda p: p.get_contents().get_data() if p.get_contents() else b""
    if len(body(before)) > LIMIT or body(before) != body(after):
        raise ValueError("Original page body differs")
    for key in ["/MediaBox", "/CropBox", "/Rotate"]:
        if str(before.get(key)) != str(after.get(key)):
            raise ValueError("Page geometry differs")
    if after.rotation != 0:
        raise ValueError("Rotated pages are outside the bounded writer")
    annotations = [r.get_object() for r in after.get("/Annots", [])]
    if len(annotations) > 32 or any(a.get("/Subtype") not in
                                     ["/FreeText", "/Ink", "/Highlight", "/Underline"] for a in annotations):
        raise ValueError("Annotation set outside test scope")
    if any(a.get("/AP", {}).get("/N") is None for a in annotations if a.get("/Subtype") != "/FreeText"):
        raise ValueError("Non-text annotation appearance missing")
    return annotations


def main():
    packet = sys.stdin.buffer.read(2 * LIMIT + 9)
    if len(packet) < 8:
        raise ValueError("Incomplete binary packet")
    a, b = struct.unpack("<II", packet[:8])
    if a > LIMIT or b > LIMIT or len(packet) != 8 + a + b:
        raise ValueError("Invalid binary lengths")
    source, candidate = packet[8:8+a], packet[8+a:]
    annotations = validate_copy(source, candidate)
    count = sum(a.get("/Subtype") == "/FreeText" for a in annotations)
    if count > 1:
        raise ValueError("Existing Unicode PoC supports exactly one FreeText")
    output, diagnostic = complete_pdf(candidate) if count else (candidate, {"writer": "pdfjs-native-nontext"})
    validate_copy(source, output)
    sys.stdout.buffer.write(output)
    print(json.dumps({"writer": diagnostic["writer"], "bytes": len(output), "testCopyOnly": True}), file=sys.stderr)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(str(error)[:1000], file=sys.stderr)
        sys.exit(2)
