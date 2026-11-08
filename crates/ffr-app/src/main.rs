//! Fast File Recovery: CLI flags, self-install dispatch, elevation and the GUI.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod branding;
mod elevate;
mod install;
mod paths;
mod platform_window;
mod preview;
mod screens;
mod settings;
mod theme;
mod updater;
mod widgets;

use branding::{APP_NAME, VERSION};
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "Usage: fast-file-recovery [--version] [--here] [--uninstall [--delete-data]]

  --version              print the version and exit
  --here                 run without installing or asking to install
  --uninstall            remove the installed copy (keeps settings and saved scans)
  --delete-data          with --uninstall, also delete settings, saved scans and logs";

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    let dir = paths::logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("ffr")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init()
        .ok()?;
    elevate::give_to_original_user(&dir);
    Some(guard)
}

fn icon() -> Option<eframe::egui::IconData> {
    let img = image::load_from_memory(branding::ICON_256).ok()?.to_rgba8();
    Some(eframe::egui::IconData {
        width: img.width(),
        height: img.height(),
        rgba: img.into_raw(),
    })
}

fn native_options(
    settings: &settings::Settings,
    renderer: eframe::Renderer,
) -> eframe::NativeOptions {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_app_id(branding::APP_ID)
        .with_inner_size([1200.0, 780.0])
        .with_min_inner_size([960.0, 640.0]);
    if let Some(w) = settings.window {
        if w.w >= 960.0 && w.h >= 640.0 {
            viewport = viewport.with_inner_size([w.w, w.h]);
        }
        if w.x > -2000.0 && w.y > -2000.0 {
            viewport = viewport.with_position([w.x, w.y]);
        }
    }
    if let Some(i) = icon() {
        viewport = viewport.with_icon(Arc::new(i));
    }
    eframe::NativeOptions {
        viewport,
        renderer,
        persist_window: false,
        ..Default::default()
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--version") || has("-V") {
        platform_window::attach_console();
        println!("{APP_NAME} {VERSION}");
        return ExitCode::SUCCESS;
    }
    if has("--help") || has("-h") {
        platform_window::attach_console();
        println!("{APP_NAME} {VERSION} — {}\n\n{USAGE}", branding::MADE_BY);
        return ExitCode::SUCCESS;
    }
    if has("--uninstall") {
        platform_window::attach_console();
        return match install::uninstall(has("--delete-data")) {
            Ok(msg) => {
                println!("{msg}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if let Some(unknown) = args
        .iter()
        .find(|a| a.starts_with('-') && !["--here", "--elevated"].contains(&a.as_str()))
    {
        platform_window::attach_console();
        eprintln!("unknown option {unknown}\n\n{USAGE}");
        return ExitCode::from(2);
    }
    elevate::set_elevated_flag(has("--elevated"));
    let _log = init_logging();
    tracing::info!("{APP_NAME} {VERSION} starting ({})", branding::os_arch());
    install::cleanup_old();
    let startup = if has("--here") || elevate::is_elevated_instance() || elevate::is_elevated() {
        install::StartupOffer::None
    } else {
        install::startup_offer()
    };
    let settings = settings::Settings::load();

    let run = |renderer: eframe::Renderer,
               settings: settings::Settings,
               startup: install::StartupOffer| {
        let opts = native_options(&settings, renderer);
        eframe::run_native(
            APP_NAME,
            opts,
            Box::new(move |cc| Ok(Box::new(app::FfrApp::new(cc, settings, startup)))),
        )
    };
    let result = match run(eframe::Renderer::Wgpu, settings.clone(), startup.clone()) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("wgpu renderer failed ({e}); falling back to OpenGL");
            run(eframe::Renderer::Glow, settings, startup)
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("could not start the window: {e}");
            platform_window::attach_console();
            eprintln!("Fast File Recovery could not open its window: {e}");
            ExitCode::FAILURE
        }
    }
}
