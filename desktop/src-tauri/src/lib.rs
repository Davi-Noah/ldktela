//! Núcleo nativo do cliente desktop: cofre, bandeja e IPC.

mod capture;
mod preview;
mod publisher;
mod seen;
mod share;
mod vault;

#[cfg(target_os = "windows")]
mod audio;
/// Câmera (ADR-0038). Só existe no Windows, como a captura de áudio: o Media
/// Foundation é o caminho da plataforma, e não há segundo alvo hoje.
#[cfg(target_os = "windows")]
mod camera;

use std::sync::atomic::{AtomicU32, Ordering};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::webview::NewWindowResponse;
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Pedido de troca de conta, vindo da bandeja.
///
/// O aplicativo nao tem como saber qual conta do Discord esta aberta na maquina
/// — nao existe API para isso — entao trocar de conta e um ato explicito. Quem
/// faz o trabalho e o TypeScript, que ja sabe revogar a sessao no servidor antes
/// de apagar o token do cofre.
const SIGN_OUT_EVENT: &str = "session://sign-out";

/// Pedido de parada, vindo da bandeja ou do atalho global.
///
/// Quem para de verdade e o TypeScript: ele e que sabe se ha compartilhamento em
/// andamento e precisa atualizar a sala junto. Aqui so se pede.
const STOP_EVENT: &str = "share://stop-requested";

/// O atalho de panico. Existe porque o aplicativo vive na bandeja e a janela
/// fica escondida: descobrir que a tela errada esta no ar e precisar caçar a
/// janela para parar e tempo demais para esse tipo de erro.
fn stop_hotkey() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyE)
}

/// O que a interface precisa alcançar na bandeja para refletir o estado da
/// transmissao.
struct Tray {
    icon: TrayIcon<Wry>,
    stop: MenuItem<Wry>,
    idle: Image<'static>,
    /// O mesmo icone com um ponto vermelho no canto. O aplicativo passa o dia
    /// na bandeja com a janela escondida (RF-26): sem um sinal ali, "estou
    /// transmitindo agora?" so se responde abrindo a janela.
    live: Image<'static>,
}

/// Espelha na bandeja o que a sala ja sabe: se estamos no ar.
///
/// Sem isto, "Parar de compartilhar" estaria sempre clicavel, inclusive quando
/// nao ha nada para parar — e um menu que aceita um clique sem fazer nada e pior
/// do que um item apagado.
#[tauri::command]
fn tray_set_sharing(tray: State<'_, Tray>, sharing: bool, what: Option<String>) {
    let _ = tray.stop.set_enabled(sharing);
    let tooltip = match (sharing, what.as_deref()) {
        (true, Some(title)) if !title.trim().is_empty() => format!("ldktela — no ar: {title}"),
        (true, _) => "ldktela — no ar".to_string(),
        (false, _) => "ldktela".to_string(),
    };
    let _ = tray.icon.set_tooltip(Some(tooltip));
    let icon = if sharing { &tray.live } else { &tray.idle };
    let _ = tray.icon.set_icon(Some(icon.clone()));
}

/// Desenha o ponto de "no ar" no canto inferior direito do icone.
///
/// Feito em pixel cru de proposito: e um circulo, e trazer um decodificador de
/// imagem so para carregar um segundo PNG seria dependencia nova para vinte
/// linhas de aritmetica (CLAUDE.md 2.11). O anel escuro na borda existe porque
/// sem ele o vermelho some contra uma bandeja de tema claro.
fn with_live_dot(base: &Image<'_>) -> Image<'static> {
    const DOT: [u8; 4] = [237, 66, 69, 255];
    const RING: [u8; 4] = [18, 18, 22, 255];

    let (width, height) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    let radius = f32::from(u16::try_from(width.min(height)).unwrap_or(u16::MAX)) * 0.30;
    let center = |side: u32| side as f32 - radius - 1.0;
    let (cx, cy) = (center(width), center(height));

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let distance = dx.mul_add(dx, dy * dy).sqrt();
            if distance > radius {
                continue;
            }
            let offset = ((y * width + x) * 4) as usize;
            let Some(pixel) = rgba.get_mut(offset..offset + 4) else {
                continue;
            };
            pixel.copy_from_slice(if distance > radius - 1.5 { &RING } else { &DOT });
        }
    }

    Image::new_owned(rgba, width, height)
}

