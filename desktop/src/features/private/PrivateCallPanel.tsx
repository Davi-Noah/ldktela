import { type FormEvent, useState } from 'react';
import { createPrivateCall, endPrivateCall, joinPrivateCall } from '../../app/runtime';
import { useMediaStore } from '../../store/media';
import { usePrivateCallStore } from '../../store/privateCall';
import { useSessionStore } from '../../store/session';
import { Button } from '../../ui/Button';
import { PublisherPanel } from '../room/PublisherPanel';

interface Props {
  onShare: () => void;
  onStop: () => void;
}

export function PrivateCallPanel({ onShare, onStop }: Props) {
  const call = usePrivateCallStore((state) => state.call);
  return call === null ? (
    <PrivateCallIdle />
  ) : (
    <PrivateCallActive onShare={onShare} onStop={onStop} />
  );
}

function PrivateCallIdle() {
  const [code, setCode] = useState('');
  const busy = usePrivateCallStore((state) => state.busy);
  const error = usePrivateCallStore((state) => state.error);

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (code.trim().length === 22 && !busy) {
      void joinPrivateCall(code);
    }
  }

  return (
    <div className="mt-group w-full max-w-sm border-t border-border pt-group">
      <p className="text-text">Chamada privada</p>
      <p className="mt-1 text-text-muted">Crie uma chamada 1:1 ou entre com o código recebido.</p>
      <Button
        variant="primary"
        onClick={() => void createPrivateCall()}
        disabled={busy}
        className="mt-group w-full py-2"
      >
        Criar chamada
      </Button>
      <form onSubmit={submit} className="mt-row">
        <label htmlFor="private-call-code" className="block text-text-faint">
          Código da chamada
        </label>
        <input
          id="private-call-code"
          value={code}
          onChange={(event) => setCode(event.target.value.trim().slice(0, 22))}
          autoComplete="off"
          spellCheck={false}
          className="mt-1 w-full rounded-panel border border-border bg-surface-1 px-3 py-2 font-mono text-text"
        />
        <Button
          type="submit"
          disabled={busy || code.trim().length !== 22}
          className="mt-row w-full py-2"
        >
          Entrar com código
        </Button>
      </form>
      {error !== null && <p className="mt-row text-danger">{error}</p>}
    </div>
  );
}

function PrivateCallActive({ onShare, onStop }: Props) {
  const call = usePrivateCallStore((state) => state.call);
  const inviteCode = usePrivateCallStore((state) => state.inviteCode);
  const error = usePrivateCallStore((state) => state.error);
  const selfId = useSessionStore((state) => state.user?.id);
  const publishing = useMediaStore((state) => state.publishing);
  const starting = useMediaStore((state) => state.starting);
  if (call === null) {
    return null;
  }
  const owner = call.owner.display_name ?? call.owner.username;
  const guest =
    call.guest === undefined
      ? 'Aguardando convidado'
      : (call.guest.display_name ?? call.guest.username);
  return (
    <div className="mx-auto flex h-full w-full max-w-md flex-col justify-center px-8">
      <p className="text-text-faint">Chamada privada</p>
      <h1 className="text-lg font-semibold text-text">
        {owner} e {guest}
      </h1>
      {inviteCode !== null && call.guest === undefined && (
        <div className="mt-group rounded-panel border border-border bg-surface-1 p-3">
          <p className="text-text-faint">Envie este código</p>
          <p className="mt-1 break-all font-mono text-text">{inviteCode}</p>
        </div>
      )}
      {publishing ? (
        <div className="mt-group">
          <PublisherPanel />
          <Button variant="danger" onClick={onStop} className="mt-row w-full py-2">
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
          {starting ? 'Conectando…' : 'Compartilhar tela'}
        </Button>
      )}
      {selfId === call.owner.id && (
        <Button onClick={() => void endPrivateCall()} className="mt-row w-full py-2">
          Encerrar chamada
        </Button>
      )}
      {error !== null && <p className="mt-row text-danger">{error}</p>}
    </div>
  );
}
