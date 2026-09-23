//! Camera pixel formats into NV12 (ADR-0039, decision 7).
//!
//! The Media Foundation path never needs this: it asks the source reader for
//! NV12 and the reader inserts whatever converter is missing. DirectShow has no
//! equivalent, so the DirectShow path negotiates a format the device **already
//! emits** and converts here.
//!
//! Everything in this file is a pure function over byte slices, which is the
//! point: it is the half of the DirectShow work that can be proven without a
//! camera in the machine.

use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::video_frame::NV12Buffer;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    MEDIASUBTYPE_I420, MEDIASUBTYPE_IYUV, MEDIASUBTYPE_NV12, MEDIASUBTYPE_RGB24,
    MEDIASUBTYPE_RGB32, MEDIASUBTYPE_UYVY, MEDIASUBTYPE_YUY2,
};

use crate::capture::Size;

/// A pixel layout we know how to turn into NV12.
///
/// Deliberately short. A device that offers none of these is refused with a
/// message of its own instead of letting DirectShow assemble a decoder for us
/// (ADR-0039, decision 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pixels {
    Nv12,
    I420,
    Yuy2,
    Uyvy,
    /// Windows `RGB32`: B, G, R, X per pixel, and bottom-up by convention.
    Rgb32,
    /// Windows `RGB24`: B, G, R per pixel, rows padded to 4 bytes, bottom-up.
    Rgb24,
}

/// Formats in the order we would rather have them.
///
/// NV12 first because it is what the encoder wants and costs a memcpy; then the
/// other YUV layouts, which are a cheap shuffle; RGB last, because it means the
/// device is converting for us and throwing away the chroma subsampling we are
/// about to redo.
pub(crate) const PREFERENCE: [Pixels; 6] = [
    Pixels::Nv12,
    Pixels::I420,
    Pixels::Yuy2,
    Pixels::Uyvy,
    Pixels::Rgb32,
    Pixels::Rgb24,
];

impl Pixels {
    pub(crate) fn from_subtype(subtype: &GUID) -> Option<Self> {
        // `MEDIASUBTYPE_I420` and `MEDIASUBTYPE_IYUV` are different GUIDs for
        // the same three planes.
        match *subtype {
            s if s == MEDIASUBTYPE_NV12 => Some(Self::Nv12),
            s if s == MEDIASUBTYPE_I420 || s == MEDIASUBTYPE_IYUV => Some(Self::I420),
            s if s == MEDIASUBTYPE_YUY2 => Some(Self::Yuy2),
            s if s == MEDIASUBTYPE_UYVY => Some(Self::Uyvy),
            s if s == MEDIASUBTYPE_RGB32 => Some(Self::Rgb32),
            s if s == MEDIASUBTYPE_RGB24 => Some(Self::Rgb24),
            _ => None,
        }
    }

    /// Bytes per row, as DirectShow lays the format out for `biWidth` pixels.
    ///
    /// RGB rows are padded to a 4-byte boundary; YUV rows are not, because
    /// `biWidth` is already the aligned width for those.
    pub(crate) fn stride(self, width: u32) -> usize {
        let width = width as usize;
        match self {
            Self::Nv12 | Self::I420 => width,
            Self::Yuy2 | Self::Uyvy => width * 2,
            Self::Rgb32 => width * 4,
            Self::Rgb24 => (width * 3).div_ceil(4) * 4,
        }
    }

    /// Bytes in one whole frame.
    pub(crate) fn frame_bytes(self, size: Size) -> usize {
        let rows = size.height as usize;
        let stride = self.stride(size.width);
        match self {
            // Luma plus half a plane of chroma.
            Self::Nv12 | Self::I420 => stride * rows + stride * rows.div_ceil(2),
            _ => stride * rows,
        }
    }

    /// Whether row 0 of the buffer is the **bottom** of the picture.
    ///
    /// The RGB formats are stored bottom-up in a `VIDEOINFOHEADER` with positive
    /// height, which is the Windows bitmap convention and the single easiest way
    /// to ship a camera that shows everyone upside down.
    fn bottom_up(self) -> bool {
        matches!(self, Self::Rgb32 | Self::Rgb24)
    }
}

/// Converts frames of one layout, reusing its scratch between them.
pub(crate) struct Converter {
    kind: Pixels,
    /// Only `Rgb24` uses it: libyuv has no 24-bit entry point in the binding we
    /// have, so those frames are widened to 32-bit first.
    scratch: Vec<u8>,
}

impl Converter {
    pub(crate) fn new(kind: Pixels) -> Self {
        Self {
            kind,
            scratch: Vec::new(),
        }
    }

    pub(crate) fn kind(&self) -> Pixels {
        self.kind
    }

