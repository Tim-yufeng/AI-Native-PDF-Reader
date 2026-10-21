//! Disposable loopback fixture for the production request/SSE path. This module
//! is absent from normal builds; it never reads credentials or arbitrary URLs.
use crate::{ai_config::Model, ai_network::failure, error::Result};
use std::{io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};

pub(crate) fn selected(model: &Model) -> bool {
    model.provider == "mock" && model.endpoint.is_empty()
        && matches!(model.model.as_str(), "mock-http/main" | "mock-http/auxiliary")
}

pub(crate) fn connection(model: &Model) -> Result<(reqwest::Url, reqwest::Client, crate::credentials::Secret)> {
    if !selected(model) || crate::ai_policy::REAL_ENABLED { return Err(failure("mock_only")); }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|_|failure("network"))?;
    let port = listener.local_addr().map_err(|_|failure("network"))?.port();
    listener.set_nonblocking(true).map_err(|_|failure("network"))?;
    let expected_model = model.model.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, peer)) if peer.ip().is_loopback() => break stream,
                Ok(_) => return,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                Err(_) => return,
            }
        };
        // Windows inherits the listener's nonblocking mode on accept. The
        // request headers/body can arrive later; reads below use timeouts.
        if stream.set_nonblocking(false).is_err(){return;}
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let run = || -> std::io::Result<()> {
            let mut raw=Vec::new();let mut chunk=[0u8;8192];
            let (header_end, length) = loop {
                let n=stream.read(&mut chunk)?;if n==0||raw.len()+n>2*1024*1024{return Ok(());}raw.extend_from_slice(&chunk[..n]);
                if let Some(end)=raw.windows(4).position(|v|v==b"\r\n\r\n") {
                    let header=String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                    if !header.starts_with("post /v1/chat/completions http/1.1\r\n") || !header.lines().any(|line|line=="authorization: bearer isolated-fake-key") {return Ok(());}
                    let Some(length)=header.lines().find_map(|line|line.strip_prefix("content-length: ").and_then(|v|v.parse::<usize>().ok())) else{return Ok(());};
                    if length>2*1024*1024{return Ok(());}break (end+4,length);
                }
            };
            while raw.len()<header_end+length{let n=stream.read(&mut chunk)?;if n==0{return Ok(());}raw.extend_from_slice(&chunk[..n]);if raw.len()>2*1024*1024+8192{return Ok(());}}
            let Ok(body)=serde_json::from_slice::<serde_json::Value>(&raw[header_end..header_end+length]) else{return Ok(());};
            if body["model"]!=expected_model||body["stream"]!=true{return Ok(());}
            let Some(messages)=body["messages"].as_array() else{return Ok(());};
            let Some(system)=messages.first().filter(|m|m["role"]=="system").and_then(|m|m["content"].as_str()) else{return Ok(());};
            if !system.contains("保持数学严谨")||!system.contains("禁止虚构页码"){return Ok(());}
            let synthetic_style=system.contains("CLASSROOM_SYNTHETIC_STYLE");
            let Some(content)=messages.last().and_then(|m|m["content"].as_array()) else{return Ok(());};
            let images=content.iter().filter(|item|item["type"]=="image_url").count();
            if content.iter().filter(|item|item["type"]=="image_url").any(|item|!item["image_url"]["url"].as_str().is_some_and(|v|v.starts_with("data:image/png;base64,"))){return Ok(());}
            let slot=expected_model.strip_prefix("mock-http/").unwrap();
            if synthetic_style&&(!system.contains("必须体现在回答里")||!system.contains("永远不能改变事实")){return Ok(());}
            let text=format!("MOCK_HTTP {slot} images={images} history={} style={} 合成完整回答 [S1]",messages.len().saturating_sub(2),if synthetic_style{"selected"}else{"none"});
            let sse=format!("data: {}\r\n\r\ndata: {}\n\ndata: [DONE]\n\n",serde_json::json!({"choices":[{"delta":{"content":text}}]}),serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20}}));
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",sse.len()).as_bytes())?;
            // Small transport chunks exercise split frames and UTF-8 boundaries.
            for part in sse.as_bytes().chunks(7){stream.write_all(part)?;stream.flush()?;std::thread::sleep(Duration::from_millis(1));}Ok(())
        };
        let mut run=run;let _=run();
    });
    let url=reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1/chat/completions")).map_err(|_|failure("network"))?;
    let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10)).build().map_err(|_|failure("network"))?;
    Ok((url,client,crate::credentials::Secret(b"isolated-fake-key".to_vec())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn m5_mock_http_waits_for_split_request_body() {
        let model=Model{provider:"mock".into(),endpoint:String::new(),model:"mock-http/main".into(),vision:false};
        let (url,_client,_fake)=connection(&model).unwrap();
        let mut stream=std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST,url.port().unwrap())).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let body=serde_json::json!({"model":model.model,"stream":true,"messages":[
            {"role":"system","content":"保持数学严谨；禁止虚构页码。"},
            {"role":"user","content":[{"type":"text","text":"Synthetic delayed body"}]}
        ]}).to_string();
        stream.write_all(format!("POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer isolated-fake-key\r\nContent-Length: {}\r\n\r\n",body.len()).as_bytes()).unwrap();
        // Headers and body need not arrive in the same socket read. On Windows
        // an accepted socket inherits the listener's nonblocking mode.
        std::thread::sleep(Duration::from_millis(100));
        let _=stream.write_all(body.as_bytes());
        let mut response=String::new();let _=stream.read_to_string(&mut response);
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"),"fixture closed before the request body arrived");
        assert!(response.contains("MOCK_HTTP main")&&response.ends_with("data: [DONE]\n\n"));
    }
}
