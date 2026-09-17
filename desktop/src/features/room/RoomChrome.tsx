import { useEffect, useRef } from 'react';
import { media } from '../../app/runtime';
import { PUBLISH_PRESETS, SELF_ID, useMediaStore, visibleTiles } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { useUiStore } from '../../store/ui';
import { Avatar } from '../../ui/Avatar';
import { Button } from '../../ui/Button';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { MenuItem, MenuLabel, Popover } from '../../ui/Popover';
import { kbps } from './format';

interface RoomChromeProps {
  onShare: () => void;
  onStop: () => void;
  onToggleFullscreen: () => void;
  fullscreen: boolean;
}

/**
 * O cromo sobre o vídeo: um cabeçalho em gradiente e uma **pílula flutuante** de
 * controles, centralizada acima da borda inferior.
 *
 * Não é estilo. Eram duas barras de ponta a ponta — uma da sala, outra por
 * ladrilho — disputando a mesma faixa da janela, e o ladrilho em foco (`z-10`)
 * ainda passava por cima da barra da sala, deixando "Parar de compartilhar"
 * inalcançável justamente no modo em que mais se precisa dele. Com um cromo só,
 * acima de tudo, o conflito deixa de existir e a largura inteira da base volta
 * para o vídeo (CLAUDE.md §8).
 *
 * Controles por tela — volume, qualidade, destacar — continuam em cada ladrilho:
 * com várias telas não existe "a" tela para um controle global agir sobre
 * (RF-31).
 */
export function RoomChrome({ onShare, onStop, onToggleFullscreen, fullscreen }: RoomChromeProps) {
  const channelName = useRoomStore((state) => state.channelName);
  const publishing = useMediaStore((state) => state.publishing);
  const starting = useMediaStore((state) => state.starting);
  const connection = useMediaStore((state) => state.connection);
  const screenOrder = useMediaStore((state) => state.screenOrder);
  const viewers = useMediaStore((state) => state.viewerIds.length);
  const stats = useMediaStore((state) => state.stats);
  const preset = useMediaStore((state) => state.publishPreset);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const focused = useMediaStore((state) => state.focused);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const setShowSelfPreview = useUiStore((state) => state.setShowSelfPreview);
  const holdChrome = useUiStore((state) => state.holdChrome);
  const release = useRef<(() => void) | null>(null);

  // Enquanto o ponteiro estiver sobre os controles, o temporizador de
  // ociosidade não pode escondê-los: mirar num botão e parar de mexer o mouse
  // fazia a barra sumir debaixo do cursor.
  useEffect(() => {
    return () => {
      release.current?.();
      release.current = null;
    };
  }, []);

  const tiles = visibleTiles(screenOrder, publishing, showSelfPreview);

  return (
    <>
      <header className="pointer-events-none absolute inset-x-0 top-0 flex items-start gap-2 bg-linear-to-b from-scrim to-transparent px-3 pb-8 pt-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-text">{channelName ?? 'Canal de voz'}</span>
          <span className="shrink-0 text-text-muted">
            {tiles.length === 1 ? '1 tela' : `${tiles.length} telas`}
          </span>
          {connection !== 'connected' && (
            <span className="shrink-0 text-warning">{connectionLabel(connection)}</span>
          )}
        </div>

        {publishing && (
          <div className="ml-auto flex shrink-0 items-center gap-2">
            <span className="flex items-center gap-1 rounded-pill bg-surface-1/80 px-2 py-0.5 text-xs font-medium text-danger">
              <Icon name="dot" size={10} />
              NO AR
            </span>
            {stats !== null && (
              <span className="rounded-pill bg-surface-1/80 px-2 py-0.5 font-mono text-xs text-text-muted">
                {stats.width}×{stats.height} · {stats.fps} fps · {kbps(stats.bitrateKbps)} ·{' '}
                {viewers === 1 ? '1 assistindo' : `${viewers} assistindo`}
              </span>
            )}
          </div>
        )}
      </header>

      <footer className="pointer-events-none absolute inset-x-0 bottom-0 flex justify-center px-3 pb-3 pt-10">
        <div
          className="pointer-events-auto flex items-center gap-1 rounded-pill border border-line-soft bg-chrome p-1"
          onPointerEnter={() => {
            release.current?.();
            release.current = holdChrome();
          }}
          onPointerLeave={() => {
            release.current?.();
            release.current = null;
          }}
        >
          {publishing ? (
            <>
              <Button variant="danger" icon="stop" onClick={onStop} className="rounded-pill px-3">
                Parar
              </Button>
              <Popover icon="gear" label="Ajustes da sua transmissão">
                {(close) => (
                  <>
                    {sharingTitle !== null && (
                      <>
                        <MenuLabel>Enviando</MenuLabel>
                        <p className="truncate px-2 pb-1 text-text">{sharingTitle}</p>
                      </>
                    )}
                    <MenuLabel>Qualidade que você envia</MenuLabel>
                    {PUBLISH_PRESETS.map((option) => (
                      <MenuItem
                        key={option}
                        selected={option === preset}
                        hint={option === preset ? undefined : 'reinicia'}
                        onClick={() => {
                          void media.changePreset(option);
                          close();
                        }}
                      >
                        {option}
                      </MenuItem>
                    ))}
                    <MenuLabel>Sua tela</MenuLabel>
                    <MenuItem
                      selected={showSelfPreview}
                      onClick={() => {
                        setShowSelfPreview(!showSelfPreview);
                        close();
                      }}
                    >
                      Ver a minha própria tela
                    </MenuItem>
                  </>
                )}
              </Popover>
            </>
          ) : (
            <Button
              variant="primary"
              icon="monitor"
              onClick={onShare}
              disabled={starting}
              className="rounded-pill px-3"
            >
              {starting ? 'Conectando…' : 'Compartilhar tela'}
            </Button>
          )}

          {focused !== null && tiles.length > 1 && (
            <FocusSwitcher tiles={tiles} focused={focused} />
          )}

          <IconButton
            icon={fullscreen ? 'exit-fullscreen' : 'fullscreen'}
            label={fullscreen ? 'Sair da tela cheia (Esc)' : 'Tela cheia (F)'}
            aria-pressed={fullscreen}
            onClick={onToggleFullscreen}
          />
        </div>
      </footer>
    </>
  );
}

