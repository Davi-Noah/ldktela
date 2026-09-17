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

use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::NV12Buffer;
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
/// 1600 px, e nao 1280: numa janela maximizada de 1080p a area util do video
/// passa de 1500 px de largura, e 1280 ali ainda seria aumento.
pub const FOCUS_MAX: Size = Size {
    width: 1600,
    height: 900,
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

/// Qualidade do JPEG. Sobe junto com a resolucao, pelo mesmo motivo que ela: em
/// foco a imagem e olhada de perto, e artefato de bloco em texto pequeno e
/// justamente o que faz um preview parecer transmissao quebrada.
const GRID_QUALITY: u8 = 70;
/// A miniatura do seletor e um cartao pequeno e frio: existe para distinguir
/// tres janelas do mesmo navegador, e nao para ser lida.
pub const THUMBNAIL_QUALITY: u8 = 72;
const FOCUS_QUALITY: u8 = 80;

/// Um quadro reduzido, BGRA compacto (sem sobra de stride).
pub struct Raw {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Viaja com o quadro porque quem codifica esta noutra thread e nao tem como
    /// saber se este veio da grade ou do foco.
    pub quality: u8,
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

    fn quality(&self) -> u8 {
        if self.max_width.load(Ordering::Relaxed) > GRID_MAX.width {
            FOCUS_QUALITY
        } else {
            GRID_QUALITY
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
    /// Oferece o quadro recem-capturado, **ja em NV12**, que e a forma em que a
    /// captura o entregou ao encoder.
    ///
    /// Chamado depois de o encoder ja ter recebido o seu, para que um preview
    /// lento jamais adie a transmissao. Reaproveitar o NV12 em vez de reduzir o
    /// BGRA cru sai mais barato — a conversao ja foi paga — e sai melhor: quem
    /// reduz e o libyuv, com filtro, e nao um laco de vizinho mais proximo que
    /// joga fora uma coluna a cada seis e transforma texto em serrilhado.
    pub fn offer(&mut self, frame: &mut NV12Buffer, source: Size) {
        if !self.control.enabled() {
            return;
        }
        let now = Instant::now();
        if now < self.next {
            return;
        }
        self.next = now + self.control.period();

        let target = fit(source, self.control.ceiling());
        let mut small = frame.scale(target.width as i32, target.height as i32);
        let Some(pixels) = nv12_to_bgra(&mut small, target) else {
            return;
        };
        // `try_send` e nao `send`: se o codificador ainda esta no quadro
        // anterior, este e descartado. Um preview atrasado nao vale segurar a
        // thread que alimenta o encoder.
        let _ = self.tx.try_send(Raw {
            width: target.width,
            height: target.height,
            pixels,
            quality: self.control.quality(),
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

/// Reduz um quadro BGRA cru para BGRA compacto, com filtro.
///
/// O caminho passa pelo NV12 porque e la que mora o redimensionador do libyuv,
/// que filtra. A versao anterior escolhia o pixel mais proximo, o que de 1920
/// para 1600 descarta uma coluna a cada seis: numa tela de codigo ou de
/// planilha isso nao reduz, desmancha.
///
/// So a miniatura do seletor entra por aqui. O preview ao vivo chama
/// `nv12_to_bgra` direto, porque a captura ja converteu o quadro para NV12 para
/// entregar ao encoder, e converter de novo seria pagar duas vezes.
pub fn downscale(data: &[u8], stride: u32, source: Size, target: Size) -> Option<Vec<u8>> {
    if source.width == 0 || source.height == 0 || target.width == 0 || target.height == 0 {
        return None;
    }
    let needed = (stride as usize).checked_mul(source.height as usize)?;
    if data.len() < needed || stride < source.width.checked_mul(4)? {
        return None;
    }

    let mut full = NV12Buffer::new(source.width, source.height);
    let (stride_y, stride_uv) = full.strides();
    let (dst_y, dst_uv) = full.data_mut();
    yuv_helper::argb_to_nv12(
        data,
        stride,
        dst_y,
        stride_y,
        dst_uv,
        stride_uv,
        source.width as i32,
        source.height as i32,
    );

    let mut small = full.scale(target.width as i32, target.height as i32);
    nv12_to_bgra(&mut small, target)
}

/// NV12, ja no tamanho de saida, para o BGRA compacto que o codificador de JPEG
/// aceita.
fn nv12_to_bgra(frame: &mut NV12Buffer, size: Size) -> Option<Vec<u8>> {
    let width = usize::try_from(size.width).ok()?;
    let height = usize::try_from(size.height).ok()?;
    let mut out = vec![0u8; width.checked_mul(height)?.checked_mul(4)?];
    let (stride_y, stride_uv) = frame.strides();
    let (src_y, src_uv) = frame.data();
    yuv_helper::nv12_to_argb(
        src_y,
        stride_y,
        src_uv,
        stride_uv,
        &mut out,
        size.width * 4,
        size.width as i32,
        size.height as i32,
    );
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
    let encoder = jpeg_encoder::Encoder::new(&mut jpeg, frame.quality);
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
    fn reducing_produces_a_tight_buffer_of_the_asked_size() {
        let source = Size {
            width: 64,
            height: 32,
        };
        let target = Size {
            width: 16,
            height: 8,
        };
        let data = vec![0x7f_u8; 64 * 4 * 32];
        let out = downscale(&data, 64 * 4, source, target).expect("deve reduzir");
        assert_eq!(out.len(), 16 * 8 * 4);
        // Cinza uniforme continua cinza uniforme: a ida e volta pelo NV12 tira
        // alguns niveis, e por isso a faixa em vez da igualdade exata.
        for pixel in out.as_chunks::<4>().0 {
            for channel in &pixel[..3] {
                assert!(
                    channel.abs_diff(0x7f) <= 4,
                    "cinza uniforme saiu manchado: {out:?}"
                );
            }
        }
    }

    /// O capturador entrega linhas com sobra no fim (`stride` maior que a
    /// largura util). Ler como se fosse compacto embaralharia a imagem.
    #[test]
    fn a_padded_stride_does_not_shift_the_image() {
        let source = Size {
            width: 8,
            height: 4,
        };
        let stride: u32 = 8 * 4 + 12;
        let mut data = vec![0u8; (stride as usize) * 4];
        // Metade de baixo branca, metade de cima preta. Passa por YUV, entao o
        // que se checa e claro contra escuro, nao 0x00 contra 0xff.
        for byte in data.iter_mut().skip((stride as usize) * 2) {
            *byte = 0xff;
        }
        let out = downscale(&data, stride, source, source).expect("deve reduzir");
        // Só os canais de cor: o alfa sai sempre em 0xff, e incluí-lo faria o
        // lado escuro parecer claro.
        let luminance: Vec<u8> = out
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[0])
            .collect();
        let (top, bottom) = luminance.split_at(8 * 2);
        assert!(
            top.iter().all(|value| *value < 0x40),
            "metade de cima deveria estar escura, veio {top:?}"
        );
        assert!(
            bottom.iter().all(|value| *value > 0xc0),
            "metade de baixo deveria estar clara, veio {bottom:?}"
        );
    }

    #[test]
    fn a_short_buffer_is_refused_instead_of_read_past_the_end() {
        let source = Size {
            width: 64,
            height: 32,
        };
        assert!(downscale(&[0u8; 16], 64 * 4, source, GRID_MAX).is_none());
    }

    /// Reduzir de verdade preserva a borda; escolher o pixel mais proximo a
    /// perde. E a diferenca entre texto legivel e texto serrilhado no preview,
    /// que foi relatada como "qualidade comicamente baixa" da transmissao.
    #[test]
    fn reducing_by_half_keeps_a_thin_line_instead_of_dropping_it() {
        let source = Size {
            width: 16,
            height: 16,
        };
        let target = Size {
            width: 8,
            height: 8,
        };
        // Colunas alternadas, uma clara e uma escura. O vizinho mais proximo
        // devolveria oito colunas de uma so cor; um filtro devolve o meio-termo.
        let mut data = vec![0u8; 16 * 16 * 4];
        for y in 0..16 {
            for x in (0..16).step_by(2) {
                let i = (y * 16 + x) * 4;
                data[i..i + 4].fill(0xff);
            }
        }
        let out = downscale(&data, 16 * 4, source, target).expect("deve reduzir");
        let extremes = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] < 0x20 || pixel[0] > 0xe0)
            .count();
        assert!(
            extremes < out.len() / 4 / 2,
            "a maioria dos pixels saiu no extremo: o filtro nao entrou,              sobraram {extremes} de {}",
            out.len() / 4
        );
    }

    /// What the preview costs the capture thread, and what it costs the IPC.
    ///
    /// Both numbers are load-bearing and neither is guessable. The reduction
    /// runs **inside** the capture loop, which at 60 fps has 16 ms per frame for
    /// everything it does; and the encoded frame crosses to the WebView as a
    /// base64 data URL, so its size is bandwidth on a channel shared with every
    /// other event the application sends.
    ///
    /// Uses a real screen, because a synthetic image measures the wrong thing:
    /// noise is the JPEG's worst case and flat colour is its best, and a desktop
    /// is neither.
    ///
    /// ```text
    /// cargo test --release -- --ignored --nocapture preview_costs
    /// ```
    #[test]
    #[ignore]
    fn preview_costs_what_it_is_worth() {
        use crate::capture::{self, SourceKind};
        use std::time::Instant;

        let sources = capture::list_sources(&[]);
        let screen = sources
            .iter()
            .find(|s| s.kind == SourceKind::Screen)
            .expect("deve existir uma tela");
        let full = capture::thumbnail(
            SourceKind::Screen,
            screen.id.parse().expect("id numerico"),
            Size {
                width: 1920,
                height: 1080,
            },
        )
        .expect("a tela deve entregar um quadro");
        let source = Size {
            width: full.width,
            height: full.height,
        };
        println!("tela de origem: {}x{}", source.width, source.height);

        // O caminho vivo nao reconverte BGRA para NV12: a captura ja fez isso
        // para o encoder, e o `Tap` reduz a partir do mesmo buffer. Medir o
        // `downscale` inteiro contaria uma conversao que o produto nao paga.
        let mut scratch = NV12Buffer::new(source.width, source.height);
        {
            let (stride_y, stride_uv) = scratch.strides();
            let (dst_y, dst_uv) = scratch.data_mut();
            yuv_helper::argb_to_nv12(
                &full.pixels,
                source.width * 4,
                dst_y,
                stride_y,
                dst_uv,
                stride_uv,
                source.width as i32,
                source.height as i32,
            );
        }

        const ROUNDS: u32 = 20;
        for (label, ceiling, quality, fps) in [
            ("grade", GRID_MAX, GRID_QUALITY, 3.0),
            ("foco", FOCUS_MAX, FOCUS_QUALITY, 15.0),
        ] {
            let target = fit(source, ceiling);

            let at = Instant::now();
            let mut pixels = Vec::new();
            for _ in 0..ROUNDS {
                let mut small = scratch.scale(target.width as i32, target.height as i32);
                pixels = nv12_to_bgra(&mut small, target).expect("deve converter");
            }
            let reduce_ms = at.elapsed().as_secs_f64() * 1000.0 / f64::from(ROUNDS);

            let frame = Raw {
                width: target.width,
                height: target.height,
                pixels,
                quality,
            };
            let at = Instant::now();
            let mut url = String::new();
            for _ in 0..ROUNDS {
                url = encode_data_url(&frame).expect("deve codificar");
            }
            let encode_ms = at.elapsed().as_secs_f64() * 1000.0 / f64::from(ROUNDS);

            println!(
                "{label:<6} {:>9}  reducao {reduce_ms:>5.2} ms/quadro ({:>4.1} % da thread de                  captura)  jpeg {encode_ms:>5.2} ms  url {:>4} KB  ipc {:>5.0} KB/s",
                format!("{}x{}", target.width, target.height),
                reduce_ms * fps / 10.0,
                url.len() / 1024,
                url.len() as f64 * fps / 1024.0,
            );

            // Metade do orcamento de um quadro a 60 fps. Acima disso o preview
            // deixa de ser um ramo frio e passa a tirar quadros da transmissao,
            // que e exatamente o que o ADR-0030 proibiu.
            assert!(
                reduce_ms < 8.0,
                "a reducao do preview {label} custa {reduce_ms:.2} ms dentro da thread de captura"
            );
        }
    }

    #[test]
    fn a_frame_becomes_a_data_url_an_img_can_load() {
        let frame = Raw {
            width: 16,
            height: 16,
            pixels: vec![0x40; 16 * 16 * 4],
            quality: GRID_QUALITY,
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

    /// Pixels a mais com o mesmo JPEG de ladrilho trocariam borrão por bloco.
    /// As duas coisas são qualidade, e as duas acompanham o contexto.
    #[test]
    fn focusing_the_preview_also_raises_the_jpeg_quality() {
        let control = Control::new();
        control.set(true, GRID_FPS, false);
        assert_eq!(control.quality(), GRID_QUALITY);
        control.set(true, 15, true);
        assert_eq!(control.quality(), FOCUS_QUALITY);
        const { assert!(FOCUS_QUALITY > GRID_QUALITY) };
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
