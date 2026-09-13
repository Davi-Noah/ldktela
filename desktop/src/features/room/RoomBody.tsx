import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { useSessionStore } from '../../store/session';
import { Avatar } from '../../ui/Avatar';
import { Button } from '../../ui/Button';
import { PublisherPanel } from './PublisherPanel';

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
  const error = useMediaStore((state) => state.error);

  return (
    <div className="mx-auto flex h-full w-full max-w-md flex-col justify-center px-8">
      <p className="text-text-faint">Na sala</p>
      <h1 className="text-lg font-semibold text-text">{channelName ?? 'Canal de voz'}</h1>

      <ul className="mt-group space-y-row">
        {participantIds.map((id) => {
          const participant = participants[id];
          if (participant === undefined) {
            return null;
          }
          return (
            <li key={id} className="flex items-center gap-2">
              <Avatar url={participant.user.avatar_url} name={participant.user.username} />
              <span className="truncate text-text">
                {participant.user.display_name ?? participant.user.username}
              </span>
              {participant.publishing && (
                <span className="ml-auto text-accent">compartilhando</span>
              )}
            </li>
          );
        })}
      </ul>

      {publishing ? (
        <div className="mt-group">
          <PublisherPanel />
          <Button variant="danger" onClick={onStop} className="mt-group w-full py-2">
            Parar de compartilhar
          </Button>
        </div>
      ) : (
        <Button
          variant="primary"
          onClick={onShare}
          disabled={starting}
          className="mt-group w-full py-2"
        >
          {starting ? 'Escolhendo a tela…' : 'Compartilhar tela'}
        </Button>
      )}

      {error !== null && (
        <p role="alert" className="mt-group text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
