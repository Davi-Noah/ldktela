import { useState } from 'react';
import type { RoomParticipant } from '../../api/types/RoomParticipant';
import type { CameraDevice } from '../../media/native';
import { media } from '../../app/runtime';
import {
  isSelfPublication,
  leftPublicationIds,
  ownerOf,
  PUBLISH_PRESETS,
  useMediaStore,
  visibleTiles,
} from '../../store/media';
import {
  ownerOfPublication,
  publicationId,
  sourceLabel,
  sourceOfPublication,
} from '../../media/publication';
import { publicationsOf, useRoomStore } from '../../store/room';
import { useSessionStore } from '../../store/session';
import { useUiStore } from '../../store/ui';
import { Avatar } from '../../ui/Avatar';
import { Button } from '../../ui/Button';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { MenuItem, MenuLabel, Popover } from '../../ui/Popover';
import { kbps } from './format';
import { WatchButton } from './WatchButton';

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
  const publicationOrder = useMediaStore((state) => state.publicationOrder);
  const publications = useMediaStore((state) => state.publications);
  const cameraOn = useMediaStore((state) => state.camera.publishing);
  const solo = useMediaStore((state) => state.solo);
  const viewers = useMediaStore((state) => state.viewerIds.length);
  const stats = useMediaStore((state) => state.stats);
  const preset = useMediaStore((state) => state.publishPreset);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const focused = useMediaStore((state) => state.focused);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const setShowSelfPreview = useUiStore((state) => state.setShowSelfPreview);

  const tiles = visibleTiles(
    publicationOrder,
    publications,
    { screen: publishing, camera: cameraOn },
    showSelfPreview,
  );
  const left = leftPublicationIds({ publicationOrder, publications });

  return (
    <>
      <header className="pointer-events-none absolute inset-x-0 top-0 flex items-start gap-2 bg-linear-to-b from-scrim to-transparent px-3 pb-8 pt-2">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate font-medium text-text">{channelName ?? 'Canal de voz'}</span>
          <span className="shrink-0 text-text-muted">
            {tiles.length === 1 ? '1 transmissão' : `${tiles.length} transmissões`}
            {/* Sair de uma tela tira o ladrilho do layout (ADR-0036). Sem esta
                contagem, a transmissão de alguém simplesmente não estaria lá, e
                procurar um defeito é a primeira reação a isso. */}
            {left.length > 0 && <span className="text-text-faint"> · {left.length} fora</span>}
          </span>
          {connection !== 'connected' && (
            <span className="shrink-0 text-warning">{connectionLabel(connection)}</span>
          )}
        </div>

        {(publishing || cameraOn) && (
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
          // `relative`: é esta pílula que o painel de pessoas usa para se
          // centralizar, e é ela que muda de largura quando a transmissão
          // começa ou termina (Popover, `align="bar"`).
          className="pointer-events-auto relative flex items-center gap-1 rounded-pill border border-line-soft bg-chrome p-1"
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

          {/* A câmera é independente da tela (ADR-0038): fica ao lado, e não
              dentro dos ajustes da transmissão, porque ligá-la não exige estar
              compartilhando nada. */}
          <CameraButton />

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
      {tiles.map((id) => {
        const mine = isSelfPublication(id);
        const source = sourceOfPublication(id);
        const participant = participants[ownerOfPublication(id)];
        const person = participant?.user.display_name ?? participant?.user.username ?? 'Alguém';
        // O rótulo diz a fonte porque a mesma pessoa pode aparecer duas vezes
        // aqui, e dois avatares iguais lado a lado não se distinguem (ADR-0038).
        const name = mine
          ? source === 'camera'
            ? 'Sua câmera'
            : 'Sua tela'
          : `${sourceLabel(source)} de ${person}`;
        const active = id === focused;
        return (
          <button
            key={id}
            type="button"
            aria-label={`Ver ${name}`}
            aria-pressed={active}
            title={name}
            onClick={() => {
              useMediaStore.getState().focus(id);
            }}
            className={`relative rounded-pill p-0.5 ${active ? 'bg-accent' : 'hover:bg-surface-3'}`}
          >
            {mine ? (
              <span className="flex h-6 w-6 items-center justify-center rounded-pill bg-surface-3 text-danger">
                <Icon name={source === 'camera' ? 'camera' : 'dot'} size={12} />
              </span>
            ) : (
              <Avatar url={participant?.user.avatar_url ?? null} name={person} size={24} />
            )}
            {/* Um selo de câmera sobre o avatar: sem ele, a tela e a câmera da
                mesma pessoa são dois botões idênticos. */}
            {!mine && source === 'camera' && (
              <span className="absolute -bottom-0.5 -right-0.5 flex h-3.5 w-3.5 items-center justify-center rounded-pill bg-surface-1 text-text">
                <Icon name="camera" size={9} />
              </span>
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
 * Liga, desliga e troca a câmera (ADR-0038).
 *
 * Um botão com estado, e não um item de menu: a câmera entra e sai o tempo todo
 * numa conversa, e enterrá-la atrás de um popover cobraria dois cliques por vez.
 * A escolha de dispositivo é que fica no menu ao lado, porque quase ninguém tem
 * duas câmeras e quem tem escolhe uma vez.
 *
 * A lista é buscada ao abrir o menu, e não na montagem: enumerar câmeras abre o
 * Media Foundation, e pagar isso em toda sala que se entra seria gastar por um
 * recurso que a maioria não usa.
 */
function CameraButton() {
  const camera = useMediaStore((state) => state.camera);
  const [devices, setDevices] = useState<CameraDevice[] | null>(null);

  const load = () => {
    void media
      .listCameras()
      .then(setDevices)
      .catch(() => {
        setDevices([]);
      });
  };

  if (camera.publishing) {
    return (
      <>
        <IconButton
          icon="camera"
          label={`Desligar a câmera${camera.deviceName === null ? '' : ` (${camera.deviceName})`}`}
          aria-pressed
          onClick={() => {
            void media.stopCamera();
          }}
        />
        <Popover icon="chevron" label="Escolher outra câmera" onOpen={load}>
          {(close) => (
            <CameraList
              devices={devices}
              selected={camera.deviceId}
              onPick={(device) => {
                void media.switchCamera(device);
                close();
              }}
            />
          )}
        </Popover>
      </>
    );
  }

  return (
    <Popover icon="camera" label="Ligar a câmera" onOpen={load} disabled={camera.starting}>
      {(close) => (
        <CameraList
          devices={devices}
          selected={null}
          onPick={(device) => {
            void media.startCamera(device);
            close();
          }}
        />
      )}
    </Popover>
  );
}

function CameraList({
  devices,
  selected,
  onPick,
}: {
  devices: CameraDevice[] | null;
  selected: string | null;
  onPick: (device: CameraDevice) => void;
}) {
  if (devices === null) {
    return <p className="px-2 py-1 text-text-muted">Procurando câmeras…</p>;
  }
  if (devices.length === 0) {
    // Dito uma vez e por extenso: "nenhuma câmera" sem explicação manda a
    // pessoa procurar defeito no aplicativo, e o motivo costuma ser o Windows.
    return (
      <p className="px-2 py-1 text-text-muted">
        Nenhuma câmera encontrada. Verifique se ela está conectada e se o Windows permite o acesso.
      </p>
    );
  }
  return (
    <>
      <MenuLabel>Câmera</MenuLabel>
      {devices.map((device) => (
        <MenuItem
          key={device.id}
          selected={device.id === selected}
          onClick={() => {
            onPick(device);
          }}
        >
          {device.name}
        </MenuItem>
      ))}
    </>
  );
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
  const cameraOn = useMediaStore((state) => state.camera.publishing);
  const viewerIds = useMediaStore((state) => state.viewerIds);
  const publications = useMediaStore((state) => state.publications);
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
      align="bar"
    >
      {() => (
        <>
          <MenuLabel centered>Na sala</MenuLabel>
          <ul className="max-h-56 overflow-y-auto">
            {participantIds.map((id) => {
              const participant = participants[id];
              if (participant === undefined) {
                return null;
              }
              const live = publicationsOf(participant);
              return (
                <li key={id} className="px-2 py-1">
                  <div className="flex items-center gap-2">
                    <Avatar
                      url={participant.user.avatar_url}
                      name={participant.user.username}
                      size={20}
                    />
                    <span className="min-w-0 flex-1 truncate text-text">
                      {nameOf(participants, id)}
                      {id === me && <span className="text-text-faint"> · você</span>}
                    </span>
                    {live.length > 0 && publicationsWithTrack(live, id, publications) === 0 && (
                      // Transmitindo, mas a trilha ainda não chegou: não há do
                      // que sair, e um botão ali prometeria uma ação que não
                      // acontece.
                      <span className="flex shrink-0 items-center gap-1 text-danger">
                        <Icon name="dot" size={10} />
                        no ar
                      </span>
                    )}
                  </div>

                  {/* Uma linha por publicação (ADR-0038). Botão com texto, e
                      não com dica: a dica de um botão de ícone é um bloco
                      posicionado, e dentro desta lista, que rola, ela empurrava
                      a largura do menu e ganhava uma barra de rolagem
                      horizontal — foi assim que o menu apareceu torto. */}
                  {live.map((publication) => {
                    const key = publicationId(id, publication.source);
                    const state = publications[key];
                    if (state === undefined) {
                      return null;
                    }
                    return (
                      <div key={key} className="mt-0.5 flex items-center gap-2 pl-7">
                        <span className="min-w-0 flex-1 truncate text-text-muted">
                          {sourceLabel(publication.source)}
                        </span>
                        <WatchButton
                          watching={state.subscribed}
                          onClick={() => {
                            media.setPublicationSubscribed(key, !state.subscribed);
                          }}
                        />
                      </div>
                    );
                  })}
                </li>
              );
            })}
          </ul>

          {(publishing || cameraOn) && (
            <>
              <MenuLabel centered>Vendo você</MenuLabel>
              {watching.length === 0 ? (
                <p className="px-2 pb-1 text-center text-text-muted">Ninguém ainda.</p>
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
          <p className="px-2 pb-1 pt-1.5 text-center text-xs text-text-faint">
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

/** Quantas publicações desta pessoa já têm trilha chegando aqui. */
function publicationsWithTrack(
  live: readonly { source: 'screen' | 'camera' }[],
  ownerId: string,
  publications: Record<string, unknown>,
): number {
  return live.filter((publication) => publications[publicationId(ownerId, publication.source)])
    .length;
}
