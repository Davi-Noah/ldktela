//! Núcleo nativo do cliente desktop: cofre, bandeja e IPC.

mod capture;
mod publisher;
mod share;
mod vault;

#[cfg(target_os = "windows")]
mod audio;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};

/// Pedido de troca de conta, vindo da bandeja.
///
/// O aplicativo nao tem como saber qual conta do Discord esta aberta na maquina
/// — nao existe API para isso — entao trocar de conta e um ato explicito. Quem
/// faz o trabalho e o TypeScript, que ja sabe revogar a sessao no servidor antes
/// de apagar o token do cofre.
const SIGN_OUT_EVENT: &str = "session://sign-out";

/// Entry point shared by `main.rs` and, later, by mobile targets.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(share::Sharing::default())
        .invoke_handler(tauri::generate_handler![
            vault::vault_get_refresh_token,
            vault::vault_set_refresh_token,
            vault::vault_clear_refresh_token,
            share::share_sources,
            share::share_start,
            share::share_stop,
            share::share_stats,
        ])
        .setup(|app| {
            build_tray(app.handle())?;
            #[cfg(target_os = "linux")]
            enable_linux_webrtc(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Fechar esconde em vez de sair. O aplicativo passa o dia na
            // bandeja esperando alguem entrar num canal de voz (RF-26); sair no
            // X faria o usuario perder o aviso de que a tela abriu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
}

/// O WebKitGTK entrega `enable-webrtc` e `enable-media-stream` desligados, e o
/// Tauri nao os religa: sem isto o livekit-client recusa com "LiveKit doesn't
/// seem to be supported on this browser" antes mesmo de abrir a sinalizacao, e
/// nada aparece no log do servidor. No WebView2 do Windows nao ha equivalente.
///
/// Falhar aqui nao impede o aplicativo de subir — so o compartilhamento nao vai
/// funcionar — entao o erro e reportado em vez de derrubar o processo.
#[cfg(target_os = "linux")]
fn enable_linux_webrtc(app: &tauri::AppHandle) {
    use webkit2gtk::{SettingsExt, WebViewExt};

    let Some(window) = app.get_webview_window("main") else {
        eprintln!("webrtc: janela principal ausente, WebRTC segue desligado");
        return;
    };
    let applied = window.with_webview(|webview| {
        if let Some(settings) = WebViewExt::settings(&webview.inner()) {
            settings.set_enable_webrtc(true);
            settings.set_enable_media_stream(true);
        }
    });
    if let Err(error) = applied {
        eprintln!("webrtc: nao consegui ajustar o WebKitGTK: {error}");
    }
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Abrir", true, None::<&str>)?;
    let switch = MenuItem::with_id(app, "switch", "Trocar de conta", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &switch, &quit])?;

    TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("ícone padrão da janela".into()))?,
        )
        .tooltip("ldkcord")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "switch" => {
                // Abrir junto: a tela de pareamento nao serve para nada na
                // bandeja, e sem isso o clique nao parece ter feito nada.
                reveal(app);
                let _ = app.emit(SIGN_OUT_EVENT, ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn reveal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
