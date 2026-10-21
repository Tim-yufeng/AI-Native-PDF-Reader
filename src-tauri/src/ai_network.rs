//! HTTPS-only transport. Validate every resolved address and pin it for this client.
use crate::error::{AppError,Result};
use std::{net::{IpAddr,SocketAddr},time::Duration};
pub(crate) fn failure(code:&'static str)->AppError{AppError{code,message:match code{
 "mock_only"=>"本阶段仅允许 Mock；真实凭据与 Provider 网络已禁用。","authentication"=>"API 鉴权失败，请检查安全凭据。","quota"=>"余额或请求配额不足。","timeout"=>"请求超时，可重试。",
 "vision"=>"所选模型尚未确认支持图像，请选择视觉模型或移除图像。","budget"=>"上下文超过本次预算，请减少页数或区域。",
  // A refusal caused by what the user explicitly chose. Retrieval can never
  // produce this: it is trimmed to fit instead of failing the request.
  "budget_selection"=>"本轮你指定选择的来源本身就超过了容量（页数或文字量过多）；请减少所选页数，或改用区域截图提问。自动检索的相关内容不会挤掉它们。",
 "endpoint"=>"Endpoint 或解析目标不符合公网 HTTPS 安全限制。","protocol"=>"Provider 返回了不完整或不支持的响应。",
 "cancelled"=>"生成已取消，已提交记录保留。",_=>"AI 请求未完成；本地阅读和已有历史不受影响。"}}}
pub(crate) fn endpoint(value:&str)->Result<reqwest::Url>{
 let url=reqwest::Url::parse(value).map_err(|_|failure("endpoint"))?;
 if url.scheme()!="https"||url.host_str().is_none()||!url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some()||url.port_or_known_default()!=Some(443)
 ||url.path().contains('%')||!matches!(url.path(),"/chat/completions"|"/v1/chat/completions"|"/api/v1/chat/completions"){return Err(failure("endpoint"));}
 if let Ok(ip)=url.host_str().unwrap().parse(){if !public(ip){return Err(failure("endpoint"));}}Ok(url)
}
pub(crate) fn public(ip:IpAddr)->bool{match ip{
 IpAddr::V4(v)=>{let a=v.octets();!v.is_private()&&!v.is_loopback()&&!v.is_link_local()&&!v.is_multicast()&&!v.is_broadcast()&&!v.is_documentation()&&a[0]!=0&&a[0]<224&&!(a[0]==100&&(64..=127).contains(&a[1]))&&!(a[0]==198&&(a[1]==18||a[1]==19))&&!(a[0]==192&&a[1]==0)},
 IpAddr::V6(v)=>{let s=v.segments();v.to_ipv4_mapped().map(|ip|public(IpAddr::V4(ip))).unwrap_or((s[0]&0xe000)==0x2000&&!(s[0]==0x2001&&(s[1]==0x0db8||s[1]<0x0200))&&s[0]!=0x2002)}
}}
pub(crate) async fn client(url:&reqwest::Url)->Result<reqwest::Client>{
 crate::ai_policy::require_real()?;
 let host=url.host_str().ok_or_else(||failure("endpoint"))?;
 let addresses=tokio::time::timeout(Duration::from_secs(10),tokio::net::lookup_host((host,443))).await.map_err(|_|failure("timeout"))?.map_err(|_|failure("network"))?.collect::<Vec<SocketAddr>>();
 if addresses.is_empty()||addresses.iter().any(|a|!public(a.ip())){return Err(failure("endpoint"));}
 reqwest::Client::builder().https_only(true).no_proxy().redirect(reqwest::redirect::Policy::none()).resolve_to_addrs(host,&addresses)
 .connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(120)).build().map_err(|_|failure("network"))
}
pub(crate) fn status(code:u16)->AppError{failure(match code{401|403=>"authentication",402|429=>"quota",408|504=>"timeout",400|422=>"invalid_request",_=>"network"})}
// UTF-8 is decoded only after SSE framing; a code point can span network chunks.
#[derive(Default)] pub(crate) struct Sse {buffer:Vec<u8>}
impl Sse {pub(crate) fn feed(&mut self,bytes:&[u8])->Result<Vec<String>>{
 self.buffer.extend_from_slice(bytes);if self.buffer.len()>256*1024{return Err(failure("protocol"));}
 let mut result=Vec::new();
 while let Some(at)=self.buffer.windows(2).position(|v|v==b"\n\n").into_iter().chain(self.buffer.windows(4).position(|v|v==b"\r\n\r\n")).min(){
  let delimiter=if self.buffer[at]==b'\r'{4}else{2};let frame=self.buffer.drain(..at+delimiter).collect::<Vec<_>>();
  let raw=std::str::from_utf8(&frame).map_err(|_|failure("protocol"))?;
  let data=raw.lines().filter_map(|line|line.strip_prefix("data:").map(|v|v.trim_start())).collect::<Vec<_>>().join("\n");if !data.is_empty(){result.push(data);}
 }Ok(result)
}}
#[cfg(test)]mod tests{use super::*;
 #[test]fn m5_targets_and_dns_addresses_are_restricted(){for value in ["http://api.deepseek.com/chat/completions","https://127.0.0.1/chat/completions","https://10.1.1.1/chat/completions","https://user@api.deepseek.com/chat/completions","https://api.deepseek.com:444/chat/completions","https://api.deepseek.com/chat/completions?q=x","https://api.deepseek.com/files"]{assert!(endpoint(value).is_err());}
 assert!(endpoint("https://api.deepseek.com/chat/completions").is_ok());for ip in ["::1","::ffff:127.0.0.1","fe80::1","fc00::1","100.64.0.1","198.19.0.1","192.0.0.5","2001:db8::1"]{assert!(!public(ip.parse().unwrap()));}assert!(public("1.1.1.1".parse().unwrap()));}
 #[test]fn m5_sse_split_utf8_crlf_comments_and_usage(){let mut s=Sse::default();assert!(s.feed(b": ping\r\n\r\ndata: {\"text\":\"\xe4").unwrap().is_empty());assert_eq!(s.feed(b"\xb8\xad\"}\r\n\r\ndata: [DONE]\n\n").unwrap(),vec!["{\"text\":\"中\"}","[DONE]"]);assert!(s.feed(&vec![b'x';300000]).is_err());}
}
