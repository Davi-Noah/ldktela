import { media } from '../../app/runtime';
import { type MediaFault, useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { useSessionStore } from '../../store/session';
import { useUiStore } from '../../store/ui';
import { Avatar } from '../../ui/Avatar';
import { Button } from '../../ui/Button';
import { Icon } from '../../ui/Icon';
import { PublisherPanel } from './PublisherPanel';
import { WatchButton } from './WatchButton';

interface RoomBodyProps {
  onShare: () => void;
  onStop: () => void;
}

/**
 * What fills the window when there is no video: either "you are not in a voice
 * channel" or the people who are, plus the one button the product has.
 */
export function RoomBody({ onShare, onStop }: RoomBodyProps) {
  const channelId = useRoomStore((state) => state.channelId);
  return channelId === null ? <Idle /> : <InRoom onShare={onShare} onStop={onStop} />;
}

function Idle() {
  const reason = useRoomStore((state) => state.lastLeaveReason);
  const gateway = useSessionStore((state) => state.gateway);

  return (
    <div className="flex h-full flex-col items-center justify-center px-8 text-center">
      <p className="text-text">Nada acontecendo.</p>
      <p className="mt-1 max-w-md text-text-muted">
        Entre num canal de voz do Discord e este aplicativo entra na sala sozinho. A sala é o canal
        de voz — não há nada para escolher aqui.
      </p>
      {reason === 'access_revoked' && (
        <p className="mt-group text-warning">Você perdeu o acesso ao canal em que estava.</p>
      )}
      {reason === 'replica_stale' && (
        <p className="mt-group text-warning">
          O servidor perdeu contato com o Discord e encerrou a sala por precaução.
        </p>
      )}
      {gateway !== 'ready' && <p className="mt-group text-text-faint">Conectando ao servidor…</p>}
    </div>
  );
}

function InRoom({ onShare, onStop }: RoomBodyProps) {
  const channelName = useRoomStore((state) => state.channelName);
  const participantIds = useRoomStore((state) => state.participantIds);
  const participants = useRoomStore((state) => state.participants);
  const publishing = useMediaStore((state) => state.publishing);
  const starting = useMediaStore((state) => state.starting);
  const screens = useMediaStore((state) => state.screens);
  const fault = useMediaStore((state) => state.fault);
  const showSelfPreview = useUiStore((state) => state.showSelfPreview);
  const setShowSelfPreview = useUiStore((state) => state.setShowSelfPreview);

  return (
    <div className="mx-auto flex h-full w-full max-w-md flex-col justify-center px-8">
      <p className="text-text-faint">Na sala</p>
      <h1 className="text-lg font-semibold text-text">{channelName ?? 'Canal de voz'}</h1>

      {fault !== null && <Fault fault={fault} />}

      <ul className="mt-group space-y-row">
        {participantIds.map((id) => {
          const participant = participants[id];
          if (participant === undefined) {
            return null;
          }
          return (
            <li key={id} className="flex items-center gap-2">
              <Avatar url={participant.user.avatar_url} name={participant.user.username} />
              <span className="min-w-0 truncate text-text">
                {participant.user.display_name ?? participant.user.username}
              </span>
              {participant.publishing && (
                // A etiqueta acompanha o nome, e não o botão: ela diz o que a
                // pessoa está fazendo, e o botão é o que se pode fazer com isso.
                // Lado a lado à direita, as duas coisas competiam.
                <span className="flex shrink-0 items-center gap-1 rounded-pill bg-danger-soft px-1.5 py-px text-[0.6875rem] font-semibold tracking-wide text-danger">
                  <Icon name="dot" size={8} />
                  AO VIVO
                </span>
              )}
              {/* Saindo de todas as telas, a sala volta a esta lista — que passa a
                  ser o único caminho de volta (ADR-0036). Só aparece onde há uma
                  tela a que se possa entrar: quem transmite mas cuja trilha ainda
                  não chegou não tem o que oferecer. */}
              {participant.publishing && screens[id] !== undefined && (
                <span className="ml-auto">
                  <WatchButton
                    size="md"
                    watching={screens[id].subscribed}
                    onClick={() => {
                      media.setScreenSubscribed(id, !screens[id]?.subscribed);
                    }}
                  />
                </span>
              )}
            </li>
          );
        })}
      </ul>

      {fault !== null ? null : publishing ? (
        <div className="mt-group">
          <PublisherPanel />
          {/* Este corpo só aparece quando a própria tela está oculta — com ela
              visível, a sala está no modo grade e quem manda é o cromo. Por isso
              o caminho de volta mora aqui. */}
          {!showSelfPreview && (
            <Button
              icon="eye"
              onClick={() => {
                setShowSelfPreview(true);
              }}
              className="mt-group w-full py-2"
            >
              Ver a minha própria tela
            </Button>
          )}
          <Button variant="danger" icon="stop" onClick={onStop} className="mt-row w-full py-2">
            Parar de compartilhar
          </Button>
        </div>
      ) : (
        <Button
          variant="primary"
          icon="monitor"
          onClick={onShare}
          disabled={starting}
          className="mt-group w-full py-2"
        >
          {starting ? 'Conectando…' : 'Compartilhar tela'}
        </Button>
      )}
    </div>
  );
}

/**
 * A sala existe e a conexão de mídia não.
 *
 * Fica no lugar, e não numa torrada: a torrada some em segundos e o que sobra é
 * uma sala de aparência normal — nome do canal, lista de gente, botão de
 * compartilhar — sobre uma conexão que não existe. Quem passou por isso concluiu
 * que o produto não mostra a tela dos outros, e foi procurar o defeito na rede.
 *
 * O botão de compartilhar sai junto, de propósito: sem conexão de espectador,
 * transmitir daqui manda a tela para uma sala que este aplicativo não está
 * vendo, e o usuário não teria como saber disso.
 */
function Fault({ fault }: { fault: MediaFault }) {
  return (
    <div className="mt-group rounded-panel border border-danger/40 bg-surface-2 p-3">
      <p className="flex items-center gap-2 font-medium text-danger">
        <Icon name="alert" size={15} />
        {fault === 'duplicate_identity'
          ? 'Sua conta entrou de outro lugar'
          : 'Sem conexão de mídia'}
      </p>
      <p className="mt-1 text-text-muted">
        {fault === 'duplicate_identity' ? (
          <>
            O ldktela funciona em um computador por vez. Outro aparelho entrou com esta mesma conta
            do Discord e assumiu a sala — por isso nada aparece aqui. Feche o outro e saia e entre
            de novo no canal de voz.
          </>
        ) : (
          <>
            O servidor de mídia não respondeu. O aplicativo continua tentando; se não voltar, saia e
            entre de novo no canal de voz.
          </>
        )}
      </p>
    </div>
  );
}
