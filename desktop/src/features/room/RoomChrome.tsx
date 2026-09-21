import type { RoomParticipant } from '../../api/types/RoomParticipant';
import { media } from '../../app/runtime';
import {
  leftScreenIds,
  ownerOf,
  PUBLISH_PRESETS,
  SELF_ID,
  useMediaStore,
  visibleTiles,
} from '../../store/media';
import { useRoomStore } from '../../store/room';
import { useSessionStore } from '../../store/session';
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
  const screens = useMediaStore((state) => state.screens);
  const solo = useMediaStore((state) => state.solo);
  const viewers = useMediaStore((state) => state.viewerIds.length);
  const stats = useMediaStore((state) => state.stats);
  const preset = useMediaStore((state) => state.publishPreset);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const focused = useMediaStore((state) => state.focused);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const setShowSelfPreview = useUiStore((state) => state.setShowSelfPreview);

  const tiles = visibleTiles(screenOrder, screens, publishing, showSelfPreview);
  const left = leftScreenIds({ screenOrder, screens });

  return (
    <>
      <header className="pointer-events-none absolute inset-x-0 top-0 flex items-start gap-2 bg-linear-to-b from-scrim to-transparent px-3 pb-8 pt-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-text">{channelName ?? 'Canal de voz'}</span>
          <span className="shrink-0 text-text-muted">
            {tiles.length === 1 ? '1 tela' : `${tiles.length} telas`}
            {/* Sair de uma tela tira o ladrilho do layout (ADR-0036). Sem esta
                contagem, a transmissão de alguém simplesmente não estaria lá, e
                procurar um defeito é a primeira reação a isso. */}
            {left.length > 0 && <span className="text-text-faint"> · {left.length} fora</span>}
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
          // Enquanto o ponteiro estiver aqui, o temporizador de ociosidade não
          // esconde o cromo: mirar num botão e parar de mexer o mouse fazia a
          // barra sumir debaixo do cursor. Quem lê este atributo é o `:hover` em
          // `RoomScreen`, e não um contador — ver o comentário de lá.
          data-chrome-hold
          className="pointer-events-auto flex items-center gap-1 rounded-pill border border-line-soft bg-chrome p-1"
        >
          {publishing ? (
            <>
              <Button variant="danger" icon="stop" onClick={onStop} className="rounded-pill px-3">
                Parar
              </Button>
              {/* Trocar de tela, de janela ou o áudio sem parar antes (issue
                  #9). Por baixo a trilha é substituída, como na troca de
                  qualidade; por isso a dica avisa que a imagem reinicia. */}
              <IconButton
                icon="monitor"
                label="Trocar o que você envia (reinicia)"
                onClick={onShare}
                disabled={starting}
              />
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
            <>
              {/* Alternador, e não duas ações: o ícone é sempre o do arranjo
                  exclusivo e o estado vive no `aria-pressed`. Trocar o desenho
                  junto com o estado diria "ligado" mostrando o contrário. */}
              <IconButton
                icon="layout-solo"
                label={solo ? 'Mostrar as outras telas ao lado' : 'Ver só esta tela'}
                aria-pressed={solo}
                onClick={() => {
                  useMediaStore.getState().setSolo(!solo);
                }}
              />
              <FocusSwitcher tiles={tiles} focused={focused} />
            </>
          )}

          <People />

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

/**
 * Quem está na sala, e quem está vendo a sua tela (issue #10).
 *
 * Com alguém transmitindo, o corpo da sala dá lugar ao vídeo e a lista de
 * pessoas some junto — some justamente quando ela importa, porque é aí que se
 * quer saber quem chegou, quem saiu e quem está do outro lado da sua tela.
 *
 * Fica num popover, e não fixa no cromo: painel permanente disputando espaço com
 * a imagem é exatamente o que a direção visual proíbe (CLAUDE.md §8).
 */
function People() {
  const participantIds = useRoomStore((state) => state.participantIds);
  const participants = useRoomStore((state) => state.participants);
  const publishing = useMediaStore((state) => state.publishing);
  const viewerIds = useMediaStore((state) => state.viewerIds);
  const screens = useMediaStore((state) => state.screens);
  const me = useSessionStore((state) => state.user?.discord_user_id ?? null);

  // As conexões de quem publica já foram descartadas na origem; o `ownerOf`
  // aqui é o que sobra de um `~pub` que tenha escapado, e o `Set` evita alguém
  // aparecer duas vezes por ter duas conexões (ADR-0027).
  const watching = [...new Set(viewerIds.map(ownerOf))];

  return (
    <Popover
      icon="people"
      label={`Quem está aqui (${participantIds.length})`}
      role="group"
      width="w-72"
    >
      {() => (
        <>
          <MenuLabel>Na sala</MenuLabel>
          <ul className="max-h-56 overflow-y-auto">
            {participantIds.map((id) => {
              const participant = participants[id];
              if (participant === undefined) {
                return null;
              }
              return (
                <li key={id} className="flex items-center gap-2 px-2 py-1">
                  <Avatar
                    url={participant.user.avatar_url}
                    name={participant.user.username}
                    size={20}
                  />
                  <span className="min-w-0 flex-1 truncate text-text">
                    {nameOf(participants, id)}
                    {id === me && <span className="text-text-faint"> · você</span>}
                  </span>
                  {participant.publishing && (
                    // Botão com texto, e não com dica: a dica de um botão de
                    // ícone é um bloco posicionado, e dentro desta lista, que
                    // rola, ela empurrava a largura do menu e ganhava uma barra
                    // de rolagem horizontal — foi assim que o menu apareceu
                    // torto e cortando o conteúdo.
                    <WatchButton
                      watching={screens[id]?.subscribed !== false}
                      known={screens[id] !== undefined}
                      onClick={() => {
                        media.setScreenSubscribed(id, screens[id]?.subscribed === false);
                      }}
                    />
                  )}
                </li>
              );
            })}
          </ul>

          {publishing && (
            <>
              <MenuLabel>Vendo a sua tela</MenuLabel>
              {watching.length === 0 ? (
                <p className="px-2 pb-1 text-text-muted">Ninguém ainda.</p>
              ) : (
                <ul aria-label="Vendo a sua tela" className="max-h-40 overflow-y-auto">
                  {watching.map((id) => (
                    <li key={id} className="flex items-center gap-2 px-2 py-1">
                      <Avatar
                        url={participants[id]?.user.avatar_url ?? null}
                        name={participants[id]?.user.username ?? '?'}
                        size={20}
                      />
                      <span className="truncate text-text">{nameOf(participants, id)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}

          {/* A sala é o canal de voz do Discord, mas esta lista vem da nossa
              presença: quem está no canal sem o ldktela aberto não aparece, e
              deixar isso implícito faria a lista parecer errada. */}
          <p className="max-w-56 px-2 pb-1 pt-1.5 text-xs text-text-faint">
            Quem está no canal de voz sem o ldktela aberto não aparece aqui.
          </p>
        </>
      )}
    </Popover>
  );
}

function nameOf(participants: Record<string, RoomParticipant>, id: string): string {
  const user = participants[id]?.user;
  return user?.display_name ?? user?.username ?? 'Alguém';
}

/**
 * Entrar ou sair da tela de alguém, do tamanho de um item de lista.
 *
 * `known` separa "está transmitindo e eu recebo" de "está transmitindo e a
 * trilha ainda não chegou": no segundo caso não há do que sair, e um botão ali
 * prometeria uma ação que não acontece.
 */
function WatchButton({
  watching,
  known,
  onClick,
}: {
  watching: boolean;
  known: boolean;
  onClick: () => void;
}) {
  if (watching && !known) {
    return (
      <span className="flex shrink-0 items-center gap-1 text-danger">
        <Icon name="dot" size={10} />
        no ar
      </span>
    );
  }
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex shrink-0 items-center gap-1 rounded-pill px-2 py-0.5 text-xs ${
        watching ? 'text-text-muted hover:text-text' : 'bg-accent-soft text-text'
      }`}
    >
      <Icon name={watching ? 'eye-off' : 'eye'} size={14} />
      {watching ? 'Sair' : 'Entrar'}
    </button>
  );
}
