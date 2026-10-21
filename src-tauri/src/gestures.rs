//! Allow Chromium to deliver precision-touchpad pinch. DOM handlers consume it.
pub(crate) fn configure(window:&tauri::WebviewWindow){
    let _=window.with_webview(|view|unsafe{
        use windows::core::Interface;
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2Settings4, ICoreWebView2Settings5};
        if let Ok(core)=view.controller().CoreWebView2(){if let Ok(settings)=core.Settings(){
            // Only this app's WebView: credential inputs must not enter a browser vault.
            if let Ok(settings)=settings.cast::<ICoreWebView2Settings4>(){
                let _=settings.SetIsPasswordAutosaveEnabled(false);
                let _=settings.SetIsGeneralAutofillEnabled(false);
            }
            let _=settings.SetIsZoomControlEnabled(false);
            if let Ok(settings)=settings.cast::<ICoreWebView2Settings5>(){let _=settings.SetIsPinchZoomEnabled(true);}
        }}
    });
}
