//! One bounded Rust feasibility experiment, NOT a PDF parser or Writer.
//! Default-instance cmap12/metrics and CID text operators only. No new crate,
//! font subsetting, embedding, arbitrary PDF editing or production IPC.
use std::{collections::BTreeMap, fs, io};
use crate::save_core::{project, hash};
fn bad() -> io::Error { io::Error::other("unsupported or malformed bounded font") }
fn u16be(b: &[u8], n: usize) -> io::Result<u16> {
    Ok(u16::from_be_bytes(b.get(n..n+2).ok_or_else(bad)?.try_into().map_err(|_|bad())?))
}
fn u32be(b: &[u8], n: usize) -> io::Result<u32> {
    Ok(u32::from_be_bytes(b.get(n..n+4).ok_or_else(bad)?.try_into().map_err(|_|bad())?))
}
fn table<'a>(b: &'a [u8], tag: &[u8; 4]) -> io::Result<&'a [u8]> {
    if b.len() > 20_000_000 || u32be(b, 0)? != 0x00010000 { return Err(bad()); }
    let count = u16be(b, 4)? as usize;
    if count > 128 { return Err(bad()); }
    for i in 0..count {
        let n = 12 + i*16;
        if b.get(n..n+4) == Some(tag.as_slice()) {
            let offset = u32be(b,n+8)? as usize; let length = u32be(b,n+12)? as usize;
            return b.get(offset..offset.checked_add(length).ok_or_else(bad)?).ok_or_else(bad);
        }
    }
    Err(bad())
}
fn glyph(b: &[u8], cp: u32) -> io::Result<u16> {
    let cmap = table(b, b"cmap")?;
    let count = u16be(cmap, 2)? as usize; if count > 64 { return Err(bad()); }
    for i in 0..count {
        let n = 4+i*8; let platform = u16be(cmap,n)?; let encoding = u16be(cmap,n+2)?;
        if !(platform == 0 || (platform == 3 && encoding == 10)) { continue; }
        let offset = u32be(cmap,n+4)? as usize;
        let sub = cmap.get(offset..).ok_or_else(bad)?;
        if u16be(sub,0)? != 12 { continue; }
        let length = u32be(sub,4)? as usize;
        let sub = sub.get(..length).ok_or_else(bad)?;
        let groups = u32be(sub,12)? as usize;
        if groups > 100_000 || 16+groups*12 != sub.len() { return Err(bad()); }
        let mut previous = None; let mut found = 0u32;
        for j in 0..groups {
            let n = 16+j*12; let start=u32be(sub,n)?; let end=u32be(sub,n+4)?;
            if start>end || previous.is_some_and(|last| start<=last) { return Err(bad()); }
            previous=Some(end);
            if (start..=end).contains(&cp) { found=u32be(sub,n+8)?.checked_add(cp-start).ok_or_else(bad)?; }
        }
        if found == 0 || found >= u16be(table(b,b"maxp")?,4)? as u32 { return Err(bad()); }
        return u16::try_from(found).map_err(|_|bad());
    }
    Err(bad())
}
fn width(b: &[u8], gid: u16) -> io::Result<f64> {
    let count=u16be(table(b,b"hhea")?,34)?;
    let units=u16be(table(b,b"head")?,18)?;
    if count==0 || units==0 { return Err(bad()); }
    Ok(u16be(table(b,b"hmtx")?,usize::from(gid.min(count-1))*4)? as f64 * 1000.0 / units as f64)
}
#[test]
fn pinned_default_font_metrics_and_cid_operators_match_independent_oracle() {
    let manifest:serde_json::Value=serde_json::from_slice(&fs::read(project().join("spikes/g1/unicode-ap-font.json")).unwrap()).unwrap();
    let font=fs::read(project().join(manifest["cache"].as_str().unwrap()).join(manifest["name"].as_str().unwrap())).unwrap();
    assert_eq!(hash(&font),manifest["sha256"].as_str().unwrap());
    let work=project().join("tests/work/security-close/round2");
    let oracle:serde_json::Value=serde_json::from_slice(&fs::read(work.join("font-oracle.json")).unwrap()).unwrap();
    let text=oracle["text"].as_str().unwrap(); let mut metrics=BTreeMap::new();
    let mut encoded=String::new();
    for ch in text.chars() {
        let gid=glyph(&font,ch as u32).unwrap(); let advance=width(&font,gid).unwrap();
        let expected=&oracle["metrics"][ch.to_string()];
        assert_eq!(gid as u64,expected["glyph"].as_u64().unwrap());
        assert!((advance-expected["width1000"].as_f64().unwrap()).abs()<0.00001);
        metrics.insert(ch.to_string(),serde_json::json!({"gid":gid,"width1000":advance}));
        encoded.push_str(&format!("{gid:04X}"));
    }
    assert!(glyph(&font,0x10ffff).is_err());
    assert!(glyph(&font[..64],0x4e2d).is_err());
    let mut appearances=Vec::new();
    for size in [12,18,24] {
        for color in [[0.0,0.0,0.0],[0.15,0.3,0.6]] {
            let operators=format!("q BT /FU {size} Tf {} {} {} rg 1 0 0 1 4 32 Tm <{encoded}> Tj ET Q",color[0],color[1],color[2]);
            appearances.push(operators);
        }
    }
    fs::write(work.join("rust-font-feasibility.json"),serde_json::json!({
        "metrics":metrics,"appearanceOperators":appearances,"fontBytes":font.len(),
        "hasVariableTables":table(&font,b"fvar").is_ok(),"newDependencies":0,
        "fontSubsetOrPdfOutputImplemented":false,"productionWriter":false,
        "remaining":"font subsetting/variation instance, CID font embedding/ToUnicode, existing-PDF parsing/incremental object updates/resource lifecycle"
    }).to_string()).unwrap();
}
