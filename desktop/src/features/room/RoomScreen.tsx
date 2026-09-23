import { useCallback, useEffect, useRef, useState } from 'react';
import { media } from '../../app/runtime';
import { CHROME_IDLE_MS, PREVIEW_FOCUS_FPS, PREVIEW_GRID_FPS } from '../../config';
import type { ShareChoice } from '../../media/session';
import { type PublishPreset, selfPublication, useMediaStore } from '../../store/media';
import { useUiStore } from '../../store/ui';
import { RoomBody } from './RoomBody';
import { RoomChrome } from './RoomChrome';
import { ScreenGrid } from './ScreenGrid';
import { SharePicker } from './SharePicker';

export function RoomScreen() {
  const containerRef = useRef<HTMLDivElement>(null);
  const chromeRef = useRef<HTMLDivElement>(null);
  // Contam as telas **assistidas**: uma tela da qual se saiu não ocupa o palco
  // (ADR-0036), e com todas recusadas o que sobrava era um palco preto. Sem
  // vídeo, a sala volta a ser a lista de gente, que é onde se entra de novo.
  const screenCount = useMediaStore(
    (state) =>
      state.publicationOrder.filter((id) => state.publications[id]?.subscribed !== false).length,
  );
  const publishing = useMediaStore((state) => state.publishing);
  const cameraOn = useMediaStore((state) => state.camera.publishing);
  const focused = useMediaStore((state) => state.focused);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const [picker, setPicker] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [chromeVisible, setChromeVisible] = useState(true);
  // Mirrors the state so pointer moves do not touch React on every event.
  const chromeVisibleRef = useRef(true);

  const hasVideo = screenCount > 0 || ((publishing || cameraOn) && showSelfPreview);

  useEffect(() => {
    if (!hasVideo) {
      chromeVisibleRef.current = true;
      setChromeVisible(true);
      return;
    }
    const container = containerRef.current;
    if (container === null) {
      return;
    }
    let timer: ReturnType<typeof setTimeout> | null = null;
    const show = () => {
      if (!chromeVisibleRef.current) {
        chromeVisibleRef.current = true;
        setChromeVisible(true);
      }
    };
    const arm = () => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      timer = setTimeout(() => {
        // Nunca esconder por baixo do ponteiro nem debaixo de um menu aberto.
        // Sem isto, mirar num botão e parar de mexer o mouse por dois segundos e
        // meio fazia a barra sumir sob o cursor — e o menu de qualidade ficava
        // órfão sobre o vídeo.
        //
        // O ponteiro é consultado por `:hover`, e não por um contador que os
        // controles incrementam ao entrar e decrementam ao sair. O contador era
        // a razão de a barra **nunca** sumir: basta um `pointerleave` que não
        // acontece — e ele deixa de acontecer sempre que o elemento sob o cursor
        // é removido, desmontado ou trocado de lugar — para a conta ficar presa
        // acima de zero para o resto da sessão. `:hover` não tem como vazar,
        // porque não guarda estado nenhum.
        const hovered = document.querySelector('[data-chrome-hold]:hover') !== null;
        if (hovered || useUiStore.getState().chromeHolds > 0) {
          arm();
          return;
        }
        chromeVisibleRef.current = false;
        setChromeVisible(false);
      }, CHROME_IDLE_MS);
    };
    const onMove = () => {
      show();
      arm();
    };
    arm();
    container.addEventListener('pointermove', onMove);
    // Quem navega pelo teclado precisa do cromo de volta ao alcançar um botão.
    container.addEventListener('focusin', onMove);
    return () => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      container.removeEventListener('pointermove', onMove);
      container.removeEventListener('focusin', onMove);
    };
  }, [hasVideo]);

  /**
   * Escondido de verdade, e não só transparente: `inert` tira o conteúdo do
   * alcance do Tab e do leitor de tela. Com `opacity` sozinha, o usuário de
   * teclado tabularia para dentro de uma barra invisível.
   */
  useEffect(() => {
    const element = chromeRef.current;
    if (element !== null) {
      element.inert = !chromeVisible;
    }
  }, [chromeVisible]);

  useEffect(() => {
    const onChange = () => {
      setFullscreen(document.fullscreenElement !== null);
    };
    document.addEventListener('fullscreenchange', onChange);
    return () => {
      document.removeEventListener('fullscreenchange', onChange);
    };
  }, []);

  const onToggleFullscreen = useCallback(() => {
    if (document.fullscreenElement !== null) {
      void document.exitFullscreen();
      return;
    }
    void containerRef.current?.requestFullscreen();
  }, []);

  /**
   * O preview da própria tela tem dois regimes (ADR-0030): confirmação na
   * grade, imagem em foco. Desligar o ladrilho **para a captura do preview no
   * core** — não é o elemento que some.
   */
  useEffect(() => {
    // Um regime por fonte (ADR-0038): a tela pode estar em foco enquanto a
    // câmera é um ladrilho, e mandar o mesmo relógio para as duas gastaria
    // captura de preview em cheio onde ninguém está olhando.
    for (const [source, live] of [
      ['screen', publishing],
      ['camera', cameraOn],
    ] as const) {
      const enabled = live && showSelfPreview;
      // Sozinha na grade, a própria transmissão ocupa a janela inteira — o
      // mesmo espaço do foco. Tratá-la como ladrilho ali a deixava reduzida
      // numa área que não é de ladrilho (issue #2).
      const large =
        focused === selfPublication(source) ||
        (focused === null && screenCount === 0 && !(publishing && cameraOn));
      const fps = large ? PREVIEW_FOCUS_FPS : PREVIEW_GRID_FPS;
      void media.setPreview(source, enabled, fps, large);
    }
  }, [publishing, cameraOn, showSelfPreview, focused, screenCount]);

  /**
   * Teclado, como em qualquer reprodutor: `Esc` desfaz um nível por vez — sai da
   * tela cheia, depois sai do foco — e `F` alterna a tela cheia. Antes disso,
   * sair do foco exigia achar um botão que o próprio vídeo estava cobrindo.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) {
        return;
      }
      // Com o seletor aberto, o teclado é dele. O Escape já não chega aqui — o
      // diálogo o consome na captura —, mas o F chegaria, e alternar tela cheia
      // por baixo de um modal é o tipo de coisa que parece defeito.
      if (picker) {
        return;
      }
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      ) {
        return;
      }
      if (event.key === 'Escape') {
        if (document.fullscreenElement !== null) {
          return; // O próprio navegador sai da tela cheia com Escape.
        }
        if (useMediaStore.getState().focused !== null) {
          event.preventDefault();
          useMediaStore.getState().focus(null);
        }
        return;
      }
      if (event.key === 'f' || event.key === 'F') {
        event.preventDefault();
        onToggleFullscreen();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [onToggleFullscreen, picker]);

  const onShare = useCallback(() => {
    setPicker(true);
  }, []);

  const onConfirmShare = useCallback((choice: ShareChoice, preset: PublishPreset) => {
    setPicker(false);
    void media.switchShare(choice, preset);
  }, []);

  const loadSources = useCallback(() => media.listSources(), []);
  const loadThumbnail = useCallback(
    (kind: Parameters<typeof media.thumbnail>[0], id: string) => media.thumbnail(kind, id),
    [],
  );

  const onStop = useCallback(() => {
    void media.stopShare();
  }, []);

  return (
    <main
      ref={containerRef}
      // O ponteiro some junto com o cromo em cima do vídeo: ele é a única coisa
      // que sobra na frente da imagem depois que os controles saem.
      className={`relative h-full w-full overflow-hidden bg-surface-0 ${
        hasVideo && !chromeVisible ? 'cursor-gone' : ''
      }`}
    >
      {/* Mounted for the life of the screen. Only the wrapper's visibility changes,
          so the decoder survives every layout change. */}
      <div hidden={!hasVideo} className="screen-stage absolute inset-0 bg-stage">
        <ScreenGrid onFullscreen={onToggleFullscreen} />
      </div>

      {!hasVideo && (
        <div className="absolute inset-0">
          <RoomBody onShare={onShare} onStop={onStop} />
        </div>
      )}

      {/* Chrome exists only over a picture. With no video the body already carries
          the same controls, and a permanent bar would be chrome for its own sake.
          `z-30` põe o cromo acima do ladrilho em foco (`z-10`), que antes o
          cobria por inteiro. */}
      {hasVideo && (
        <div
          ref={chromeRef}
          // `pointer-events-none` no invólucro é obrigatório: ele cobre a área
          // inteira do vídeo, e sem isso nenhum clique chegaria ao ladrilho. Só
          // a pílula de controles volta a receber ponteiro.
          className={`chrome-fade pointer-events-none absolute inset-0 z-30 ${
            chromeVisible ? 'opacity-100' : 'opacity-0'
          }`}
        >
          <RoomChrome
            onShare={onShare}
            onStop={onStop}
            onToggleFullscreen={onToggleFullscreen}
            fullscreen={fullscreen}
          />
        </div>
      )}

      {picker && (
        <SharePicker
          loadSources={loadSources}
          loadThumbnail={loadThumbnail}
          onCancel={() => {
            setPicker(false);
          }}
          onConfirm={onConfirmShare}
        />
      )}
    </main>
  );
}
