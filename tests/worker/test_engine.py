"""Only synthetic controls. Tests both preservation and refusal boundaries."""
import io
import json
import hashlib
import re
import struct
import subprocess
import sys
import unittest
from pathlib import Path
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject as N, TextStringObject as T, FloatObject as F, NumberObject as I, ArrayObject as A, DictionaryObject as D, IndirectObject, DecodedStreamObject
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/"worker"))
from engine import complete, read, annotations, protected_shape
from pdf_objects import signature
from standards import xref_self_entries
from incremental_table import final_incremental_table

SOURCE=(ROOT/"tests/work/writer/matrix-source.pdf").read_bytes()
CANDIDATE=(ROOT/"tests/work/writer/matrix-candidate.pdf").read_bytes()
def change(data, operation):
    writer=PdfWriter(io.BytesIO(data),incremental=True)
    operation(writer)
    changed=sorted(reference.idnum for reference in writer.list_objects_in_increment())
    # A test-control serializer only. Library stream-increment output has a
    # known missing-self-entry collision, so serialize via its public full
    # table path, then append only the changed serialized objects as a table
    # revision. No production PDF or original fixture is rewritten.
    writer.incremental=False
    full=io.BytesIO();writer.write(full);body=full.getvalue()
    control=PdfReader(io.BytesIO(body),strict=True)
    positions=sorted(control.xref[0].values())
    xref=int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",body)[1])
    output=bytearray(data+b"\n");entries={}
    for number in changed:
        offset=control.xref[0][number]
        stop=next((value for value in positions if value>offset),xref)
        block=body[offset:stop]
        assert re.match(fr"{number}\s+0\s+obj\b".encode(),block)
        entries[number]=len(output);output.extend(block)
    assert entries and len(output)-len(data)<65536
    start=len(output);output.extend(b"xref\n")
    for number,offset in entries.items():
        output.extend(f"{number} 1\n{offset:010} 00000 n \n".encode())
    trailer=D({N(key):control.trailer.raw_get(key) for key in ("/Size","/Root","/Info","/ID") if key in control.trailer})
    trailer[N("/Prev")]=I(int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",data)[1]))
    footer=io.BytesIO();trailer.write_to_stream(footer)
    output.extend(b"trailer\n"+footer.getvalue()+f"\nstartxref\n{start}\n%%EOF\n".encode())
    result=bytes(output);parsed=PdfReader(io.BytesIO(result),strict=True)
    assert len(parsed.pages)==len(control.pages)
    for number in changed:
        actual=parsed.get_object(IndirectObject(number,0,parsed))
        expected=control.get_object(IndirectObject(number,0,control))
        assert signature(actual)==signature(expected), "test control mutation not preserved"
    return result

def canonical_control(data,operation=lambda w:None):
    writer=PdfWriter(io.BytesIO(data));writer.pdf_header="%PDF-1.7";operation(writer)
    out=io.BytesIO();writer.write(out);result=out.getvalue()
    parsed=PdfReader(io.BytesIO(result),strict=True)
    assert xref_self_entries(result,parsed)
    return result
def first(writer): return writer.pages[0]["/Annots"][0].get_object()

class WriterTests(unittest.TestCase):
    def test_unchanged_save_as_preserves_every_byte_after_validation(self):
        output, summary = complete(SOURCE, SOURCE)
        self.assertEqual(output, SOURCE)
        self.assertIs(output, SOURCE)
        self.assertEqual(summary, {"targets": 0, "reusedFonts": 0, "newFonts": 0})
        self.assertTrue(xref_self_entries(output, read(output)))
        legacy=(ROOT/"tests/work/writer/matrix-first.pdf").read_bytes()
        with self.assertRaisesRegex(ValueError,"input_xref_structure"):
            complete(legacy,legacy)
    def test_own_final_xref_adapter_preserves_candidate_and_passes_structure_gate(self):
        output,summary=complete(SOURCE,CANDIDATE)
        self.assertEqual(summary["targets"],8)
        self.assertTrue(xref_self_entries(output,read(output)))
        self.assertTrue(output.startswith(CANDIDATE))
        final_offset=int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",output)[1])
        self.assertEqual(output[final_offset:final_offset+4],b"xref")
        self.assertTrue(xref_self_entries(SOURCE,read(SOURCE)))
        with self.assertRaisesRegex(ValueError,"pdf_terminal"):read(output[:-7])
    def test_old_missing_self_entry_output_is_refused_not_repaired(self):
        legacy=(ROOT/"tests/work/writer/matrix-first.pdf").read_bytes()
        self.assertFalse(xref_self_entries(legacy,read(legacy)))
        with self.assertRaisesRegex(ValueError,"input_xref_structure"):complete(legacy,legacy)
        self.assertTrue(xref_self_entries(SOURCE,read(SOURCE)))
    def test_xref_adapter_changes_only_own_final_xref_and_rejects_unknown_contract(self):
        writer=PdfWriter(io.BytesIO(SOURCE),incremental=True,strict=True)
        writer.add_metadata({"/ModDate":"D:20261014000000"})
        buffer=io.BytesIO();writer.write(buffer);generated=buffer.getvalue()
        offset=int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",generated)[1])
        fixed=final_incremental_table(SOURCE,generated,30000,16*1024*1024)
        self.assertEqual(fixed[:offset],generated[:offset])
        self.assertTrue(fixed.startswith(SOURCE));self.assertTrue(xref_self_entries(fixed,read(fixed)))
        final=read(fixed)
        self.assertEqual(final.trailer["/Prev"],int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",SOURCE)[1]))
        for generation,objects in final.xref.items():
            if generation==65535:continue
            for number,position in objects.items():
                if number:self.assertRegex(fixed[position:],fr"^{number}\s+{generation}\s+obj\b".encode())
        for previous,output in [(SOURCE,generated[:-7]),(SOURCE+b"changed",generated),(SOURCE,SOURCE)]:
            with self.assertRaisesRegex(ValueError,"generated_xref_contract"):
                final_incremental_table(previous,output,30000,16*1024*1024)
        with self.assertRaisesRegex(ValueError,"generated_xref_contract"):
            final_incremental_table(SOURCE,generated,0,16*1024*1024)
        with self.assertRaisesRegex(ValueError,"generated_xref_contract"):
            final_incremental_table(SOURCE,generated,30000,len(generated)-1)
    def test_xref_adapter_refuses_type2_generation_and_wrong_object_offsets(self):
        writer=PdfWriter(io.BytesIO(SOURCE),incremental=True,strict=True)
        writer.add_metadata({"/ModDate":"D:20261014000000"})
        buffer=io.BytesIO();writer.write(buffer);generated=buffer.getvalue()
        offset=int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",generated)[1])
        number=int(re.match(rb"(\d+)\s+0\s+obj",generated[offset:])[1])
        parsed=PdfReader(io.BytesIO(generated),strict=True)
        original=parsed.get_object(IndirectObject(number,0,parsed))
        for field,values in [(0,b"\x02"),(5,b"\x01"),(1,b"\x00\x00\x00\x01")]:
            with self.subTest(field=field):
                payload=bytearray(original.get_data());payload[field:field+len(values)]=values
                stream=DecodedStreamObject();stream.update({key:value for key,value in original.items() if key not in {"/Filter","/Length"}})
                stream.set_data(bytes(payload));encoded=stream.flate_encode();tail=io.BytesIO()
                tail.write(f"{number} 0 obj\n".encode());encoded.write_to_stream(tail)
                tail.write(f"\nendobj\nstartxref\n{offset}\n%%EOF\n".encode())
                corrupted=generated[:offset]+tail.getvalue()
                with self.assertRaisesRegex(ValueError,"generated_xref_contract"):
                    final_incremental_table(SOURCE,corrupted,30000,16*1024*1024)
    def reject(self, data, reason):
        with self.assertRaisesRegex(ValueError,reason): complete(SOURCE,data)
    def test_resource_change_cannot_hide_behind_unchanged_page_content(self):
        self.reject(change(CANDIDATE,lambda w:w.pages[0]["/Resources"]["/Font"]["/F1"].update({N("/BaseFont"):N("/Courier")})),"protected_document_changed")
    def test_geometry_change_refused(self):
        self.reject(change(CANDIDATE,lambda w:w.pages[0].update({N("/Rotate"):F(90)})),"protected_document_changed")
    def test_catalog_and_info_changes_refused(self):
        self.reject(change(CANDIDATE,lambda w:w.root_object.update({N("/Lang"):T("changed")})),"protected_document_changed")
        self.reject(change(CANDIDATE,lambda w:w.add_metadata({"/Author":"synthetic tamper"})),"protected_document_changed")
    def test_rich_text_and_effects_refused(self):
        for key,value in [("/RC",T("<p>synthetic</p>")),("/CA",F(.5)),("/BS",D({N("/W"):F(2)})),("/Border",A([F(0),F(0),F(2)]))]:
            with self.subTest(key=key): self.reject(change(CANDIDATE,lambda w:first(w).update({N(key):value})),"unsupported_freetext")
    def test_missing_glyph_and_control_refused(self):
        for value in ["bad \U0010FFFF","bad\x01"]:
            self.reject(change(CANDIDATE,lambda w:first(w).update({N("/Contents"):T(value)})),"missing_glyph|text_budget_or_control")
    def test_overflow_not_hidden_by_shrinking(self):
        self.reject(change(CANDIDATE,lambda w:first(w).update({N("/Rect"):A([F(10),F(10),F(11),F(11)])})),"text_overflow")
    def test_form_and_signature_policy_refused(self):
        self.reject(change(CANDIDATE,lambda w:w.root_object.update({N("/AcroForm"):D()})),"signed_or_form_document")
    def test_nonincremental_rebuild_refused(self):
        writer=PdfWriter(io.BytesIO(CANDIDATE));out=io.BytesIO();writer.write(out)
        self.reject(out.getvalue(),"not_incremental")
    def test_untouched_foreign_standard_annotations_remain_identical(self):
        source=(ROOT/"tests/work/security-close/round2/complex-v17-source.pdf").read_bytes()
        candidate=change(source,lambda w:w.add_annotation(0,D({N("/Type"):N("/Annot"),N("/Subtype"):N("/FreeText"),N("/Contents"):T("中文 α ≤ 10"),N("/DA"):T("/Helv 12 Tf .2 .4 .6 rg"),N("/Rect"):A([F(40),F(300),F(250),F(370)]),N("/NM"):T("worker-added")})))
        output,summary=complete(source,candidate);before,after=annotations(read(source)),annotations(read(output))
        self.assertEqual(summary["targets"],1);self.assertEqual(protected_shape(read(source)),protected_shape(read(output)))
        for key,(_,_,a) in before.items():
            self.assertEqual(signature({k:v for k,v in a.items() if k!="/P"}),signature({k:v for k,v in after[key][2].items() if k!="/P"}))
    def test_new_glyphs_can_add_one_shared_font(self):
        first_output=canonical_control((ROOT/"tests/work/writer/matrix-first.pdf").read_bytes())
        candidate=change(first_output,lambda w:first(w).update({N("/Contents"):T("新增文字 Ω ≥ 10")}))
        output,report=complete(first_output,candidate)
        self.assertEqual(report["newFonts"],1);self.assertTrue(output.startswith(candidate))
    def test_fractional_rectangles_keep_geometry_without_false_exact_float_failures(self):
        candidate=change(CANDIDATE,lambda w:first(w).update({N("/Rect"):A([F(70.1234567),F(100.2222222),F(250.8888888),F(260.4444444)])}))
        output,summary=complete(SOURCE,candidate);self.assertEqual(summary["targets"],8);self.assertTrue(output.startswith(candidate))
    def test_bookmark_destination_is_preserved_without_requiring_immutable_annotation_graph(self):
        writer=PdfWriter(io.BytesIO(SOURCE));writer.pdf_header="%PDF-1.7";writer.add_outline_item("Synthetic bookmark",0);buffer=io.BytesIO();writer.write(buffer);source=buffer.getvalue()
        candidate=change(source,lambda w:w.add_annotation(0,D({N("/Type"):N("/Annot"),N("/Subtype"):N("/FreeText"),N("/Contents"):T("中文 α ≤ 10"),N("/DA"):T("/Helv 12 Tf 0 g"),N("/Rect"):A([F(40),F(300),F(250),F(370)])})))
        output,summary=complete(source,candidate);self.assertEqual(summary["targets"],1);self.assertEqual(protected_shape(read(source)),protected_shape(read(output)))
    def test_serializer_removing_foreign_rich_text_cannot_silently_reduce_source_format(self):
        source=canonical_control((ROOT/"tests/work/writer/matrix-first.pdf").read_bytes(),lambda w:first(w).update({N("/RC"):T("<p>synthetic rich text</p>")}))
        def stripped(w):
            del first(w)["/RC"];first(w)[N("/Contents")]=T("中文 α ≤ 10")
        candidate=change(source,stripped)
        with self.assertRaisesRegex(ValueError,"unsupported_source_freetext_format"):complete(source,candidate)
    def test_helv_resource_name_cannot_disguise_a_foreign_font(self):
        def disguised(w):
            a=first(w);a[N("/DA")]=T("/Helv 12 Tf 0 g")
            a["/AP"]["/N"]["/Resources"]["/Font"][N("/Helv")]=D({N("/Type"):N("/Font"),N("/Subtype"):N("/Type1"),N("/BaseFont"):N("/Courier")})
        source=canonical_control((ROOT/"tests/work/writer/matrix-first.pdf").read_bytes(),disguised)
        candidate=change(source,lambda w:first(w).update({N("/Contents"):T("中文 α ≤ 10")}))
        with self.assertRaisesRegex(ValueError,"unsupported_source_font"):complete(source,candidate)
    def test_library_repeated_stream_increment_collision_is_refused_by_protocol(self):
        source=(ROOT/"tests/work/writer/matrix-first.pdf").read_bytes()
        writer=PdfWriter(io.BytesIO(source),incremental=True)
        first(writer)[N("/Contents")]=T("新增文字 Ω ≥ 10")
        output=io.BytesIO();writer.write(output);candidate=output.getvalue()
        def xref_id(data):
            offset=int(re.search(rb"startxref\s+(\d+)\s+%%EOF\s*\Z",data)[1])
            return int(re.match(rb"(\d+)\s+0\s+obj",data[offset:])[1])
        self.assertEqual(xref_id(source),xref_id(candidate))
        self.assertFalse(xref_self_entries(source,read(source)))
        header={"schema":1,"taskId":"00000000-0000-0000-0000-000000000000",
            "sourceHash":hashlib.sha256(source).hexdigest(),"candidateHash":hashlib.sha256(candidate).hexdigest()}
        blob=json.dumps(header).encode();packet=b"AUPW\x01\0\0\0"+struct.pack("<III",len(blob),len(source),len(candidate))+blob+source+candidate
        lock=json.loads((ROOT/"scripts/python-worker-dependencies.json").read_text("utf-8"))["pypdfWheel"]
        wheel=ROOT/".cache/python-wheels"/lock["filename"]
        code=f"import sys,runpy;sys.path.insert(0,{str(wheel)!r});runpy.run_path({str(ROOT/'worker/main.py')!r},run_name='__main__')"
        result=subprocess.run([sys.executable,"-I","-B","-X","utf8","-c",code],input=packet,capture_output=True,timeout=10)
        self.assertEqual(result.returncode,2);self.assertEqual(result.stdout,b"")
        self.assertIn(json.loads(result.stderr)["error"],["parser_warning","input_xref_structure"])
    def test_protocol_refuses_truncation_bad_hash_unknown_paths_and_trailing_data(self):
        header={"schema":1,"taskId":"00000000-0000-0000-0000-000000000000","sourceHash":"x","candidateHash":"x"}
        def packet(h):
            blob=json.dumps(h).encode();return b"AUPW\x01\0\0\0"+struct.pack("<III",len(blob),len(SOURCE),len(CANDIDATE))+blob+SOURCE+CANDIDATE
        for blob in [b"broken",packet(header),packet({**header,"path":"C:/outside"}),packet(header)+b"extra"]:
            result=subprocess.run([sys.executable,"-I","-B",str(ROOT/"worker/main.py")],input=blob,capture_output=True,timeout=10)
            self.assertEqual(result.returncode,2);self.assertEqual(result.stdout,b"");error=json.loads(result.stderr)
            self.assertFalse(error["ok"]);self.assertNotIn("C:/",result.stderr.decode())

if __name__=="__main__":unittest.main()
