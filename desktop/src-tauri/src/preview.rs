//! O preview da propria tela, e as miniaturas do seletor (ADR-0030).
//!
//! Os pixels ja estao nesta maquina, saindo do `DesktopCapturer`. Eles nunca
//! precisam ir ate o SFU e voltar: assinar a propria publicacao custaria egress
//! mais ingress para receber de volta o que ja esta aqui, e — pior — daria a
//! track um inscritor permanente, de modo que o `dynacast` nunca mais pausaria
//! o encoder. Pagar-se-ia um nucleo de codificacao para transmitir para si
//! mesmo.
//!
//! O ramo e deliberadamente frio: relogio proprio, subamostragem barata na
//! thread de captura, e codificacao numa thread separada que **descarta** quadro
//! quando fica para tras. Nada aqui pode atrasar o caminho do encoder.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::capture::{fit, Size};

/// Emitido a cada quadro de preview, como data URL pronta para `img.src`.
const PREVIEW_EVENT: &str = "share://preview";

/// Teto do preview **na grade**, onde ele e um ladrilho entre outros. 480 px
/// chegam para responder "e esta a janela certa?" e "continua indo?". Em JPEG
/// q70 isso da cerca de 20 KB por quadro, contra 518 KB do mesmo quadro em RGBA
/// cru.
pub const GRID_MAX: Size = Size {
    width: 480,
    height: 270,
};

/// Teto do preview **em foco**, onde ele ocupa a janela inteira.
///
/// O ADR-0030 usava 480 px nos dois casos e variava so o relogio. Errado: em
/// foco, 480 px esticados para 1280 sao tres vezes o tamanho original, e o
/// resultado e borrado a ponto de o usuario concluir que a transmissao esta
/// quebrada — foi exatamente o que aconteceu. A resolucao acompanha o contexto
/// pelo mesmo motivo que o relogio ja acompanhava.
pub const FOCUS_MAX: Size = Size {
    width: 1280,
    height: 720,
};

/// Miniatura do seletor. Menor que o preview, e suficiente: ela existe para
/// distinguir tres janelas do mesmo navegador, nao para ser lida.
pub const THUMBNAIL_MAX: Size = Size {
    width: 320,
    height: 180,
};

/// Quadros por segundo do preview na grade. Ele e uma confirmacao, nao um
/// monitor: tres por segundo mostram que a imagem esta viva e se mexe.
pub const GRID_FPS: u32 = 3;

const QUALITY: u8 = 70;

/// Um quadro subamostrado, BGRA compacto (sem sobra de stride).
pub struct Raw {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// O que a interface liga e desliga. Compartilhado entre a thread de captura e
/// os comandos do Tauri.
pub struct Control {
    enabled: AtomicBool,
    period_ms: AtomicU64,
    /// Largura maxima do quadro. Anda junto com o relogio: em foco o preview
    /// enche a janela, e uma imagem pensada para ladrilho fica borrada ali.
    max_width: AtomicU32,
}

impl Control {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(true),
            period_ms: AtomicU64::new(period_ms(GRID_FPS)),
            max_width: AtomicU32::new(GRID_MAX.width),
        }
    }

    /// `enabled: false` para o ramo **na origem**: a thread de captura deixa de
    /// subamostrar. Esconder o elemento no WebView nao economizaria nada.
    pub fn set(&self, enabled: bool, fps: u32, focused: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.period_ms.store(period_ms(fps), Ordering::Relaxed);
        let max = if focused { FOCUS_MAX } else { GRID_MAX };
        self.max_width.store(max.width, Ordering::Relaxed);
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn period(&self) -> Duration {
        Duration::from_millis(self.period_ms.load(Ordering::Relaxed))
    }

    /// O teto de agora. A altura acompanha a largura pela proporcao de 16:9, e o
    /// `fit` corta para o que a fonte realmente tem — uma tela 4:3 nao vira
    /// 16:9 por causa disto.
    fn ceiling(&self) -> Size {
        let width = self.max_width.load(Ordering::Relaxed);
        Size {
            width,
            height: width * 9 / 16,
        }
    }
}

fn period_ms(fps: u32) -> u64 {
    1000 / u64::from(fps.clamp(1, 30))
}

/// A ponta que vive dentro do laco de captura.
pub struct Tap {
    control: Arc<Control>,
    tx: SyncSender<Raw>,
    next: Instant,
}

