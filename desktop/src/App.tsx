import { useEffect } from 'react';
import { start } from './app/runtime';
import { PairingScreen } from './features/pairing/PairingScreen';
import { RoomScreen } from './features/room/RoomScreen';
import { ReleaseNotesDialog } from './features/update/ReleaseNotesDialog';
import { UpdateBanner } from './features/update/UpdateBanner';
import { useSessionStore } from './store/session';
import { Toasts } from './ui/Toasts';

export function App() {
  const phase = useSessionStore((state) => state.phase);

  useEffect(() => {
    void start();
  }, []);

  return (
    <>
      {renderPhase(phase)}
      <Toasts />
      <UpdateBanner />
      <ReleaseNotesDialog />
    </>
  );
}

function renderPhase(phase: ReturnType<typeof useSessionStore.getState>['phase']) {
  switch (phase) {
    case 'booting':
      return <Message text="Abrindo…" />;
    case 'pairing':
      return <PairingScreen />;
    case 'update_required':
      return (
        <Message text="Esta versão é velha demais para o servidor. Atualize o aplicativo para continuar." />
      );
    case 'authenticated':
      return <RoomScreen />;
  }
}

function Message({ text }: { text: string }) {
  return (
    <main className="flex h-full items-center justify-center bg-surface-0 px-8">
      <p className="max-w-md text-center text-text-muted">{text}</p>
    </main>
  );
}
