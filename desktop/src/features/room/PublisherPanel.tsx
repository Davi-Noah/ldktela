import type { AudioMode } from '../../media/native';
import { useMediaStore } from '../../store/media';
import { useRoomStore } from '../../store/room';
import { Avatar } from '../../ui/Avatar';
import { Icon } from '../../ui/Icon';
import { kbps } from './format';

/** RF-21 and RF-22, for whoever is publishing. Numbers arrive on an interval. */
export function PublisherPanel() {
  const stats = useMediaStore((state) => state.stats);
  const viewerIds = useMediaStore((state) => state.viewerIds);
  const sharingAudio = useMediaStore((state) => state.sharingAudio);
  const sharingTitle = useMediaStore((state) => state.sharingTitle);
  const audioMode = useMediaStore((state) => state.audioMode);
  const participants = useRoomStore((state) => state.participants);

  return (
    <div className="rounded-panel border border-border bg-surface-1">
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <span className="shrink-0 text-danger">
          <Icon name="dot" size={12} />
        </span>
        <div className="min-w-0 flex-1">
          {/* O objeto direto da frase. Antes dizia "Você está compartilhando" e
              parava aí — e "estou mostrando a coisa certa?" é a pergunta que um
              publicador refaz a cada poucos minutos. */}
          <p className="truncate font-semibold text-text">{sharingTitle ?? 'Sua tela'}</p>
          <p className="truncate text-xs text-text-muted">
            {sharingAudio ? audioLabel(audioMode) : 'Sem áudio'}
          </p>
        </div>
      </div>

      <div className="px-3 py-2">
        {stats !== null && stats.capturedFrames > 30 && stats.encodedFrames === 0 && (
          // A tela está sendo capturada e o encoder recusa tudo: é o estado de uma
          // transmissão pausada por falta de quem assista, e por fora é idêntico a
          // uma captura morta.
          <Notice>
            A tela está sendo capturada, mas nada está sendo codificado. Normalmente é porque
            ninguém está assistindo ainda.
          </Notice>
        )}
        {sharingAudio && stats !== null && stats.audioSamples === 0 && (
          <Notice>
            A captura de áudio abriu, mas nada está chegando. Some com o jogo mudo na aparência e
            não é a mesma coisa: confira se o som está mesmo tocando neste computador.
          </Notice>
        )}
        {audioMode === 'whole_system' && (
          // RF-30: o fallback é declarado, não escondido. Quem descobre isso pelos
          // amigos descobre tarde demais.
          <Notice>
            Não encontrei o Discord em execução, então está saindo o áudio do sistema inteiro. Se
            abrir o Discord depois, pare e recomece o compartilhamento para excluí-lo.
          </Notice>
        )}

        <dl className="grid grid-cols-2 gap-x-2 gap-y-row font-mono">
          <Stat label="Bitrate" value={stats === null ? '—' : kbps(stats.bitrateKbps)} />
          <Stat label="FPS" value={stats === null ? '—' : String(stats.fps)} />
          <Stat
            label="Resolução"
            value={stats === null || stats.width === 0 ? '—' : `${stats.width}×${stats.height}`}
          />
          <Stat
            label="Encoder"
            value={stats === null ? '—' : stats.hardwareEncoder ? 'hardware' : 'software'}
          />
        </dl>

        <p className="mt-group text-text-faint">
          {viewerIds.length === 0
            ? 'Ninguém assistindo ainda'
            : viewerIds.length === 1
              ? '1 pessoa assistindo'
              : `${viewerIds.length} pessoas assistindo`}
        </p>
        {viewerIds.length > 0 && (
          <ul className="mt-row flex flex-wrap gap-2">
            {viewerIds.map((id) => {
              const viewer = participants[id];
              const name = viewer?.user.display_name ?? viewer?.user.username ?? id;
              return (
                <li
                  key={id}
                  className="flex items-center gap-1.5 rounded-pill bg-surface-2 py-0.5 pl-0.5 pr-2"
                >
                  <Avatar url={viewer?.user.avatar_url ?? null} name={name} size={20} />
                  <span className="truncate text-xs text-text">{name}</span>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

function Notice({ children }: { children: React.ReactNode }) {
  return (
    <p className="mb-group flex items-start gap-2 rounded-panel bg-surface-2 p-2 text-warning">
      <span className="mt-0.5 shrink-0">
        <Icon name="alert" size={15} />
      </span>
      <span className="text-text-muted">{children}</span>
    </p>
  );
}

function audioLabel(mode: AudioMode | null): string {
  return mode === 'whole_system' ? 'Com áudio do sistema inteiro' : 'Com áudio, sem o Discord';
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="font-ui text-text-faint">{label}</dt>
      <dd className="text-right text-text">{value}</dd>
    </>
  );
}
