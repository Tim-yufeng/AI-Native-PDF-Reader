//! Validate the PNG subset emitted by browser canvases, with bounded inflation.
use crate::{error::Result,ai_network::failure};use std::io::Read;
pub(crate) fn validate(data:&[u8])->Result<()>{
 if data.len()<57||data.len()>2*1024*1024||&data[..8]!=b"\x89PNG\r\n\x1a\n"{return Err(failure("invalid_image"));}
 let(mut at,mut width,mut height,mut channels)=(8,0usize,0usize,0usize);let mut compressed=Vec::new();let mut ended=false;
 while at+12<=data.len(){let size=u32::from_be_bytes(data[at..at+4].try_into().unwrap()) as usize;
  let end=at.checked_add(size+12).filter(|v|*v<=data.len()).ok_or_else(||failure("invalid_image"))?;let kind=&data[at+4..at+8];let body=&data[at+8..end-4];
  if crc32fast::hash(&data[at+4..end-4])!=u32::from_be_bytes(data[end-4..end].try_into().unwrap()){return Err(failure("invalid_image"));}
  if at==8&&kind!=b"IHDR"{return Err(failure("invalid_image"));}
  match kind{b"IHDR"=>{if at!=8||size!=13||body[8]!=8||body[10..13]!=[0,0,0]{return Err(failure("invalid_image"));}channels=match body[9]{2=>3,6=>4,_=>return Err(failure("invalid_image"))};width=u32::from_be_bytes(body[..4].try_into().unwrap())as usize;height=u32::from_be_bytes(body[4..8].try_into().unwrap())as usize;
   if width==0||height==0||width>1600||height>1600||width*height>2560000{return Err(failure("budget"));}},
   b"IDAT"=>compressed.extend_from_slice(body),b"IEND"=>{if size!=0||end!=data.len(){return Err(failure("invalid_image"));}ended=true;break;},_=>{if kind[0].is_ascii_uppercase()&&kind!=b"PLTE"{return Err(failure("invalid_image"));}}
  }at=end;
 }
 if !ended||compressed.is_empty(){return Err(failure("invalid_image"));}let row=width*channels+1;let expected=row*height;let mut decoded=Vec::new();
 flate2::read::ZlibDecoder::new(&compressed[..]).take(expected as u64+1).read_to_end(&mut decoded).map_err(|_|failure("invalid_image"))?;
 if decoded.len()!=expected||decoded.chunks(row).any(|line|line[0]>4){return Err(failure("invalid_image"));}Ok(())
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn m5_png_rejects_fake_truncated_bad_crc_and_valid_canvas_subset(){let mut png=b"\x89PNG\r\n\x1a\n".to_vec();let mut chunk=|kind:&[u8],data:&[u8]|{png.extend_from_slice(&(data.len()as u32).to_be_bytes());let mut sum=kind.to_vec();sum.extend_from_slice(data);png.extend_from_slice(&sum);png.extend_from_slice(&crc32fast::hash(&sum).to_be_bytes());};
  chunk(b"IHDR",&[0,0,0,1,0,0,0,1,8,6,0,0,0]);let mut z=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());std::io::Write::write_all(&mut z,&[0,255,255,255,255]).unwrap();chunk(b"IDAT",&z.finish().unwrap());chunk(b"IEND",&[]);assert!(validate(&png).is_ok());assert!(validate(&png[..png.len()-12]).is_err());png[20]^=1;assert!(validate(&png).is_err());
 }
}
#[cfg(test)]pub(crate) fn fixture()->Vec<u8>{
 let mut png=b"\x89PNG\r\n\x1a\n".to_vec();let mut chunk=|kind:&[u8],data:&[u8]|{png.extend_from_slice(&(data.len()as u32).to_be_bytes());let mut sum=kind.to_vec();sum.extend_from_slice(data);png.extend_from_slice(&sum);png.extend_from_slice(&crc32fast::hash(&sum).to_be_bytes());};
 chunk(b"IHDR",&[0,0,0,1,0,0,0,1,8,6,0,0,0]);let mut z=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());std::io::Write::write_all(&mut z,&[0,255,255,255,255]).unwrap();chunk(b"IDAT",&z.finish().unwrap());chunk(b"IEND",&[]);png
}