/**
 * Trocar de tela sem sair do foco, por **avatar** (ADR-0031).
 *
 * O Discord faz isto com uma faixa de miniaturas ao vivo. Miniatura ao vivo é
 * assinatura ao vivo: com N telas, o egress multiplica por N enquanto a janela
 * estiver aberta (RF-32). O avatar é um PNG que a lista de participantes já
 * carregou, e resolve a mesma navegação por zero.
 */
function FocusSwitcher({ tiles, focused }: { tiles: string[]; focused: string }) {
  const participants = useRoomStore((state) => state.participants);

  return (
    <div className="mx-1 flex items-center gap-1 border-l border-line-soft pl-2">
      {tiles.map((identity) => {
        const participant = participants[identity];
        const name =
          identity === SELF_ID
            ? 'Sua tela'
            : (participant?.user.display_name ?? participant?.user.username ?? 'Alguém');
        const active = identity === focused;
        return (
          <button
            key={identity}
            type="button"
            aria-label={`Ver ${name}`}
            aria-pressed={active}
            title={name}
            onClick={() => {
              useMediaStore.getState().focus(identity);
            }}
            className={`rounded-pill p-0.5 ${active ? 'bg-accent' : 'hover:bg-surface-3'}`}
          >
            {identity === SELF_ID ? (
              <span className="flex h-6 w-6 items-center justify-center rounded-pill bg-surface-3 text-danger">
                <Icon name="dot" size={12} />
              </span>
            ) : (
              <Avatar url={participant?.user.avatar_url ?? null} name={name} size={24} />
            )}
          </button>
        );
      })}
    </div>
  );
}

function connectionLabel(state: string): string {
  switch (state) {
    case 'connecting':
      return 'conectando';
    case 'reconnecting':
      return 'reconectando';
    case 'failed':
      return 'sem conexão de mídia';
    default:
      return state;
  }
}
