//! Focused success-cache correctness and CPU cost probes. No native save or UI.
use crate::pdf_export::{Service,verify_writer_assets};
use std::{fs,sync::{Arc,atomic::{AtomicUsize,Ordering}},time::Instant};
use serde::Deserialize;
fn new_service()->Service{Service::new(crate::unicode_worker::prototype())}
fn assets()->impl Fn(&str)->Option<Vec<u8>>{
    let context:tauri::Context<tauri::Wry>=tauri::generate_context!();
    move|path|context.assets().get(&tauri::utils::assets::AssetKey::from(path)).map(|b|b.to_vec()).or_else(||{
        if tauri::is_dev(){fs::read(crate::save_core::project().join("dist").join(path)).ok()}else{None}
    })
}
#[test]fn save_performance_asset_cache_success_failure_and_mutable_dev(){
    let service=new_service();let fetch=assets();let calls=AtomicUsize::new(0);
    assert!(service.check_assets(true,&|_|None).is_err());
    service.check_assets(true,&|p|{calls.fetch_add(1,Ordering::Relaxed);fetch(p)}).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed),2);
    service.check_assets(true,&|_|panic!("a verified immutable set must not be fetched again")).unwrap();
    // Even after an immutable success, mutable/development callers must recheck.
    assert!(service.check_assets(false,&|_|None).is_err());
    assert!(new_service().check_assets(true,&|_|None).is_err());
}
#[test]fn save_performance_asset_cache_first_call_is_single_flight(){
    let service=Arc::new(new_service());let calls=Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope|{
        for _ in 0..4{let service=service.clone();let calls=calls.clone();scope.spawn(move||{
            let fetch=assets();service.check_assets(true,&|p|{calls.fetch_add(1,Ordering::Relaxed);fetch(p)}).unwrap();
        });}
    });
    assert_eq!(calls.load(Ordering::Relaxed),2);
}
#[test]#[ignore="bounded local CPU measurement, run explicitly with --release"]
fn save_performance_cpu_probe(){
    assert!(!cfg!(debug_assertions),"measure optimized code only");
    let out=crate::save_core::project().join("tests/work/save-interop-performance/expert-01eabfb");
    let wire=fs::read(out.join("wire.json")).unwrap();let raw=fs::read(out.join("wire-output.bin")).unwrap();
    let fetch=assets();let service=new_service();let mut rows=vec![];
    for repeat in 0..3{
        let start=Instant::now();verify_writer_assets(&fetch).unwrap();let direct_ms=start.elapsed().as_secs_f64()*1000.0;
        let start=Instant::now();service.check_assets(true,&fetch).unwrap();let cached_ms=start.elapsed().as_secs_f64()*1000.0;
        let start=Instant::now();let value:serde_json::Value=serde_json::from_slice(&wire).unwrap();let parse_ms=start.elapsed().as_secs_f64()*1000.0;
        let start=Instant::now();let candidate=Vec::<u8>::deserialize(&value["candidate"]).unwrap();let output=Vec::<u8>::deserialize(&value["output"]).unwrap();let args_ms=start.elapsed().as_secs_f64()*1000.0;
        let start=Instant::now();let raw_owned=std::hint::black_box(raw.clone());let raw_copy_ms=start.elapsed().as_secs_f64()*1000.0;
        assert_eq!(raw_owned,output);assert_eq!(&raw_owned[..candidate.len()],candidate.as_slice());
        rows.push(serde_json::json!({"repeat":repeat,"directAssetMs":direct_ms,"cacheAssetMs":cached_ms,"jsonValueParseMs":parse_ms,"vecDecodeMs":args_ms,"rawCopyMs":raw_copy_ms}));
    }
    let result=serde_json::json!({"scope":"Release CPU only: embedded asset verification, actual serde_json Value/Vec decoding, raw copy; excludes Wry transport and executor queue","rows":rows});
    fs::write(out.join("rust-cpu.json"),serde_json::to_vec_pretty(&result).unwrap()).unwrap();println!("{result}");
}

#[test]#[ignore="run after writer-compression.test.ts generates disposable evidence"]
fn save_performance_compressed_output_keeps_original_admission(){
    let root=crate::save_core::project();let source=fs::read(root.join("tests/fixtures/g1/plain.pdf")).unwrap();
    for name in ["compression-raw.pdf","compression-output.pdf"]{
        let bytes=fs::read(root.join("tests/work/save-interop-performance/expert-01eabfb").join(name)).unwrap();
        let prepared=crate::pdf_export::Prepared{bytes:bytes.clone(),source_hash:crate::save_core::hash(&source),candidate_hash:String::new()};
        assert!(crate::original_commit::ApprovedOutput::checked(&source,prepared).is_ok(),"native admission: {name}");
        let mut changed=bytes;changed[7]^=1;
        let prepared=crate::pdf_export::Prepared{bytes:changed,source_hash:crate::save_core::hash(&source),candidate_hash:String::new()};
        assert!(crate::original_commit::ApprovedOutput::checked(&source,prepared).is_err(),"changed original prefix: {name}");
    }
}

#[test]
#[ignore="run after writer-changes.test.ts generates disposable evidence"]
fn writer_changes_legacy_appearances_keep_native_original_admission(){
    let root=crate::save_core::project().join("tests/work/writer-changes");
    for kind in ["classic","stream"] { for edit in ["highlight","ink","unchanged"] {
        let stem=format!("{kind}-{edit}");
        let source=fs::read(root.join(format!("{stem}-old.pdf"))).unwrap();
        let candidate=fs::read(root.join(format!("{stem}-candidate.pdf"))).unwrap();
        let output=fs::read(root.join(format!("{stem}-output.pdf"))).unwrap();
        assert!(candidate.starts_with(&source)&&output.starts_with(&candidate));
        let prepared=crate::pdf_export::Prepared{bytes:output.clone(),source_hash:crate::save_core::hash(&source),candidate_hash:crate::save_core::hash(&candidate)};
        assert!(crate::original_commit::ApprovedOutput::checked(&source,prepared).is_ok(),"native admission: {stem}");
        let mut wrong=output;wrong[7]^=1;
        let prepared=crate::pdf_export::Prepared{bytes:wrong,source_hash:crate::save_core::hash(&source),candidate_hash:crate::save_core::hash(&candidate)};
        assert!(crate::original_commit::ApprovedOutput::checked(&source,prepared).is_err(),"altered prefix: {stem}");
    }}
}
