import { type FormEvent, useState } from 'react';
import { pair } from '../../app/runtime';
import { useSessionStore } from '../../store/session';
import { Button } from '../../ui/Button';
import { PAIRING_CODE_LENGTH, isCompletePairingCode, normalizePairingCode } from './code';

export function PairingScreen() {
  const [code, setCode] = useState('');
  const error = useSessionStore((state) => state.pairingError);
  const busy = useSessionStore((state) => state.pairing);
  const ready = isCompletePairingCode(code) && !busy;

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (ready) {
      void pair(code);
    }
  }

  return (
    <main className="flex h-full items-center justify-center bg-surface-0 p-8">
      <form onSubmit={onSubmit} className="w-full max-w-sm">
        <h1 className="text-lg font-semibold text-text">ldktela</h1>
        <p className="mt-1 text-text-muted">
          Compartilhe sua tela para o canal de voz do Discord em que você já está.
        </p>

        <ol className="mt-group space-y-row text-text-muted">
          <li>
            1. No Discord, rode{' '}
            <code className="rounded bg-surface-2 px-1 font-mono text-text">/tela</code>.
          </li>
          <li>
            2. Digite abaixo o código de {PAIRING_CODE_LENGTH} caracteres que o bot responder.
          </li>
        </ol>

        <label htmlFor="pairing-code" className="mt-group block text-text-faint">
          Código de pareamento
        </label>
        <input
          id="pairing-code"
          value={code}
          onChange={(event) => {
            setCode(normalizePairingCode(event.target.value));
          }}
          autoFocus
          autoComplete="off"
          spellCheck={false}
          inputMode="text"
          placeholder="XXXXXXXX"
          aria-describedby={error === null ? undefined : 'pairing-error'}
          className="mt-1 w-full rounded-panel border border-border bg-surface-1 px-3 py-2 text-center font-mono text-xl tracking-[0.35em] text-text uppercase placeholder:text-text-faint"
        />

        <Button type="submit" variant="primary" disabled={!ready} className="mt-group w-full py-2">
          {busy ? 'Pareando…' : 'Parear'}
        </Button>

        {error !== null && (
          <p id="pairing-error" role="alert" className="mt-group text-danger">
            {error}
          </p>
        )}

        <p className="mt-group text-text-faint">O código vale por 5 minutos e serve uma vez só.</p>
      </form>
    </main>
  );
}