    /// Writes one frame of `src` into `dst`, returning false if `src` is short.
    ///
    /// Refusing is the right answer to a short buffer: a camera that hands over
    /// half a frame is a camera we skip for one frame, not a reason to read past
    /// the end of it.
    pub(crate) fn write(&mut self, src: &[u8], size: Size, dst: &mut NV12Buffer) -> bool {
        let (width, height) = (size.width, size.height);
        if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
            return false;
        }
        if src.len() < self.kind.frame_bytes(size) {
            return false;
        }

        let stride = self.kind.stride(width);
        let (stride_y, stride_uv) = dst.strides();
        let rows = height as usize;
        {
            let (dst_y, dst_uv) = dst.data_mut();
            if dst_y.len() < stride_y as usize * rows
                || dst_uv.len() < stride_uv as usize * (rows / 2)
            {
                return false;
            }
        }

        // libyuv reads a bottom-up source when the height is negative, and always
        // writes the destination top-down.
        let signed_height = if self.kind.bottom_up() {
            -(height as i32)
        } else {
            height as i32
        };

        match self.kind {
            Pixels::Nv12 => {
                let (dst_y, dst_uv) = dst.data_mut();
                nv12_into(
                    src,
                    stride,
                    size,
                    dst_y,
                    stride_y as usize,
                    dst_uv,
                    stride_uv as usize,
                )
            }
            Pixels::I420 => {
                let luma = stride * rows;
                let chroma_stride = stride / 2;
                let chroma = chroma_stride * (rows / 2);
                let (y, rest) = src.split_at(luma);
                let (u, rest) = rest.split_at(chroma);
                let v = &rest[..chroma];
                let (dst_y, dst_uv) = dst.data_mut();
                yuv_helper::i420_to_nv12(
                    y,
                    stride as u32,
                    u,
                    chroma_stride as u32,
                    v,
                    chroma_stride as u32,
                    dst_y,
                    stride_y,
                    dst_uv,
                    stride_uv,
                    width as i32,
                    height as i32,
                );
                true
            }
            Pixels::Yuy2 | Pixels::Uyvy => {
                // YUY2 is `Y0 U Y1 V` per pixel pair; UYVY is the same four bytes
                // rotated by one.
                let (luma_at, u_at, v_at) = match self.kind {
                    Pixels::Yuy2 => (0usize, 1usize, 3usize),
                    _ => (1usize, 0usize, 2usize),
                };
                let (dst_y, dst_uv) = dst.data_mut();
                packed_422_into(
                    src,
                    stride,
                    size,
                    (luma_at, u_at, v_at),
                    dst_y,
                    stride_y as usize,
                    dst_uv,
                    stride_uv as usize,
                )
            }
            Pixels::Rgb32 => {
                let (dst_y, dst_uv) = dst.data_mut();
                // libyuv's "ARGB" is B, G, R, A in memory, which is exactly what
                // Windows calls RGB32.
                yuv_helper::argb_to_nv12(
                    &src[..stride * rows],
                    stride as u32,
                    dst_y,
                    stride_y,
                    dst_uv,
                    stride_uv,
                    width as i32,
                    signed_height,
                );
                true
            }
            Pixels::Rgb24 => {
                let widened = stride_for_rgb32(width);
                self.scratch.resize(widened * rows, 0);
                for row in 0..rows {
                    let from = &src[row * stride..row * stride + width as usize * 3];
                    let into = &mut self.scratch[row * widened..row * widened + width as usize * 4];
                    let (pixels, _) = into.as_chunks_mut::<4>();
                    let (triples, _) = from.as_chunks::<3>();
                    for (pixel, bgr) in pixels.iter_mut().zip(triples) {
                        pixel[0] = bgr[0];
                        pixel[1] = bgr[1];
                        pixel[2] = bgr[2];
                        pixel[3] = 0xFF;
                    }
                }
                let (dst_y, dst_uv) = dst.data_mut();
                yuv_helper::argb_to_nv12(
                    &self.scratch,
                    widened as u32,
                    dst_y,
                    stride_y,
                    dst_uv,
                    stride_uv,
                    width as i32,
                    signed_height,
                );
                true
            }
        }
    }
}

fn stride_for_rgb32(width: u32) -> usize {
    width as usize * 4
}

