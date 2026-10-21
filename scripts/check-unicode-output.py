"""Separate object census of known controls; no PDF writes or cleaning."""
import io
import json
from pathlib import Path
from pypdf import PdfReader
from pypdf.generic import ContentStream,IndirectObject
ROOT=Path(__file__).resolve().parent.parent
WORK=ROOT/'tests/work/writer'
def census(path):
    data=path.read_bytes();pdf=PdfReader(io.BytesIO(data),strict=True);files=set();forms=set();texts=[]
    refs={(n,g) for g,entries in pdf.xref.items() if g!=65535 for n,o in entries.items() if n and o}
    refs.update((n,0) for n in pdf.xref_objStm)
    for n,g in refs:
        item=pdf.get_object(IndirectObject(n,g,pdf))
        if isinstance(item,dict) and item.get('/Subtype')=='/Form':forms.add((n,g))
        if isinstance(item,dict) and '/FontFile2' in item:
            f=item.raw_get('/FontFile2');files.add((f.idnum,f.generation))
    for index,page in enumerate(pdf.pages):
        for reference in page.get('/Annots',[]):
            a=reference.get_object();assert a['/Subtype']=='/FreeText'
            ap=a['/AP']['/N'];assert ap['/Subtype']=='/Form' and '/Resources' in ap
            ops=ContentStream(ap,pdf).operations
            assert any(op in {b'Tj',b'TJ'} for _,op in ops) and not any(op in {b'Do',b'BI'} for _,op in ops)
            fonts=ap['/Resources']['/Font'];embedded=[]
            for f in fonts.values():
                f=f.get_object();descriptor=f.get('/FontDescriptor',{})
                if hasattr(descriptor,'get_object'):descriptor=descriptor.get_object()
                if '/FontFile2' in descriptor:
                    assert '/ToUnicode' in f;embedded.append(f)
            assert len(embedded)==1
            texts.append({'page':index+1,'object':[reference.idnum,reference.generation],'contents':str(a['/Contents']),
                'rect':list(a['/Rect']),'rotate':a.get('/Rotate',0),'bbox':list(ap['/BBox']),'matrix':list(ap['/Matrix'])})
    return {'bytes':len(data),'embeddedFontStreams':len(files),'retainedForms':len(forms),'annotations':texts}
first,final=census(WORK/'matrix-first.pdf'),census(WORK/'matrix-final.pdf')
assert first['embeddedFontStreams']==final['embeddedFontStreams']==1
assert len(first['annotations'])==len(final['annotations'])==8
source=PdfReader(str(WORK/'matrix-source.pdf'),strict=True);out=PdfReader(str(WORK/'matrix-final.pdf'),strict=True)
for a,b in zip(source.pages,out.pages):
    assert a.get_contents().get_data()==b.get_contents().get_data()
    assert list(a.mediabox)==list(b.mediabox) and list(a.cropbox)==list(b.cropbox) and a.rotation==b.rotation
report={'status':'PASS','scope':'independent known-object census, not external reader or GUI acceptance','first':first,'final':final,'pageBodyAndGeometryPreserved':True}
(WORK/'objects.json').write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding='utf-8');print(json.dumps({'status':'PASS','firstBytes':first['bytes'],'finalBytes':final['bytes'],'fontStreams':[1,1],'forms':[first['retainedForms'],final['retainedForms']]}))
