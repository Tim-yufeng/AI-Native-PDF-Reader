//! Existing bounded structural preflight, shared with the save regression.
//! Complements independent PDF.js reopening; deliberately refuses richer PDFs.
use std::{collections::BTreeMap,io::{self,Read}};
// Narrow canonical xref-stream preflight for independently parsed controls,
// NOT a general PDF validator. Only a bounded integer/name/array dictionary is
// supported; richer trailers refuse. The actual entry bytes must resolve to
// matching newly appended indirect objects, not merely contain plausible keys.
#[derive(Debug,PartialEq)]enum XValue{Number(usize),Name(String),Reference(usize,usize),Array(Vec<XValue>),Hex,Literal(usize)}
/// A literal string travels through the token list as its own byte length, in a
/// form no other token can produce. `/ID` strings are 16 bytes and every real
/// producer stays far inside this bound, so a huge one is refused, not honoured.
const LITERAL_PREFIX:&str="\u{1}lit:";
const LITERAL_MAX_BYTES:usize=255;
pub(crate) fn x_error()->io::Error{io::Error::other("unsupported/invalid canonical xref")}
// Fixed diagnostic vocabulary only: never include PDF bytes, paths or object
// contents. The caller may surface this rule in an isolated diagnostic build.
fn rule(code:&'static str)->io::Error{io::Error::other(code)}
/// Allowlisted identifiers for redacted production diagnostics. Do not expose
/// `io::Error` text: parser/decoder errors can contain input-derived details.
pub(crate) fn diagnostic_rule(error:&io::Error)->&'static str{
 const RULES:&[&str]=&[
  "I01_source_prefix","I02_pdf_header","I03_source_terminal","I04_source_xref_bounds",
  "I05_source_trailer_missing","I06_source_dictionary_boundary","I07_source_dictionary_syntax",
  "I08_source_encrypted","I09_source_root","I10_output_terminal","I11_chain_source_bounds",
  "I12_chain_order","I13_revision_budget",
  "T01_trailer_boundary","T02_dictionary_syntax","T03_dictionary_fields","T04_previous_revision",
  "T05_size","T06_terminal","T07_ranges","T08_entry_shape","T09_free_entry","T10_entry_type",
  "T11_live_entry_bounds","T12_live_entry_identity",
  "S01_object_boundary","S02_dictionary_syntax","S03_dictionary_fields","S04_previous_revision",
  "S05_size","S06_width_ranges","S07_stream_encoding","S08_entry_shape","S09_free_entry",
  "S10_entry_type","S11_live_entry_bounds","S12_live_entry_identity","S13_terminal",
 ];
 let message=error.to_string();RULES.iter().copied().find(|code|*code==message).unwrap_or("I00_unclassified")
}
/// Tokenizes a dictionary *by byte*. A `/ID` may hold arbitrary binary, so the
/// buffer is not necessarily valid UTF-8 and must never be decoded as a whole;
/// only the individual tokens are built as text.
fn x_tokens(data:&[u8])->io::Result<Vec<String>>{
 let mut at=0usize;let mut result=Vec::new();
 while at<data.len(){
  let c=data[at];
  if c.is_ascii_whitespace(){at+=1;continue;}
  let token=match c{
   b'['|b']'=>{at+=1;(c as char).to_string()},
   b'<'=>{if data.get(at+1)==Some(&b'<'){at+=2;"<<".to_owned()}else{
     at+=1;let mut hex=String::from("<");loop{let ch=*data.get(at).ok_or_else(||io::Error::other(format!("hex-eof at {at}")))?;at+=1;hex.push(ch as char);if ch==b'>'{break;}if !ch.is_ascii_hexdigit()&&!ch.is_ascii_whitespace(){return Err(io::Error::other(format!("hex-byte 0x{ch:02x} at {at}")));}}hex}},
   // A literal string is one object however many bytes it holds. Bytes are only
   // counted, so binary stays opaque, and a balanced parenthesis or a backslash
   // escape is stepped over instead of ending the string.
   b'('=>{at+=1;let mut bytes=0usize;let mut depth=0usize;loop{let ch=*data.get(at).ok_or_else(x_error)?;at+=1;match ch{
     b'\\'=>{at+=1;if at>data.len(){return Err(x_error());}bytes+=1;}
     b'('=>{depth+=1;bytes+=1;}
     b')'=>{if depth==0{break;}depth-=1;bytes+=1;}
     _=>bytes+=1,}
     if bytes>LITERAL_MAX_BYTES{return Err(x_error());}}
    format!("{LITERAL_PREFIX}{bytes}")},
   b'>'=>{at+=1;if data.get(at)!=Some(&b'>'){return Err(io::Error::other(format!("lone-gt at {at} next=0x{:02x}",data.get(at).copied().unwrap_or(0))));}at+=1;">>".to_owned()},
   b'/'|b'0'..=b'9'|b'A'..=b'Z'|b'a'..=b'z'=>{
     // The first byte is consumed before the loop; a leading delimiter such as
     // `/` must advance the cursor, or the scan never moves forward.
     let mut word=String::from(c as char);at+=1;
     while let Some(&ch)=data.get(at){
      if ch.is_ascii_whitespace()||matches!(ch,b'['|b']'|b'<'|b'>'|b'/'){break;}
      if !ch.is_ascii_alphanumeric(){return Err(x_error());}
      word.push(ch as char);at+=1;}
     word},
   _=>return Err(io::Error::other(format!("token-byte 0x{c:02x} at {at}")))};
  result.push(token);if result.len()>4096{return Err(x_error());}
 }Ok(result)
}
fn x_value(tokens:&[String],index:&mut usize,depth:usize)->io::Result<XValue>{
 if depth>2{return Err(x_error());}let value=tokens.get(*index).ok_or_else(x_error)?;*index+=1;
 if value=="["{let mut items=Vec::new();while tokens.get(*index).map(String::as_str)!=Some("]"){items.push(x_value(tokens,index,depth+1)?);if items.len()>4096{return Err(x_error());}}*index+=1;return Ok(XValue::Array(items));}
 if value.starts_with('/')&&value.len()>1{return Ok(XValue::Name(value.clone()));}
 if value.starts_with('<')&&value!="<<"{return Ok(XValue::Hex);}
 if let Some(count)=value.strip_prefix(LITERAL_PREFIX){return Ok(XValue::Literal(count.parse::<usize>().map_err(|_|x_error())?));}
 let number=value.parse::<usize>().map_err(|_|x_error())?;
 if tokens.get(*index+1).map(String::as_str)==Some("R"){let generation=tokens[*index].parse::<usize>().map_err(|_|x_error())?;*index+=2;return Ok(XValue::Reference(number,generation));}Ok(XValue::Number(number))
}
fn x_dictionary(data:&[u8])->io::Result<BTreeMap<String,XValue>>{
 if data.len()>4096{return Err(x_error());}let tokens=x_tokens(data)?;let mut index=0;if tokens.first().map(String::as_str)!=Some("<<"){return Err(x_error());}index+=1;let mut map=BTreeMap::new();
 while tokens.get(index).map(String::as_str)!=Some(">>"){
  let key=tokens.get(index).ok_or_else(x_error)?.clone();index+=1;if !key.starts_with('/')||key.len()<2{return Err(x_error());}let value=x_value(&tokens,&mut index,0)?;if map.insert(key,value).is_some(){return Err(x_error());}
 }index+=1;if index!=tokens.len(){return Err(x_error());}Ok(map)
}
fn x_number(map:&BTreeMap<String,XValue>,key:&str)->io::Result<usize>{match map.get(key){Some(XValue::Number(n))=>Ok(*n),_=>Err(x_error())}}
/// Locate a dictionary boundary, not a byte pattern: a legal binary /ID literal
/// can contain `>>`, `<<`, escaped parentheses, or backslashes. The former first
/// `>>` search truncated those strings before x_dictionary could parse them.
/// Syntax/allowed keys remain checked by x_dictionary after this bounded scan.
fn dictionary_end(data:&[u8])->Option<usize>{
 if !data.starts_with(b"<<"){return None;}
 let data=&data[..data.len().min(4096)];let(mut at,mut dictionaries)=(2usize,1usize);
 while at<data.len(){match data[at]{
  b'('=>{at+=1;let mut depth=1usize;while depth>0{match *data.get(at)?{
   b'\\'=>{at+=2;if at>data.len(){return None;}},
   b'('=>{depth+=1;at+=1;},b')'=>{depth-=1;at+=1;},_=>at+=1,
  }}},
  b'<'=>{at+=1;if data.get(at)==Some(&b'<'){dictionaries+=1;at+=1;}else{
   while *data.get(at)?!=b'>'{at+=1;}at+=1;
  }},
  b'>'=>{if data.get(at+1)!=Some(&b'>'){return None;}dictionaries-=1;at+=2;if dictionaries==0{return Some(at);}},
  b'%'=>{while data.get(at).is_some_and(|v|!matches!(*v,b'\r'|b'\n')){at+=1;}},
  _=>at+=1,
 }}None
}
fn x_array(map:&BTreeMap<String,XValue>,key:&str)->io::Result<Vec<usize>>{match map.get(key){Some(XValue::Array(a))=>a.iter().map(|v|match v{XValue::Number(n)=>Ok(*n),_=>Err(x_error())}).collect(),_=>Err(x_error())}}
impl XValue{
 /// One half of a `/ID` array. Both spellings are legal PDF for the same bytes: a
 /// hexadecimal string, or a literal string. A real document does not keep one
 /// spelling throughout - the renderer rewrites a non-ASCII identifier as a
 /// literal binary string, and the incremental revision after it carries that
 /// forward - so demanding one spelling refuses a file that every writer and
 /// reader in the chain treats as valid. The shape is still enforced: exactly two
 /// strings and nothing else.
 fn is_id_string(&self)->bool{match self{XValue::Hex=>true,XValue::Literal(bytes)=>*bytes>0&&*bytes<=LITERAL_MAX_BYTES,_=>false}}
}
fn original_root(old:&[u8],offset:usize)->io::Result<XValue>{
 let tail=old.get(offset..).ok_or_else(||rule("I04_source_xref_bounds"))?;
 let prefix=if tail.starts_with(b"xref"){let start=tail.windows(7).position(|w|w==b"trailer").ok_or_else(||rule("I05_source_trailer_missing"))?+7;&tail[start..]}else{tail};
 let start=prefix.windows(2).position(|w|w==b"<<").ok_or_else(||rule("I06_source_dictionary_boundary"))?;let end=dictionary_end(&prefix[start..]).ok_or_else(||rule("I06_source_dictionary_boundary"))?+start;
 let map=x_dictionary(&prefix[start..end]).map_err(|_|rule("I07_source_dictionary_syntax"))?;if map.contains_key("/Encrypt"){return Err(rule("I08_source_encrypted"));}
 match map.get("/Root"){Some(XValue::Reference(n,g))=>Ok(XValue::Reference(*n,*g)),_=>Err(rule("I09_source_root"))}
}
pub(crate) fn last_xref(data:&[u8])->io::Result<usize>{
 let start=data.windows(9).rposition(|w|w==b"startxref").ok_or_else(||io::Error::other("startxref missing"))?;
 let tail=std::str::from_utf8(&data[start..]).map_err(io::Error::other)?;
 let fields:Vec<_>=tail.split_whitespace().collect();
 if fields.len()!=3||fields[0]!="startxref"||fields[2]!="%%EOF"{return Err(io::Error::other("terminal xref/EOF invalid"));}
 let offset=fields[1].parse::<usize>().map_err(io::Error::other)?;
 if offset>=start{return Err(io::Error::other("xref offset invalid"));}Ok(offset)
}
/// Accepted version headers. The earlier gate demanded `%PDF-1.5`..`1.9` or
/// `2.0` because a canonical xref *stream* needs at least 1.5, but that message
/// described a structure this preflight never actually relies on: `xref_revision`
/// dispatches on what each revision really contains and walks a classic xref table
/// just as strictly. Demanding 1.5 therefore refused ordinary `%PDF-1.3` and
/// `%PDF-1.4` documents that were otherwise fully verifiable. A file whose
/// structure cannot be walked still fails closed below, so the header only has to
/// be one we recognise.
fn supported_header(data:&[u8])->bool{
 if !data.starts_with(b"%PDF-"){return false;}
 let Some(rest)=data.get(5..9) else{return false;};
 if &rest[..3]==b"2.0"{return true;}
 rest[0]==b'1'&&rest[1]==b'.'&&rest[2].is_ascii_digit()
}
pub(crate) fn incremental_preflight(old:&[u8],new:&[u8])->io::Result<()>{
 if !new.starts_with(old)||new.len()<=old.len(){return Err(rule("I01_source_prefix"));}
 if !supported_header(new){return Err(rule("I02_pdf_header"));}
 let prior=last_xref(old).map_err(|_|rule("I03_source_terminal"))?;let root=original_root(old,prior)?;let mut offset=last_xref(new).map_err(|_|rule("I10_output_terminal"))?;
 // The Reader serializes one revision and Unicode completion appends another.
 // Walk every new revision back to the held source, with a bounded, strictly
 // decreasing Prev chain. A valid final dictionary alone is insufficient.
 for _ in 0..64{
  if offset<old.len(){return Err(rule("I11_chain_source_bounds"));}
  let previous=xref_revision(old,new,offset,&root)?;
  if previous==prior{return Ok(());}if previous>=offset{return Err(rule("I12_chain_order"));}offset=previous;
 }Err(rule("I13_revision_budget"))
}
fn xref_terminal(tail:&[u8],offset:usize,stream:bool)->io::Result<()> {
 let mut cursor=0;let mut tokens=Vec::new();for _ in 0..if stream{5}else{3}{
  while tail.get(cursor).is_some_and(u8::is_ascii_whitespace){cursor+=1;}let start=cursor;
  while tail.get(cursor).is_some_and(|v|!v.is_ascii_whitespace()){cursor+=1;if cursor-start>64{return Err(x_error());}}
  if cursor==start{return Err(x_error());}tokens.push(std::str::from_utf8(&tail[start..cursor]).map_err(|_|x_error())?);
 }
 let index=if stream{if tokens[0]!="endstream"||tokens[1]!="endobj"{return Err(x_error());}2}else{0};
 if tokens[index]!="startxref"||tokens[index+1].parse::<usize>().map_err(|_|x_error())?!=offset||tokens[index+2]!="%%EOF"{return Err(x_error());}Ok(())
}
fn xref_table_revision(old:&[u8],new:&[u8],offset:usize,root:&XValue)->io::Result<usize>{
 let tail=&new[offset..];let trailer=tail.windows(7).position(|w|w==b"trailer").ok_or_else(||rule("T01_trailer_boundary"))?;
 if trailer>128*1024{return Err(rule("T01_trailer_boundary"));}let text=std::str::from_utf8(&tail[..trailer]).map_err(|_|rule("T08_entry_shape"))?;let tokens:Vec<_>=text.split_whitespace().collect();
 if tokens.first()!=Some(&"xref"){return Err(rule("T01_trailer_boundary"));}let rest=&tail[trailer+7..];let begin=rest.windows(2).position(|w|w==b"<<").ok_or_else(||rule("T01_trailer_boundary"))?;
 let end=dictionary_end(&rest[begin..]).ok_or_else(||rule("T01_trailer_boundary"))?+begin;let dict=x_dictionary(&rest[begin..end]).map_err(|_|rule("T02_dictionary_syntax"))?;
 if dict.keys().any(|k|!matches!(k.as_str(),"/Size"|"/Root"|"/Prev"|"/ID"|"/Info"))||dict.get("/Root")!=Some(root){return Err(rule("T03_dictionary_fields"));}
 if let Some(value)=dict.get("/Info"){if !matches!(value,XValue::Reference(n,g)if *n>0&&*g<=65535){return Err(rule("T03_dictionary_fields"));}}
 if let Some(value)=dict.get("/ID"){if !matches!(value,XValue::Array(items)if items.len()==2&&items.iter().all(XValue::is_id_string)){return Err(rule("T03_dictionary_fields"));}}
 let size=x_number(&dict,"/Size").map_err(|_|rule("T05_size"))?;if size>100000{return Err(rule("T05_size"));}let previous=x_number(&dict,"/Prev").map_err(|_|rule("T04_previous_revision"))?;if previous>=offset{return Err(rule("T04_previous_revision"));}
 xref_terminal(&rest[end..],offset,false).map_err(|_|rule("T06_terminal"))?;let mut index=1;let mut count=0;let mut last=0;
 while index<tokens.len(){let start=tokens[index].parse::<usize>().map_err(|_|rule("T07_ranges"))?;let length=tokens.get(index+1).ok_or_else(||rule("T07_ranges"))?.parse::<usize>().map_err(|_|rule("T07_ranges"))?;index+=2;
  let finish=start.checked_add(length).ok_or_else(||rule("T07_ranges"))?;if length==0||start<last||finish>size||count+length>4096{return Err(rule("T07_ranges"));}last=finish;count+=length;
  for number in start..finish{let location=tokens.get(index).ok_or_else(||rule("T08_entry_shape"))?;let generation=tokens.get(index+1).ok_or_else(||rule("T08_entry_shape"))?;let kind=tokens.get(index+2).ok_or_else(||rule("T08_entry_shape"))?;index+=3;
   if location.len()!=10||generation.len()!=5{return Err(rule("T08_entry_shape"));}let target=location.parse::<usize>().map_err(|_|rule("T08_entry_shape"))?;let generation=generation.parse::<usize>().map_err(|_|rule("T08_entry_shape"))?;
   // PDF.js emits one free entry for each removed existing annotation. These
   // entries have no object body/byte offset: field 2 is the next-free number.
   // Admit its bounded canonical form only, retaining strict live-object checks.
   if *kind=="f"{if number==0||matches!(root,XValue::Reference(n,_)if *n==number)||target!=0||generation==0||generation>65535{return Err(rule("T09_free_entry"));}continue;}
   if *kind!="n"{return Err(rule("T10_entry_type"));}
   if target<old.len()||target>=offset||generation>65535{return Err(rule("T11_live_entry_bounds"));}let identity=format!("{number} {generation} obj");let at=new.get(target..).ok_or_else(||rule("T11_live_entry_bounds"))?;
   if !at.starts_with(identity.as_bytes())||!at.get(identity.len()).is_some_and(|v|v.is_ascii_whitespace()||*v==b'<'){return Err(rule("T12_live_entry_identity"));}
  }
 }if count==0{return Err(rule("T07_ranges"));}Ok(previous)
}
fn xref_revision(old:&[u8],new:&[u8],offset:usize,root:&XValue)->io::Result<usize>{
 if new.get(offset..).is_some_and(|v|v.starts_with(b"xref")){return xref_table_revision(old,new,offset,root);}
 let tail=&new[offset..];let begin=tail.windows(2).position(|w|w==b"<<").ok_or_else(||rule("S01_object_boundary"))?;let end=dictionary_end(&tail[begin..]).ok_or_else(||rule("S01_object_boundary"))?+begin;
 let object=x_tokens(&tail[..begin]).map_err(|_|rule("S01_object_boundary"))?;if object.len()!=3||object[2]!="obj"{return Err(rule("S01_object_boundary"));}let object_id=object[0].parse::<usize>().map_err(|_|rule("S01_object_boundary"))?;if object[1]!="0"{return Err(rule("S01_object_boundary"));}
 let dict=x_dictionary(&tail[begin..end]).map_err(|_|rule("S02_dictionary_syntax"))?;
 if dict.keys().any(|k|!matches!(k.as_str(),"/Type"|"/Size"|"/Root"|"/Filter"|"/Index"|"/W"|"/Prev"|"/ID"|"/Length"|"/Info")){return Err(rule("S03_dictionary_fields"));}
 if dict.get("/Type")!=Some(&XValue::Name("/XRef".to_owned()))||dict.get("/Root")!=Some(root){return Err(rule("S03_dictionary_fields"));}
 if let Some(value)=dict.get("/Info"){if !matches!(value,XValue::Reference(n,g)if *n>0&&*g<=65535){return Err(rule("S03_dictionary_fields"));}}
 if let Some(value)=dict.get("/ID"){if !matches!(value,XValue::Array(items)if items.len()==2&&items.iter().all(XValue::is_id_string)){return Err(rule("S03_dictionary_fields"));}}
 let previous=x_number(&dict,"/Prev").map_err(|_|rule("S04_previous_revision"))?;if previous>=offset{return Err(rule("S04_previous_revision"));}
 let size=x_number(&dict,"/Size").map_err(|_|rule("S05_size"))?;if size!=object_id.checked_add(1).ok_or_else(||rule("S05_size"))?||size>100000{return Err(rule("S05_size"));}
 let width=x_array(&dict,"/W").map_err(|_|rule("S06_width_ranges"))?;let ranges=x_array(&dict,"/Index").map_err(|_|rule("S06_width_ranges"))?;
 if width.len()!=3||width.iter().any(|n|*n>8)||width.iter().sum::<usize>()==0||ranges.is_empty()||ranges.len()%2!=0{return Err(rule("S06_width_ranges"));}
 let mut objects=Vec::new();let mut last=0;for range in ranges.chunks_exact(2){let finish=range[0].checked_add(range[1]).ok_or_else(||rule("S06_width_ranges"))?;if range[0]<last||range[1]==0||finish>size||objects.len()+range[1]>4096{return Err(rule("S06_width_ranges"));}objects.extend(range[0]..finish);last=finish;}
 let remaining=&tail[end..];let stream_start=remaining.iter().position(|c|!c.is_ascii_whitespace()).ok_or_else(||rule("S07_stream_encoding"))?;
 let marker=&remaining[stream_start..];let eol=if marker.starts_with(b"stream\r\n"){8}else if marker.starts_with(b"stream\n"){7}else{return Err(rule("S07_stream_encoding"));};
 let start=end+stream_start+eol;let length=x_number(&dict,"/Length").map_err(|_|rule("S07_stream_encoding"))?;let finish=start.checked_add(length).ok_or_else(||rule("S07_stream_encoding"))?;let compressed=tail.get(start..finish).ok_or_else(||rule("S07_stream_encoding"))?;
 xref_terminal(tail.get(finish..).ok_or_else(||rule("S13_terminal"))?,offset,true).map_err(|_|rule("S13_terminal"))?;
 let expected=objects.len().checked_mul(width.iter().sum()).ok_or_else(||rule("S06_width_ranges"))?;let mut entries=Vec::new();
 match dict.get("/Filter"){
  None=>entries.extend_from_slice(compressed),
  Some(XValue::Name(name))if name=="/FlateDecode"=>{let mut decoder=flate2::read::ZlibDecoder::new(compressed);decoder.by_ref().take((expected+1)as u64).read_to_end(&mut entries).map_err(|_|rule("S07_stream_encoding"))?;if decoder.total_in()!=compressed.len()as u64{return Err(rule("S07_stream_encoding"));}},
  _=>return Err(rule("S07_stream_encoding"))}
 if entries.len()!=expected{return Err(rule("S08_entry_shape"));}let mut cursor=0;
 for number in objects{let mut fields=[0u64;3];for (index,count)in width.iter().enumerate(){for byte in &entries[cursor..cursor+count]{fields[index]=fields[index]*256+*byte as u64;}cursor+=count;}
  let kind=if width[0]==0{1}else{fields[0]};let target=usize::try_from(fields[1]).map_err(|_|rule("S08_entry_shape"))?;
  if kind==0{if number==0||number==object_id||matches!(root,XValue::Reference(n,_)if *n==number)||target!=0||fields[2]==0||fields[2]>65535{return Err(rule("S09_free_entry"));}continue;}
  if kind!=1{return Err(rule("S10_entry_type"));}if fields[2]>65535||target<old.len()||target>offset{return Err(rule("S11_live_entry_bounds"));}
  let identity=format!("{number} {} obj",fields[2]);let at=new.get(target..).ok_or_else(||rule("S11_live_entry_bounds"))?;if !at.starts_with(identity.as_bytes())||!at.get(identity.len()).is_some_and(|v|v.is_ascii_whitespace()||*v==b'<'){return Err(rule("S12_live_entry_identity"));}
 }
 Ok(previous)
}
#[cfg(test)]pub(crate) mod tests{
 use super::*;
 fn project()->std::path::PathBuf{std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()}
 #[test]fn literal_dictionary_delimiters_are_opaque_in_source_and_new_revision(){
  for id in [
   b"/ID [(012345>>89abcdef)(0123456789abcdef)]".as_slice(),
   b"/ID [(012345<<89abcdef)(0123456789abcdef)]",
   b"/ID [(abc\\)>>(nested)def)(0123456789abcdef)]",
   b"/ID [(abc\\\\>>def)(0123456789abcdef)]",
  ]{
   let source=third_party_pdf("%PDF-1.4",6,1,5);
   let first=renderer_revision(&source,6,"<< /Type /Font >>",&lf_trailer(last_xref(&source).unwrap(),7,id));
   // Before the fix, dictionary_end stopped inside the first literal /ID.
   assert!(incremental_preflight(&source,&first).is_ok());
   let second=appended_revision(&first,7,1);
   assert!(incremental_preflight(&first,&second).is_ok());
  }
  for input in [b"<< /ID [(unclosed >>".as_slice(),b"<< /ID [<1234] >>",b"<< /Size 1 >",b"< /Size 1 >>"]{
   assert_eq!(dictionary_end(input),None);
  }
 }
 fn free_revision(old:&[u8],stream:bool,number:usize,next:usize,generation:usize)->Vec<u8>{
  let prior=last_xref(old).unwrap();let mut out=old.to_vec();let offset=out.len();
  if stream{
   // Include the xref stream's own live entry alongside the free entry.
   let mut entries=vec![0u8];entries.extend_from_slice(&(next as u32).to_be_bytes());entries.extend_from_slice(&(generation as u32).to_be_bytes());
   entries.push(1);entries.extend_from_slice(&(offset as u32).to_be_bytes());entries.extend_from_slice(&0u32.to_be_bytes());
   out.extend_from_slice(format!("10 0 obj\n<< /Type /XRef /Size 11 /Root 1 0 R /Prev {prior} /W [1 4 4] /Index [{number} 1 10 1] /Length {} >>\nstream\n",entries.len()).as_bytes());
   out.extend_from_slice(&entries);out.extend_from_slice(b"\nendstream\nendobj\n");
  }else{
   out.extend_from_slice(format!("xref\n{number} 1\n{next:010} {generation:05} f \ntrailer\n<< /Size 11 /Root 1 0 R /Prev {prior} >>\n").as_bytes());
  }
  out.extend_from_slice(format!("startxref\n{offset}\n%%EOF\n").as_bytes());out
 }
 #[test]fn removed_objects_accept_only_bounded_canonical_free_entries(){
  let old=third_party_pdf("%PDF-1.4",6,1,5);
  for stream in [false,true]{
   assert!(incremental_preflight(&old,&free_revision(&old,stream,5,0,1)).is_ok());
   for (number,next,generation) in [(0,0,65535),(1,0,1),(5,1,1),(5,0,0),(5,0,65536)]{
    let error=incremental_preflight(&old,&free_revision(&old,stream,number,next,generation)).unwrap_err();
    assert_eq!(diagnostic_rule(&error),if stream{"S09_free_entry"}else{"T09_free_entry"});
   }
  }
 }
 #[test]fn diagnostics_never_expose_input_or_paths(){
  assert_eq!(diagnostic_rule(&io::Error::other("secret path or PDF content")),"I00_unclassified");
  let old=third_party_pdf("%PDF-1.4",6,1,5);let mut bad=appended_revision(&old,6,1);bad[0]=b'x';
  assert_eq!(diagnostic_rule(&incremental_preflight(&old,&bad).unwrap_err()),"I01_source_prefix");
 }
 #[test]#[ignore]fn probe_actual_writer_repeated_revisions(){
  let dir=project().join("tests/work/stability-save-probe");
  let mut failures=Vec::new();let mut count=0;
  for label in ["classic-13","classic-14","xref-stream","literal-delimiter","ordinary","classic-delete","stream-delete"]{
   for round in 1..=10{
    let stem=format!("{label}-{round}");
    let old=std::fs::read(dir.join(format!("{stem}-old.pdf"))).expect("run node scripts/probe-save-revisions.mjs first");
    let new=std::fs::read(dir.join(format!("{stem}-output.pdf"))).unwrap();
    count+=1;
    if let Err(error)=incremental_preflight(&old,&new){failures.push(format!("{stem}: {error}"));}
   }
  }
  println!("PROBE checked={count} rejected={} failures={failures:?}",failures.len());
  assert!(failures.is_empty());
 }
 /// Read-only verdict over one authorised real failing pair, when the disposable
 /// copies are present under `.cache/diag`. They are user-supplied and never
 /// committed, so the test simply reports that it has nothing to check.
 #[test]#[ignore]fn probe_authorised_real_pair(){
  let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".cache/diag");
  let(Ok(old),Ok(new))=(std::fs::read(dir.join("source.pdf")),std::fs::read(dir.join("saved-copy.pdf")))else{
   return println!("PROBE no authorised copies present; nothing to check");
  };
  println!("PROBE source={} appended={} verdict={:?}",old.len(),new.len(),incremental_preflight(&old,&new).map_err(|e|e.to_string()));
 }
 /// Read-only probe over the app's own committed outputs: does the preflight
 /// accept what the writer actually produced? Writes nothing.
 #[test]#[ignore]fn probe_preflight_on_recorded_outputs(){
  let root=project().join("tests/work/save-chain-release");
  let mut runs=std::fs::read_dir(&root).map(|d|d.filter_map(|e|e.ok()).map(|e|e.path()).filter(|p|p.is_dir()).collect::<Vec<_>>()).unwrap_or_default();
  runs.sort();
  let(mut pairs,mut accepted)=(0usize,0usize);
  for run in runs.iter().rev().take(3){
   for entry in std::fs::read_dir(run).into_iter().flatten().flatten(){
    let path=entry.path();let name=path.file_name().and_then(|v|v.to_str()).unwrap_or("").to_owned();
    if !name.ends_with("-output.pdf"){continue;}
    let candidate=run.join(name.replace("-output.pdf","-candidate.pdf"));if !candidate.exists(){continue;}
    let old=std::fs::read(&candidate).unwrap();let new=std::fs::read(&path).unwrap();pairs+=1;
    let ok=incremental_preflight(&old,&new).is_ok();if ok{accepted+=1;}
    println!("PROBE {name} = {}",if ok{"accept"}else{"REJECT"});
   }
  }
  println!("PROBE pairs={pairs} accepted={accepted}");
  assert!(pairs>0,"no recorded candidate/output pairs were found");
 }

 // ---- Real third-party structures ----------------------------------------
 // These construct the bytes here, so the test needs no external fixture and
 // never reads a user file. They exist because the preflight previously demanded
 // an `%PDF-1.5` header: ordinary `%PDF-1.3`/`%PDF-1.4` documents were refused as
 // "invalid structure" although their revisions walk perfectly well.

