import { useEffect, useState } from 'react';
import { log } from '../../log';
import type { ShareChoice } from '../../media/session';
import type { ShareSource } from '../../media/native';
import { type PublishPreset, PUBLISH_PRESETS, useMediaStore } from '../../store/media';
import { Button } from '../../ui/Button';

interface SharePickerProps {
  /** Enumerated by the Rust core (ADR-0026), which is why this list is ours. */
  loadSources: () => Promise<ShareSource[]>;
  onCancel: () => void;
  onConfirm: (choice: ShareChoice, preset: PublishPreset) => void;
}

type Loading = { state: 'loading' } | { state: 'ready' } | { state: 'failed'; message: string };

/**
 * Our own picker, source list included (RF-37).
 *
 * Until ADR-0026 this was impossible: WebView2 lets an app cancel a screen
 * capture but never supply the source, so the Chromium dialog was unavoidable.
 * Publishing from the core removed the question — `getDisplayMedia` is never
 * called, so neither the dialog nor the "you are sharing" bar exist.
 */
export function SharePicker({ loadSources, onCancel, onConfirm }: SharePickerProps) {
  const [sources, setSources] = useState<ShareSource[]>([]);
  const [status, setStatus] = useState<Loading>({ state: 'loading' });
  const [selected, setSelected] = useState<string | null>(null);
  const [audio, setAudio] = useState(false);
  const [preset, setPreset] = useState<PublishPreset>(useMediaStore.getState().publishPreset);

  useEffect(() => {
    let live = true;
    loadSources().then(
      (found) => {
        if (!live) {
          return;
        }
        setSources(found);
        setSelected(found[0]?.id ?? null);
        setStatus({ state: 'ready' });
      },
      (error: unknown) => {
        if (!live) {
          return;
        }
        log.error('seletor: não consegui listar as fontes', error);
        setStatus({ state: 'failed', message: 'Não consegui listar suas telas e janelas.' });
      },
    );
    return () => {
      live = false;
    };
  }, [loadSources]);

  const screens = sources.filter((source) => source.kind === 'screen');
  const windows = sources.filter((source) => source.kind === 'window');
  const chosen = sources.find((source) => source.id === selected) ?? null;

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-scrim p-8">
      <div className="flex max-h-full w-full max-w-2xl flex-col rounded-panel border border-border bg-surface-1 p-4">
        <h2 className="font-semibold text-text">O que você quer compartilhar?</h2>

        <div className="mt-group min-h-0 flex-1 overflow-y-auto">
          {status.state === 'loading' && <p className="text-text-muted">Procurando…</p>}
          {status.state === 'failed' && <p className="text-warning">{status.message}</p>}
          {status.state === 'ready' && sources.length === 0 && (
            <p className="text-text-muted">Nenhuma tela ou janela disponível.</p>
          )}

          {screens.length > 0 && (
            <Group title="Telas">
              {screens.map((source) => (
                <SourceOption
                  key={source.id}
                  source={source}
                  checked={source.id === selected}
                  onSelect={() => {
                    setSelected(source.id);
                  }}
                />
              ))}
            </Group>
          )}

          {windows.length > 0 && (
            <Group title="Janelas">
              {windows.map((source) => (
                <SourceOption
                  key={source.id}
                  source={source}
                  checked={source.id === selected}
                  onSelect={() => {
                    setSelected(source.id);
                  }}
                />
              ))}
            </Group>
          )}
        </div>

        <fieldset className="mt-group">
          <legend className="text-text">Qualidade que você vai enviar</legend>
          <p className="text-xs text-text-faint">
            Resolução e taxa de quadros são suas: é a sua máquina que codifica. Quem assiste escolhe
            entre as camadas que chegam.
          </p>
          <div className="mt-row flex flex-wrap gap-2">
            {PUBLISH_PRESETS.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => {
                  setPreset(option);
                }}
                className={
                  option === preset
                    ? 'rounded border border-accent px-2 py-1 text-xs text-text'
                    : 'rounded border border-line px-2 py-1 text-xs text-text-muted hover:text-text'
                }
              >
                {option}
              </button>
            ))}
          </div>
        </fieldset>

        <label className="mt-group flex items-start gap-2 text-text">
          <input
            type="checkbox"
            checked={audio}
            onChange={(event) => {
              setAudio(event.target.checked);
            }}
            className="mt-1 accent-accent"
          />
          <span>
            Incluir o áudio do sistema
            <span className="mt-1 block text-xs text-text-faint">
              Sai tudo o que o computador estiver tocando, <strong>menos o Discord</strong>. A voz
              das outras pessoas não volta para elas. Enquanto você transmite com áudio, o som das
              telas dos outros fica mudo aqui — senão ele seria capturado junto e reenviado.
            </span>
          </span>
        </label>

        <div className="mt-group flex justify-end gap-2">
          <Button onClick={onCancel}>Cancelar</Button>
          <Button
            variant="primary"
            disabled={chosen === null}
            onClick={() => {
              if (chosen !== null) {
                onConfirm({ sourceId: chosen.id, kind: chosen.kind, audio }, preset);
              }
            }}
          >
            Compartilhar
          </Button>
        </div>
      </div>
    </div>
  );
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mb-group">
      <h3 className="mb-row text-xs uppercase tracking-wide text-text-faint">{title}</h3>
      <div className="space-y-row">{children}</div>
    </section>
  );
}

interface SourceOptionProps {
  source: ShareSource;
  checked: boolean;
  onSelect: () => void;
}

function SourceOption({ source, checked, onSelect }: SourceOptionProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={checked}
      className={`block w-full truncate rounded-panel border px-3 py-2 text-left ${
        checked ? 'border-accent bg-accent-soft' : 'border-border bg-surface-2'
      }`}
      title={source.title}
    >
      <span className="block truncate text-text">{source.title}</span>
    </button>
  );
}