/// Entry point shared by `main.rs` and, later, by mobile targets.
pub fn run() -> tauri::Result<()> {
    let hotkey = stop_hotkey();
    let watched = hotkey;

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // So na descida: sem o filtro, soltar a tecla dispararia um
                    // segundo pedido de parada.
                    if event.state() == ShortcutState::Pressed && shortcut == &watched {
                        eprintln!("atalho: parada pedida pelo teclado");
                        let _ = app.emit(STOP_EVENT, ());
                    }
                })
                .build(),
        )
        .manage(share::Sharing::default())
        .invoke_handler(tauri::generate_handler![
            vault::vault_get_refresh_token,
            vault::vault_set_refresh_token,
            vault::vault_clear_refresh_token,
            seen::release_notes_seen,
            seen::release_notes_mark_seen,
            share::share_sources,
            share::share_thumbnail,
            share::share_start,
            share::share_stop,
            share::share_stats,
            share::share_preview,
            share::camera_list,
            share::camera_start,
            share::camera_stop,
            share::camera_stats,
            share::camera_preview,
            tray_set_sharing,
        ])
        .setup(move |app| {
            build_main_window(app)?;
            build_tray(app.handle())?;
            // Um atalho global pode ja estar tomado por outro aplicativo. Isso
            // nao e motivo para o aplicativo nao subir: perde-se o atalho, e o
            // botao e a bandeja continuam parando a transmissao.
            if let Err(error) = app.global_shortcut().register(hotkey) {
                eprintln!("atalho: Ctrl+Shift+E indisponivel ({error})");
            }
            #[cfg(target_os = "linux")]
            enable_linux_webrtc(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Fechar esconde em vez de sair. O aplicativo passa o dia na
            // bandeja esperando alguem entrar num canal de voz (RF-26); sair no
            // X faria o usuario perder o aviso de que a tela abriu.
            //
            // So a janela principal. A janela destacada (ADR-0034) precisa
            // fechar de verdade: escondida, o documento que a abriu nunca fica
            // sabendo, e o video fica preso numa janela invisivel em vez de
            // voltar para o ladrilho.
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
}

/// Cria a janela principal a partir do `tauri.conf.json` (`create: false` la),
/// so para poder decidir sobre janelas novas.
///
/// Destacar uma tela move o `<video>` para uma janela `about:blank` aberta pelo
/// proprio documento (ADR-0034) — o Document Picture-in-Picture existe no
/// WebView2 mas falha com `InvalidStateError: no window`, porque o hospedeiro
/// nao cria a janela dele. O Tauri nega `window.open` por padrao; aqui passa a
/// permitir **so** `about:blank`, que e a janela que nos mesmos abrimos e
/// preenchemos. Qualquer outro endereco continua negado, como antes.
fn build_main_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(config) = app.config().app.windows.first().cloned() else {
        return Ok(());
    };
    let handle = app.handle().clone();
    WebviewWindowBuilder::from_config(app.handle(), &config)?
        .on_new_window(move |url, features| {
            if url.as_str() != "about:blank" {
                return NewWindowResponse::Deny;
            }
            // Uma janela nossa, e nao o popup padrao do WebView2: aquele vem
            // com barra de endereco mostrando `about:blank` e o globo do
            // navegador, que e interface de navegador dentro do produto de
            // novo. `window_features` herda posicao, tamanho e o ambiente do
            // WebView2 de quem abriu — sem o mesmo ambiente o WebView2 recusa
            // a janela.
            let label = format!("destacada-{}", DETACHED.fetch_add(1, Ordering::Relaxed));
            match WebviewWindowBuilder::new(&handle, label, WebviewUrl::External(url))
                .window_features(features)
                .title("ldktela")
                .build()
            {
                Ok(window) => NewWindowResponse::Create { window },
                Err(error) => {
                    eprintln!("destacar: nao consegui criar a janela ({error})");
                    NewWindowResponse::Deny
                }
            }
        })
        .build()?;
    Ok(())
}

/// Contador so para rotular as janelas destacadas: o Tauri exige rotulo unico,
/// e reabrir depois de fechar precisa de um novo.
static DETACHED: AtomicU32 = AtomicU32::new(0);

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
    // `cloned()` copiaria o `Image` mantendo o emprestimo do `AppHandle`, e a
    // bandeja precisa guardar os dois icones pela vida do processo. Copiar os
    // bytes e o que os desprende.
    let source = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("ícone padrão da janela".into()))?;
    let idle = Image::new_owned(source.rgba().to_vec(), source.width(), source.height());
    let live = with_live_dot(&idle);

    let show = MenuItem::with_id(app, "show", "Abrir", true, None::<&str>)?;
    // Nasce apagado: so acende quando ha transmissao, via `tray_set_sharing`.
    let stop = MenuItem::with_id(
        app,
        "stop",
        "Parar de compartilhar\tCtrl+Shift+E",
        false,
        None::<&str>,
    )?;
    let switch = MenuItem::with_id(app, "switch", "Trocar de conta", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &stop, &switch, &quit])?;

    let icon = TrayIconBuilder::new()
        .icon(idle.clone())
        .tooltip("ldktela")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "stop" => {
                let _ = app.emit(STOP_EVENT, ());
            }
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

    app.manage(Tray {
        icon,
        stop,
        idle,
        live,
    });
    Ok(())
}

fn reveal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_dot_marks_the_corner_and_leaves_the_rest_alone() {
        let side = 32_u32;
        let base = Image::new_owned(vec![0u8; (side * side * 4) as usize], side, side);
        let live = with_live_dot(&base);

        assert_eq!(live.width(), side);
        assert_eq!(live.height(), side);

        let at = |x: u32, y: u32| {
            let offset = ((y * side + x) * 4) as usize;
            live.rgba()[offset..offset + 4].to_vec()
        };

        // O canto de onde ninguem olha continua intocado...
        assert_eq!(at(0, 0), vec![0, 0, 0, 0], "canto superior esquerdo");
        // ...e o canto do ponto ficou opaco e vermelho.
        let dot = at(side - 11, side - 11);
        assert_eq!(dot[3], 255, "o ponto precisa ser opaco");
        assert!(
            dot[0] > dot[1] && dot[0] > dot[2],
            "o ponto precisa ser vermelho"
        );
    }
}