/// A trailer in the shape a real third-party CRLF document has: no space before
/// `>>`, `>>\r\n` after it, and a `/ID` written in whichever spelling the caller
/// supplies. `/Info` points at the free object `third_party_pdf` leaves for it,
/// which is exactly how such a document looks and is not dereferenced.
fn crlf_trailer(prev:usize,size:usize,id:&[u8])->Vec<u8>{
 let mut out=Vec::new();
 out.extend_from_slice(b"\r\n<</Size ");
 out.extend_from_slice(size.to_string().as_bytes());
 out.extend_from_slice(b"/Root 1 0 R/Info 5 0 R");
 out.extend_from_slice(id);
 out.extend_from_slice(b" /Prev ");
 out.extend_from_slice(prev.to_string().as_bytes());
 out.extend_from_slice(b">>\r\n");
 out
}
/// The same document with `\n` instead of `\r\n`, which is what the earlier
/// synthetic tests used and is no longer the only shape that works.
fn lf_trailer(prev:usize,size:usize,id:&[u8])->Vec<u8>{
 crlf_trailer(prev,size,id).into_iter().filter(|b|*b!=b'\r').collect()
}
/// A revision appended the way the renderer writes one: a classic xref table
/// whose entry offsets are ten digits, `\r\n` inside the table and `\n` before
/// `trailer`, and a trailer closed by `>>` immediately before the line ending.
/// The table lists only objects the revision really contains, exactly as the
/// renderer and the writer do - both leave no free entries behind.
fn renderer_revision(old:&[u8],object_id:u32,body:&str,trailer:&[u8])->Vec<u8>{
 let mut out=old.to_vec();
 let at=out.len();
 out.extend_from_slice(format!("{object_id} 0 obj\n{body}\nendobj\n").as_bytes());
 let xref_at=out.len();
 out.extend_from_slice(format!("xref\r\n{object_id} 1\r\n{at:010} 00000 n\r\n").as_bytes());
 out.extend_from_slice(b"trailer\n");
 out.extend_from_slice(trailer);
 out.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
 out
}
#[test]fn m8_preflight_accepts_a_crlf_trailer_and_a_literal_id_string(){
 let size=350usize;
 // `/Size` is the object count the real document reports; the table itself stays
 // sparse, so only real objects are listed. The synthetic source is built with a
 // matching last free object so the two agree.
 let source=third_party_pdf("%PDF-1.7",5,1,1);
 let prior=last_xref(&source).unwrap();
 let hex_id=b"/ID[<5D2F9C5BC3795F408D178E6C16A68EE0><5D2F9C5BC3795F408D178E6C16A68EE0>]";
 // Both line endings, and both legal spellings of the same identifier bytes.
 for (label,trailer) in [
  ("CRLF hex",crlf_trailer(prior,size,hex_id)),
  ("LF hex",lf_trailer(prior,size,hex_id)),
  ("CRLF literal",crlf_trailer(prior,size,b"/ID[(]/[\x9c[\xc3y_@\x8d\x17\x8el\x16\xa6\x8e\xe0)(m_pX\x80\x1e]\xa6\xc2\xbb\x1b\xa8E\xc2&)]")),
  ("LF literal",lf_trailer(prior,size,b"/ID[(]/[\x9c[\xc3y_@\x8d\x17\x8el\x16\xa6\x8e\xe0)(m_pX\x80\x1e]\xa6\xc2\xbb\x1b\xa8E\xc2&)]")),
 ]{
  let revision=renderer_revision(&source,349,"<< /Type /Font >>",&trailer);
  assert!(incremental_preflight(&source,&revision).is_ok(),"{label} was refused: {:?}",incremental_preflight(&source,&revision).err().map(|e|e.to_string()));
 }
 // The shape is still enforced: three entries, a bare number, two numbers, or a
 // string that never closes must all still refuse.
 for bad in ["/ID[<AA><BB><CC>]","/ID 5","/ID[5 6]","/ID[(unclosed"]{
  let revision=renderer_revision(&source,349,"<< /Type /Font >>",&crlf_trailer(prior,size,bad.as_bytes()));
  assert!(incremental_preflight(&source,&revision).is_err(),"{bad} was accepted as an /ID");
 }
}

 fn page_object(id:u32,content:u32)->Vec<u8>{
  format!("{id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {content} 0 R >>\nendobj\n").into_bytes()
 }
 fn content_object(id:u32)->Vec<u8>{
  let body="BT /F1 12 Tf 72 700 Td (synthetic) Tj ET";
  format!("{id} 0 obj\n<< /Length {} >>\nstream\n{body}\nendstream\nendobj\n",body.len()).into_bytes()
 }
 /// A classic-xref-table PDF with /Info and /ID, shaped like a document produced
 /// by another tool rather than by our own writer.
 pub(crate) fn third_party_pdf(header:&str,size:u32,root:u32,info:u32)->Vec<u8>{
  let mut out=format!("{header}\n").into_bytes();
  let objects:Vec<Vec<u8>>=vec![
   b"<< /Type /Catalog /Pages 2 0 R >>\n".to_vec(),
   b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>\n".to_vec(),
   b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>\n".to_vec(),
   b"<< /Length 44 >>\nstream\nBT /F1 12 Tf 72 700 Td (third party) Tj ET\nendstream\n".to_vec(),
   b"<< /Producer (synthetic) /Title (structure) >>\n".to_vec(),
  ];
  let mut offsets=Vec::new();
  for (index,body) in objects.iter().enumerate(){
   offsets.push(out.len());
   out.extend_from_slice(format!("{} 0 obj\n",index+1).as_bytes());
   out.extend_from_slice(body);
   out.extend_from_slice(b"\nendobj\n");
  }
  let xref_at=out.len();
  out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
  out.extend_from_slice(b"0000000000 65535 f \n");
  for n in 1..size{
   match offsets.get((n as usize).saturating_sub(1)){Some(offset)if n<=objects.len() as u32=>out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()),_=>out.extend_from_slice(b"0000000000 65535 f \n")}
  }
  out.extend_from_slice(format!("trailer\n<< /Size {size} /Root {root} 0 R /Info {info} 0 R /ID [<0123456789ABCDEF0123456789ABCDEF> <FEDCBA9876543210FEDCBA9876543210>] >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes());
  out
 }
 /// One more revision in the shape our writer appends: new objects, a classic
 /// xref table whose entries point only into the appended region, and a trailer
 /// that chains back by /Prev with the same /Root and /ID.
 pub(crate) fn appended_revision(old:&[u8],size:u32,root:u32)->Vec<u8>{
  let prior=last_xref(old).unwrap();
  let mut out=old.to_vec();
  let page_at=out.len();
  out.extend_from_slice(&page_object(size,size+1));
  let content_at=out.len();
  out.extend_from_slice(&content_object(size+1));
  let xref_at=out.len();
  out.extend_from_slice(format!("xref\n{size} 2\n{page_at:010} 00000 n \n{content_at:010} 00000 n \n").as_bytes());
  out.extend_from_slice(format!("trailer\n<< /Prev {prior} /Size {} /Root {root} 0 R /ID [<0123456789ABCDEF0123456789ABCDEF> <FEDCBA9876543210FEDCBA9876543210>] >>\nstartxref\n{xref_at}\n%%EOF\n",size+2).as_bytes());
  out
 }

 // Every version header a real document may carry, including the ones the old // gate refused. Each is a real classic xref table, so the structure itself is
 // verifiable and must be accepted.
 #[test]fn m8_preflight_accepts_every_real_version_header_with_a_classic_xref(){
  for header in ["%PDF-1.0","%PDF-1.1","%PDF-1.2","%PDF-1.3","%PDF-1.4","%PDF-1.5","%PDF-1.6","%PDF-1.7","%PDF-1.8","%PDF-1.9","%PDF-2.0"]{
   let old=third_party_pdf(header,6,1,5);
   let new=appended_revision(&old,6,1);
   assert!(incremental_preflight(&old,&new).is_ok(),"{header} was refused although its revision is verifiable");
   // The same source with no new revision is not a save and stays refused.
   assert!(incremental_preflight(&old,&old).is_err(),"{header} accepted an unchanged file");
  }
 }

 // The fix must not have loosened the structural guarantees. Each case below is
 // a revision that a real reader would treat as suspicious and that manual
 // approval would otherwise have to catch.
 #[test]fn m8_preflight_still_refuses_unverifiable_revisions(){
  let old=third_party_pdf("%PDF-1.4",6,1,5);
  let good=appended_revision(&old,6,1);
  assert!(incremental_preflight(&old,&good).is_ok());

  // 1. The original bytes are not a prefix of the new file.
  let mut replaced=good.clone();replaced[40]^=1;
  assert!(incremental_preflight(&old,&replaced).is_err(),"a rewritten prefix was accepted");
  // 2. No appended revision at all.
  assert!(incremental_preflight(&old,&old).is_err());
  // 3. A different Catalog: the revision must not silently repoint /Root.
  let mut other=old.clone();
  other.extend_from_slice(&page_object(6,7));
  other.extend_from_slice(&content_object(7));
  *other.last_mut().unwrap()=b'x';
  let mut foreign=appended_revision(&old,6,2);
  foreign.truncate(good.len());
  assert!(incremental_preflight(&old,&foreign).is_err(),"a revision with a different /Root was accepted");
  // 4. A non-PDF header is still refused.
  let mut wrong_header=good.clone();wrong_header[..8].copy_from_slice(b"%PDX-1.4");
  assert!(incremental_preflight(&old,&wrong_header).is_err(),"a non-PDF header was accepted");
  // 5. A truncated revision (startxref no longer terminal) is refused.
  assert!(incremental_preflight(&old,&good[..good.len()-6]).is_err(),"a truncated revision was accepted");
 }
}