impl Tap {
    /// Oferece o quadro recem-capturado. Chamado **depois** de o encoder ja ter
    /// recebido o seu, para que um preview lento jamais adie a transmissao.
    pub fn offer(&mut self, data: &[u8], stride: u32, source: Size) {
        if !self.control.enabled() {
            return;
        }
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = now + self.control.period();

        let target = fit(source, self.control.ceiling());
        let Some(pixels) = subsample(data, stride, source, target) else {
            return;
        };
        // `try_send` e nao `send`: se o codificador ainda esta no quadro
        // anterior, este e descartado. Um preview atrasado nao vale segurar a
        // thread que alimenta o encoder.
        let _ = self.tx.try_send(Raw {
            width: target.width,
            height: target.height,
            pixels,
        });
    }
}

/// O lado que fica com quem iniciou o compartilhamento.
pub struct Preview {
    control: Arc<Control>,
    worker: Option<JoinHandle<()>>,
}

impl Preview {
    pub fn control(&self) -> Arc<Control> {
        Arc::clone(&self.control)
    }

    /// Junta a thread de codificacao. So retorna depois de a captura ter parado
    /// e, com ela, o ultimo `Tap` ter sido descartado — e o fechamento do canal
    /// que encerra o laco do trabalhador.
    pub fn stop(mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Abre o par: o controle fica com o comando, o `Tap` vai para a captura.
pub fn start(app: AppHandle) -> (Preview, Tap) {
    // Profundidade 1: interessa o quadro mais recente, nunca uma fila deles.
    let (tx, rx) = sync_channel::<Raw>(1);
    let control = Arc::new(Control::new());

    let worker = std::thread::Builder::new()
        .name("ldkcord-preview".into())
        .spawn(move || {
            while let Ok(frame) = rx.recv() {
                if let Some(url) = encode_data_url(&frame) {
                    let _ = app.emit(PREVIEW_EVENT, url);
                }
            }
        })
        .ok();

    let tap = Tap {
        control: Arc::clone(&control),
        tx,
        next: Instant::now(),
    };
    (Preview { control, worker }, tap)
}

/// Vizinho mais proximo, BGRA para BGRA compacto.
///
/// Nao e interpolacao, e e de proposito: bilinear custaria quatro leituras por
/// pixel na thread que alimenta o encoder, para uma imagem de 480 px que existe
/// so para o usuario reconhecer a propria janela.
pub fn subsample(data: &[u8], stride: u32, source: Size, target: Size) -> Option<Vec<u8>> {
    if source.width == 0 || source.height == 0 || target.width == 0 || target.height == 0 {
        return None;
    }
    let row = stride as usize;
    let needed = row.checked_mul(source.height as usize)?;
    if data.len() < needed {
        return None;
    }

    let mut out = vec![0u8; (target.width as usize) * (target.height as usize) * 4];
    for y in 0..target.height {
        let sy = (u64::from(y) * u64::from(source.height) / u64::from(target.height)) as usize;
        let src_row = sy * row;
        let dst_row = (y as usize) * (target.width as usize) * 4;
        for x in 0..target.width {
            let sx = (u64::from(x) * u64::from(source.width) / u64::from(target.width)) as usize;
            let si = src_row + sx * 4;
            let di = dst_row + (x as usize) * 4;
            let (Some(src), Some(dst)) = (data.get(si..si + 4), out.get_mut(di..di + 4)) else {
                continue;
            };
            dst.copy_from_slice(src);
        }
    }
    Some(out)
}

/// JPEG em base64, no formato que o `src` de uma `<img>` aceita direto.
///
/// Data URL e nao bytes crus porque o destino e uma `<img>`: o WebView
/// decodifica fora da thread principal e desenha sozinho, sem canvas, sem
/// `ImageBitmap` e sem uma linha de JavaScript por quadro (CLAUDE.md 7).
pub fn encode_data_url(frame: &Raw) -> Option<String> {
    let (Ok(width), Ok(height)) = (u16::try_from(frame.width), u16::try_from(frame.height)) else {
        return None;
    };
    let mut jpeg = Vec::new();
    let encoder = jpeg_encoder::Encoder::new(&mut jpeg, QUALITY);
    if encoder
        .encode(&frame.pixels, width, height, jpeg_encoder::ColorType::Bgra)
        .is_err()
    {
        return None;
    }
    let mut url = String::from("data:image/jpeg;base64,");
    base64_into(&jpeg, &mut url);
    Some(url)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 proprio: sao vinte linhas, e evita mais uma dependencia so para
/// escrever quatro caracteres a cada tres bytes (CLAUDE.md 2.11).
fn base64_into(bytes: &[u8], out: &mut String) {
    out.reserve(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk.first().copied().map_or(0, u32::from);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(char::from(ALPHABET[(triple >> 18) as usize & 63]));
        out.push(char::from(ALPHABET[(triple >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(ALPHABET[(triple >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(ALPHABET[triple as usize & 63])
        } else {
            '='
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_canonical_examples() {
        let mut out = String::new();
        base64_into(b"", &mut out);
        assert_eq!(out, "");

        out.clear();
        base64_into(b"f", &mut out);
        assert_eq!(out, "Zg==");

        out.clear();
        base64_into(b"fo", &mut out);
        assert_eq!(out, "Zm8=");

        out.clear();
        base64_into(b"foo", &mut out);
        assert_eq!(out, "Zm9v");

        out.clear();
        base64_into(b"foobar", &mut out);
        assert_eq!(out, "Zm9vYmFy");
    }

    #[test]
    fn subsampling_produces_a_tight_buffer_of_the_asked_size() {
        let source = Size {
            width: 64,
            height: 32,
        };
        let target = Size {
            width: 16,
            height: 8,
        };
        let data = vec![0x7f_u8; 64 * 4 * 32];
        let out = subsample(&data, 64 * 4, source, target).expect("deve subamostrar");
        assert_eq!(out.len(), 16 * 8 * 4);
        assert!(out.iter().all(|byte| *byte == 0x7f));
    }

    /// O capturador entrega linhas com sobra no fim (`stride` maior que a
    /// largura util). Ler como se fosse compacto embaralharia a imagem.
    #[test]
    fn a_padded_stride_does_not_shift_the_image() {
        let source = Size {
            width: 4,
            height: 2,
        };
        let stride: u32 = 4 * 4 + 8;
        let mut data = vec![0u8; (stride as usize) * 2];
        // Segunda linha toda em 0xff; a primeira fica em zero.
        for byte in data.iter_mut().skip(stride as usize) {
            *byte = 0xff;
        }
        let out = subsample(&data, stride, source, source).expect("deve subamostrar");
        assert!(out[..16].iter().all(|byte| *byte == 0), "primeira linha");
        assert!(out[16..].iter().all(|byte| *byte == 0xff), "segunda linha");
    }

    #[test]
    fn a_short_buffer_is_refused_instead_of_read_past_the_end() {
        let source = Size {
            width: 64,
            height: 32,
        };
        assert!(subsample(&[0u8; 16], 64 * 4, source, GRID_MAX).is_none());
    }

    #[test]
    fn a_frame_becomes_a_data_url_an_img_can_load() {
        let frame = Raw {
            width: 16,
            height: 16,
            pixels: vec![0x40; 16 * 16 * 4],
        };
        let url = encode_data_url(&frame).expect("deve codificar");
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert!(url.len() > 64, "url curta demais para conter um JPEG");
    }

    #[test]
    fn the_preview_clock_never_divides_by_zero_nor_runs_wild() {
        let control = Control::new();
        control.set(true, 0, false);
        assert_eq!(control.period(), Duration::from_millis(1000));
        control.set(true, 1000, false);
        assert_eq!(control.period(), Duration::from_millis(33));
    }

    /// Em foco o preview enche a janela, e o teto de ladrilho deixava a imagem
    /// borrada a ponto de parecer defeito da transmissão — o que de fato foi
    /// relatado como defeito da transmissão.
    #[test]
    fn focusing_the_preview_raises_the_resolution_and_not_only_the_clock() {
        let control = Control::new();

        control.set(true, GRID_FPS, false);
        assert_eq!(control.ceiling().width, GRID_MAX.width);

        control.set(true, 12, true);
        assert_eq!(control.ceiling().width, FOCUS_MAX.width);
        assert!(
            control.ceiling().width > GRID_MAX.width * 2,
            "em foco o preview cresce de verdade, e não por um punhado de pixels"
        );
    }

    #[test]
    fn a_four_by_three_screen_does_not_come_out_stretched() {
        // O teto é 16:9, mas quem decide o formato é a fonte: `fit` preserva a
        // proporção, então uma tela 4:3 continua 4:3.
        let control = Control::new();
        control.set(true, 12, true);
        let source = Size {
            width: 1600,
            height: 1200,
        };
        let target = fit(source, control.ceiling());
        assert_eq!(
            target.width * 3,
            target.height * 4,
            "proporção 4:3 preservada, veio {target:?}"
        );
    }
}
