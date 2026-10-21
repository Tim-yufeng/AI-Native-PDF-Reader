//! Rebuildable in-memory SQLite FTS5. No page text or screenshots on disk.
use crate::{error::Result,ai_network::failure};use rusqlite::{Connection,params};use serde::{Serialize,Deserialize};
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct PageText{pub page:u32,pub text:String}
#[derive(Clone,Serialize,Deserialize)]#[serde(rename_all="camelCase",deny_unknown_fields)]
pub(crate) struct Source{pub id:String,pub grant_id:String,pub name:String,pub page:u32,pub text:String,pub rect:Option<[f64;4]>,pub image:bool,#[serde(default)]pub reasons:Vec<String>,
 #[serde(default)]pub document_id:Option<String>,#[serde(default)]pub file_key:Option<String>,#[serde(default)]pub hash:Option<String>}
pub(crate) struct Index{db:Connection,bytes:usize}
fn cjk(c:char)->bool{('\u{3400}'..='\u{9fff}').contains(&c)}
pub(crate) fn terms(text:&str)->Vec<String>{
 let normalized=text.to_lowercase().replace('≤'," le ").replace('≥'," ge ").replace('∫'," integral ").replace('∑'," sum ");
 let chars=normalized.chars().collect::<Vec<_>>();let mut out=normalized.split(|c:char|!c.is_alphanumeric()).filter(|v|!v.is_empty()).map(str::to_owned).collect::<Vec<_>>();
 for pair in chars.windows(2){if pair.iter().all(|c|cjk(*c)){out.push(pair.iter().collect());}}
 out.sort();out.dedup();out
}
impl Index{
 pub(crate) fn new()->Self{let db=Connection::open_in_memory().expect("memory SQLite");db.execute_batch("CREATE VIRTUAL TABLE pages USING fts5(grant_id UNINDEXED,page UNINDEXED,text UNINDEXED,terms);").expect("bundled FTS5");Self{db,bytes:0}}
 pub(crate) fn put(&mut self,grant:&str,pages:&[PageText],count:u32)->Result<()>{
  if pages.len()>32||pages.iter().any(|p|p.page==0||p.page>count||p.text.len()>32768){return Err(failure("budget"));}
  let unique=pages.iter().map(|p|p.page).collect::<std::collections::HashSet<_>>();if unique.len()!=pages.len(){return Err(failure("invalid_request"));}
  let mut old=0usize;for p in pages{old+=self.db.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM pages WHERE grant_id=?1 AND page=?2",params![grant,p.page],|r|r.get::<_,i64>(0)).map_err(|_|failure("index"))? as usize;}
  let new=pages.iter().map(|p|p.text.len()).sum::<usize>();if self.bytes.saturating_sub(old)+new>32*1024*1024{return Err(failure("budget"));}
  let tx=self.db.transaction().map_err(|_|failure("index"))?;
  for p in pages{tx.execute("DELETE FROM pages WHERE grant_id=?1 AND page=?2",params![grant,p.page]).map_err(|_|failure("index"))?;
   tx.execute("INSERT INTO pages VALUES(?1,?2,?3,?4)",params![grant,p.page,p.text,terms(&p.text).join(" ")]).map_err(|_|failure("index"))?;}
  tx.commit().map_err(|_|failure("index"))?;self.bytes=self.db.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM pages",[],|r|r.get::<_,i64>(0)).map_err(|_|failure("index"))? as usize;Ok(())
 }
 pub(crate) fn remove(&mut self,grant:&str){let _=self.db.execute("DELETE FROM pages WHERE grant_id=?1",[grant]);self.bytes=self.db.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM pages",[],|r|r.get::<_,i64>(0)).unwrap_or(self.bytes as i64) as usize;}
 pub(crate) fn page(&self,grant:&str,page:u32)->Result<String>{self.db.query_row("SELECT text FROM pages WHERE grant_id=?1 AND page=?2",params![grant,page],|r|r.get(0)).map_err(|_|failure("index_incomplete"))}
 pub(crate) fn coverage(&self,grant:&str)->Result<(u32,u32)>{self.db.query_row("SELECT count(DISTINCT page),coalesce(sum(CASE WHEN trim(text,char(9)||char(10)||char(13)||' ')='' THEN 1 ELSE 0 END),0) FROM pages WHERE grant_id=?1",[grant],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|failure("index"))}
 pub(crate) fn search(&self,grants:&[String],query:&str)->Result<Vec<(String,u32)>>{
  let words=terms(query);if words.len()>64{return Err(failure("budget"));}if words.is_empty(){return Ok(vec![]);}
  let match_query=words.iter().map(|w|format!("\"{}\"",w.replace('"',"\"\""))).collect::<Vec<_>>().join(" OR ");
  if grants.is_empty(){return Ok(vec![]);}if grants.len()>17{return Err(failure("budget"));}
  let sql=format!("SELECT grant_id,page FROM pages WHERE pages MATCH ?1 AND grant_id IN ({}) ORDER BY bm25(pages),grant_id,page LIMIT 2001",vec!["?";grants.len()].join(","));
  let mut stmt=self.db.prepare(&sql).map_err(|_|failure("index"))?;let args=std::iter::once(&match_query).chain(grants.iter());
  let rows=stmt.query_map(rusqlite::params_from_iter(args),|r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?))).map_err(|_|failure("index"))?;
  let mut output=Vec::new();for row in rows{let row=row.map_err(|_|failure("index"))?;if grants.contains(&row.0){output.push(row);if output.len()>2000{return Err(failure("budget"));}}}Ok(output)
 }
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn m5_chinese_short_words_math_cross_page_and_revocation(){let mut i=Index::new();i.put("doc",&[PageText{page:22,text:"定义线性空间与基底 α ≤ β".into()},PageText{page:27,text:"在当前图表应用线性变换".into()},PageText{page:1,text:"欢迎".into()}],30).unwrap();
  let result=i.search(&["doc".into()],"基底和线性变换的联系 α ≤ β").unwrap();assert!(result.contains(&("doc".into(),22)));assert!(result.contains(&("doc".into(),27)));assert!(i.search(&["other".into()],"基底").unwrap().is_empty());i.remove("doc");assert!(i.search(&["doc".into()],"基底").unwrap().is_empty());assert!(i.page("doc",22).is_err());}
}
