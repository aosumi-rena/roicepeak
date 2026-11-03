mod about;
mod actions;
mod anchored_popup;
mod audio;
mod block_commands;
mod block_selection;
mod catalog;
mod clipboard_edit;
mod diagnostics;
mod dialog_controls;
mod dialogs;
mod dictionary;
mod document;
mod edit_history;
mod editor;
mod emotion_presets;
mod experimental;
mod export;
mod file_menu;
mod find_replace;
mod hex;
mod i18n;
mod imports;
mod inspector;
mod inspector_scroll;
mod menus;
mod narrator_order;
mod narrator_picker;
mod narrator_settings;
mod native_adapter;
mod native_cli;
mod native_icons;
mod native_interop;
mod native_menu;
mod native_origin;
mod native_project;
mod native_save;
mod phoneme_history;
mod placement;
mod portrait;
mod preparation_ui;
mod preset_store;
mod project_files;
mod query_basis;
mod recent_projects;
mod render;
mod replace_voices;
mod resource_settings;
mod resources;
mod root_tooltip;
mod script_context;
mod script_interactions;
mod script_view;
mod sentence_edit;
mod sentences;
mod shortcut_dialog;
mod shortcut_editor;
mod shortcuts;
mod style;
mod text_clipboard;
mod timeline;
mod timestamp_popup;
mod toolbar;
mod transport;
mod voice_controls;
mod voice_library;

pub fn run() -> eframe::Result {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if let Some(code) = diagnostics::dispatch(&arguments) {
        std::process::exit(code);
    }
    if let native_cli::Dispatch::Exit(code) = native_cli::dispatch(&arguments) {
        std::process::exit(code);
    }
    let mut gpu = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    #[cfg(windows)]
    {
        gpu.instance_descriptor.backends = wgpu::Backends::DX12;
        gpu.instance_descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
    }
    #[cfg(target_os = "linux")]
    {
        gpu.instance_descriptor.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(application_icon())
            .with_inner_size([1280.0, 768.0])
            .with_min_inner_size([900.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(gpu),
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "ROICEPEAK",
        options,
        Box::new(|cc| {
            native_icons::install(&cc.egui_ctx);
            Ok(Box::new(editor::EditorApp::new(cc)))
        }),
    )
}

fn application_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(include_bytes!("../assets/branding/app-icon-256.png"))
        .expect("Bundled application icon must be a valid PNG")
        .into_rgba8();
    eframe::egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}