/// Row-by-row copy of an NV12 frame with `pitch` into libwebrtc's buffer.
///
/// The source pitch is rarely the width: drivers align rows, and a 1280-wide
/// frame routinely arrives with a 1536-byte stride. Copying it as if it were
/// packed shears the image diagonally — which looks like a broken encoder and
/// is not one.
pub(crate) fn nv12_into(
    source: &[u8],
    pitch: usize,
    size: Size,
    dst_y: &mut [u8],
    stride_y: usize,
    dst_uv: &mut [u8],
    stride_uv: usize,
) -> bool {
    let rows = size.height as usize;
    let width = size.width as usize;
    let chroma_rows = rows / 2;
    if pitch < width || source.len() < pitch * rows + pitch * chroma_rows {
        return false;
    }
    if stride_y < width || stride_uv < width {
        return false;
    }
    if dst_y.len() < stride_y * rows || dst_uv.len() < stride_uv * chroma_rows {
        return false;
    }

    for row in 0..rows {
        let from = &source[row * pitch..row * pitch + width];
        dst_y[row * stride_y..row * stride_y + width].copy_from_slice(from);
    }
    let uv = pitch * rows;
    for row in 0..chroma_rows {
        let from = &source[uv + row * pitch..uv + row * pitch + width];
        dst_uv[row * stride_uv..row * stride_uv + width].copy_from_slice(from);
    }
    true
}

