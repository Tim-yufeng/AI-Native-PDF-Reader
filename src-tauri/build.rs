fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        // Only these commands can ever be granted to the reader window; a command
        // the reader invokes but does not declare here has no permission at all,
        // so its invocation is rejected before any code runs. The controlled
        // original save (Ctrl+S) was exactly that case: implemented, reachable
        // from the UI, and impossible to allow. scripts/check-package.mjs now
        // fails the build when an invoked command is missing from this list.
        tauri_build::AppManifest::new().commands(&["pick_workspace","list_workspace","open_workspace_pdf","close_workspace","check_unicode_writer","begin_unicode_export","finish_unicode_export","save_original_pdf","pending_pdf_recovery","export_pdf_recovery","cancel_unicode_export","m7_open_pending","m6_preferences","m6_edit_style","m6_import_styles","m6_select_style","m6_delete_style_version","m6_set_inputs","m6_set_context","m6_mock_explain","m5_directory","m5_index_member","m5_source","m5_config","m5_chat_view","m5_chat_select","m5_chat_archive","m5_chat_delete","m5_set_config","m5_store_key","m5_delete_key","m5_test_connection","m5_index","m5_prepare","m5_images","m5_refine","m5_start","m5_cancel","m5_details","pick_pdfs", "begin_pdf_read", "read_pdf", "cancel_pdf_read", "release_pdf", "get_settings", "set_reader_settings", "save_m3_copy", "m4_status", "m4_bind_document", "m4_messages", "m4_add_message", "m4_edit_prompt", "m4_generate_synthetic", "m4_delete_message", "m4_delete_document_history", "m4_position", "m4_release_binding", "m4_put_draft", "m4_list_drafts", "m4_drop_draft", "m4_backup", "m4_cache_clear", "m4_pick_migrate_data", "m4_get_settings", "m4_set_settings", "m4_list_backups", "m4_restore_backup"]),
    )).expect("Unable to build the restricted desktop shell");
}