/// Packed 4:2:2 (YUY2, UYVY) into NV12.
///
/// `offsets` says where luma, U and V sit inside each four-byte pixel pair.
/// Chroma is taken from the even rows and the odd ones are dropped, rather than
/// averaged: the picture is about to be scaled and encoded, and the difference
/// does not survive either.
#[allow(clippy::too_many_arguments)]
fn packed_422_into(
    source: &[u8],
    pitch: usize,
    size: Size,
    offsets: (usize, usize, usize),
    dst_y: &mut [u8],
    stride_y: usize,
    dst_uv: &mut [u8],
    stride_uv: usize,
) -> bool {
    let rows = size.height as usize;
    let width = size.width as usize;
    let (luma_at, u_at, v_at) = offsets;
    if pitch < width * 2 || source.len() < pitch * rows {
        return false;
    }
    if stride_y < width || stride_uv < width {
        return false;
    }

    for row in 0..rows {
        let from = &source[row * pitch..row * pitch + width * 2];
        let into = &mut dst_y[row * stride_y..row * stride_y + width];
        for (pixel, byte) in into.iter_mut().enumerate() {
            *byte = from[pixel * 2 + luma_at];
        }
        if row % 2 != 0 {
            continue;
        }
        let into = &mut dst_uv[(row / 2) * stride_uv..(row / 2) * stride_uv + width];
        let (pairs, _) = into.as_chunks_mut::<2>();
        for (pair, chroma) in pairs.iter_mut().enumerate() {
            chroma[0] = from[pair * 4 + u_at];
            chroma[1] = from[pair * 4 + v_at];
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const TINY: Size = Size {
        width: 4,
        height: 2,
    };

    fn buffer() -> NV12Buffer {
        NV12Buffer::new(TINY.width, TINY.height)
    }

    /// Um pitch maior que a largura é o caso comum, não a exceção: tratá-lo como
    /// colado inclina a imagem e parece defeito de encoder.
    #[test]
    fn a_padded_pitch_is_copied_without_shearing() {
        let pitch = 8;
        let mut source = vec![0u8; pitch * 3];
        for row in 0..2 {
            for column in 0..4 {
                source[row * pitch + column] = (row * 4 + column) as u8;
            }
        }
        for column in 0..4 {
            source[pitch * 2 + column] = 200 + column as u8;
        }

        let mut dst = buffer();
        let (stride_y, stride_uv) = dst.strides();
        let (y, uv) = dst.data_mut();
        assert!(nv12_into(
            &source,
            pitch,
            TINY,
            y,
            stride_y as usize,
            uv,
            stride_uv as usize
        ));

        assert_eq!(&y[0..4], &[0, 1, 2, 3]);
        assert_eq!(
            &y[stride_y as usize..stride_y as usize + 4],
            &[4, 5, 6, 7],
            "a segunda linha nao pode vir deslocada pelo padding"
        );
        assert_eq!(&uv[0..4], &[200, 201, 202, 203]);
    }

    #[test]
    fn a_short_frame_is_refused_instead_of_read_past_the_end() {
        let mut dst = buffer();
        let (stride_y, stride_uv) = dst.strides();
        let (y, uv) = dst.data_mut();
        assert!(!nv12_into(
            &[0u8; 8],
            8,
            TINY,
            y,
            stride_y as usize,
            uv,
            stride_uv as usize
        ));
    }

    /// YUY2 e UYVY são os mesmos quatro bytes em ordem diferente. Trocar os dois
    /// dá uma imagem em que a luminância vem do croma: reconhecível, e horrível.
    #[test]
    fn yuy2_takes_luma_from_the_even_bytes() {
        // Dois pares de pixels por linha: Y0 U Y1 V.
        let source: Vec<u8> = vec![
            10, 100, 11, 200, 12, 101, 13, 201, // linha 0
            20, 150, 21, 250, 22, 151, 23, 251, // linha 1
        ];
        let mut dst = buffer();
        assert!(Converter::new(Pixels::Yuy2).write(&source, TINY, &mut dst));

        let (stride_y, _) = dst.strides();
        let (y, uv) = dst.data_mut();
        assert_eq!(&y[0..4], &[10, 11, 12, 13]);
        assert_eq!(
            &y[stride_y as usize..stride_y as usize + 4],
            &[20, 21, 22, 23]
        );
        assert_eq!(
            &uv[0..4],
            &[100, 200, 101, 201],
            "o croma sai da linha par, intercalado U V"
        );
    }

    #[test]
    fn uyvy_is_the_same_four_bytes_rotated_by_one() {
        let source: Vec<u8> = vec![
            100, 10, 200, 11, 101, 12, 201, 13, // linha 0
            150, 20, 250, 21, 151, 22, 251, 23, // linha 1
        ];
        let mut dst = buffer();
        assert!(Converter::new(Pixels::Uyvy).write(&source, TINY, &mut dst));

        let (y, uv) = dst.data_mut();
        assert_eq!(&y[0..4], &[10, 11, 12, 13]);
        assert_eq!(&uv[0..4], &[100, 200, 101, 201]);
    }

    /// Um branco puro em RGB tem que sair branco em YUV. Se a ordem dos canais
    /// estiver trocada, um vermelho puro vira azul e ninguém nota no branco —
    /// por isso o teste usa vermelho.
    #[test]
    fn rgb32_keeps_red_red() {
        // B, G, R, X — vermelho puro.
        let source: Vec<u8> = std::iter::repeat_n([0u8, 0, 255, 255], 8)
            .flatten()
            .collect();
        let mut dst = buffer();
        assert!(Converter::new(Pixels::Rgb32).write(&source, TINY, &mut dst));

        let (y, uv) = dst.data_mut();
        // Vermelho BT.601: Y≈81, U≈90, V≈240. Margem larga de propósito: o que
        // se prova aqui é a ordem dos canais, não o arredondamento do libyuv.
        assert!(
            (70..95).contains(&y[0]),
            "luma de vermelho fora da faixa: {}",
            y[0]
        );
        assert!(uv[1] > uv[0], "V tem que dominar U num vermelho");
    }

    /// RGB24 tem linhas preenchidas até um múltiplo de 4 bytes. Ignorar isso
    /// desloca cada linha e inclina a imagem.
    #[test]
    fn rgb24_rows_are_padded_to_four_bytes() {
        assert_eq!(Pixels::Rgb24.stride(4), 12, "4 px * 3 B ja e multiplo de 4");
        assert_eq!(Pixels::Rgb24.stride(2), 8, "2 px * 3 B = 6, sobe para 8");
        assert_eq!(Pixels::Rgb32.stride(3), 12);
        assert_eq!(Pixels::Yuy2.stride(4), 8);
    }

    #[test]
    fn rgb24_converts_with_its_padding() {
        let stride = Pixels::Rgb24.stride(TINY.width);
        let mut source = vec![0u8; stride * TINY.height as usize];
        for row in 0..TINY.height as usize {
            for column in 0..TINY.width as usize {
                let at = row * stride + column * 3;
                source[at] = 0;
                source[at + 1] = 0;
                source[at + 2] = 255;
            }
        }
        let mut dst = buffer();
        assert!(Converter::new(Pixels::Rgb24).write(&source, TINY, &mut dst));
        let (y, _) = dst.data_mut();
        assert!((70..95).contains(&y[0]));
    }

    #[test]
    fn an_odd_size_is_refused_instead_of_producing_half_a_chroma_row() {
        let odd = Size {
            width: 5,
            height: 3,
        };
        let mut dst = NV12Buffer::new(6, 4);
        assert!(!Converter::new(Pixels::Yuy2).write(&[0u8; 256], odd, &mut dst));
    }

    #[test]
    fn i420_is_recognised_under_both_of_its_guids() {
        assert_eq!(Pixels::from_subtype(&MEDIASUBTYPE_I420), Some(Pixels::I420));
        assert_eq!(Pixels::from_subtype(&MEDIASUBTYPE_IYUV), Some(Pixels::I420));
        assert_eq!(
            Pixels::from_subtype(&windows::Win32::Media::MediaFoundation::MEDIASUBTYPE_MJPG),
            None,
            "comprimido nao entra (ADR-0039)"
        );
    }

    #[test]
    fn nv12_frame_bytes_count_the_chroma_plane() {
        let size = Size {
            width: 1280,
            height: 720,
        };
        assert_eq!(Pixels::Nv12.frame_bytes(size), 1280 * 720 * 3 / 2);
        assert_eq!(Pixels::Yuy2.frame_bytes(size), 1280 * 720 * 2);
        assert_eq!(Pixels::Rgb32.frame_bytes(size), 1280 * 720 * 4);
    }
}
