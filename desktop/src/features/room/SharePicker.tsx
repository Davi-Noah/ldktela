import { useCallback, useEffect, useState } from 'react';
import { log } from '../../log';
import type { ShareChoice } from '../../media/session';
import type { ShareSource, SourceKind } from '../../media/native';
import { type PublishPreset, PUBLISH_PRESETS, useMediaStore } from '../../store/media';
import { Button } from '../../ui/Button';
import { Dialog } from '../../ui/Dialog';
import { Icon } from '../../ui/Icon';
import { IconButton } from '../../ui/IconButton';
import { Toggle } from '../../ui/Toggle';

interface SharePickerProps {
  /** Enumerated by the Rust core (ADR-0026), which is why this list is ours. */
  loadSources: () => Promise<ShareSource[]>;
  /** One picture per source, asked for as the cards are drawn (RF-37). */
  loadThumbnail: (kind: SourceKind, id: string) => Promise<string | null>;
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
 *
 * As **miniaturas** são o ponto. Escolher por título falha todo dia: três
 * janelas do mesmo navegador têm títulos parecidos, e dois monitores se chamam
 * "Tela 1" e "Tela 2" sem nada que diga qual é qual. O usuário escolhia, errava,
 * e descobria pelo amigo do outro lado.
 */
export function SharePicker({ loadSources, loadThumbnail, onCancel, onConfirm }: SharePickerProps) {
  const [sources, setSources] = useState<ShareSource[]>([]);
  const [status, setStatus] = useState<Loading>({ state: 'loading' });
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<SourceKind>('screen');
  const [audio, setAudio] = useState(false);
  const [preset, setPreset] = useState<PublishPreset>(useMediaStore.getState().publishPreset);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let live = true;
    setStatus({ state: 'loading' });
    loadSources().then(
      (found) => {
        if (!live) {
          return;
        }
        setSources(found);
        setSelected((current) =>
          current !== null && found.some((source) => source.id === current)
            ? current
            : (found[0]?.id ?? null),
        );
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
  }, [loadSources, nonce]);

  const screens = sources.filter((source) => source.kind === 'screen');
  const windows = sources.filter((source) => source.kind === 'window');
  const shown = tab === 'screen' ? screens : windows;
  const chosen = sources.find((source) => source.id === selected) ?? null;

  const confirm = useCallback(() => {
    if (chosen !== null) {
      onConfirm({ sourceId: chosen.id, kind: chosen.kind, audio, title: chosen.title }, preset);
    }
  }, [chosen, audio, preset, onConfirm]);

  return (
    <Dialog
      title="Compartilhar tela"
      onClose={onCancel}
      footer={
        <div className="flex items-center justify-between gap-3">
          <div className="flex gap-2">
            <Button onClick={onCancel}>Cancelar</Button>
            <Button variant="primary" disabled={chosen === null} onClick={confirm}>
              Compartilhar
            </Button>
          </div>
        </div>
      }
    >
      <div
        // Enter confirma, e as setas andam pela grade: um seletor é uma grade,
        // e navegar grade com Tab, cartão por cartão, entre quinze janelas, é
        // inutilizável.
        onKeyDown={(event) => {
          if (event.key === 'Enter' && chosen !== null) {
            event.preventDefault();
            confirm();
            return;
          }
          const step = ARROWS[event.key];
          if (step === undefined || shown.length === 0) {
            return;
          }
          event.preventDefault();
          const at = shown.findIndex((source) => source.id === selected);
          const next = Math.min(shown.length - 1, Math.max(0, (at === -1 ? 0 : at) + step));
          setSelected(shown[next]?.id ?? null);
        }}
      >
        <div className="flex items-center gap-2">
          <div
            role="tablist"
            aria-label="Tipo de fonte"
            className="flex rounded-panel bg-surface-2 p-0.5"
          >
            <Tab
              active={tab === 'screen'}
              count={screens.length}
              icon="monitor"
              onSelect={() => {
                setTab('screen');
                setSelected(screens[0]?.id ?? null);
              }}
            >
              Telas
            </Tab>
            <Tab
              active={tab === 'window'}
              count={windows.length}
              icon="window"
              onSelect={() => {
                setTab('window');
                setSelected(windows[0]?.id ?? null);
              }}
            >
              Janelas
            </Tab>
          </div>
          <span className="ml-auto">
            <IconButton
              icon="refresh"
              label="Atualizar a lista"
              variant="ghost"
              tipSide="bottom"
              onClick={() => {
                setNonce((value) => value + 1);
              }}
            />
          </span>
        </div>

        {status.state === 'loading' && <p className="mt-group text-text-muted">Procurando…</p>}
        {status.state === 'failed' && <p className="mt-group text-danger">{status.message}</p>}
        {status.state === 'ready' && shown.length === 0 && (
          <p className="mt-group text-text-muted">
            {tab === 'screen' ? 'Nenhuma tela disponível.' : 'Nenhuma janela aberta para mostrar.'}
          </p>
        )}

        <div className="mt-group grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-2">
          {shown.map((source) => (
            <SourceCard
              key={source.id}
              source={source}
              checked={source.id === selected}
              loadThumbnail={loadThumbnail}
              onSelect={() => {
                setSelected(source.id);
              }}
              onConfirm={confirm}
            />
          ))}
        </div>
      </div>

      <div className="mt-group border-t border-line pt-3">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <span className="text-text-faint">Qualidade</span>
          <div role="radiogroup" aria-label="Qualidade que você vai enviar" className="flex gap-1">
            {PUBLISH_PRESETS.map((option) => (
              <button
                key={option}
                type="button"
                role="radio"
                aria-checked={option === preset}
                onClick={() => {
                  setPreset(option);
                }}
                className={`rounded-pill border px-2.5 py-1 text-xs ${
                  option === preset
                    ? 'border-accent bg-accent-soft text-text'
                    : 'border-border text-text-muted hover:text-text'
                }`}
              >
                {option}
              </button>
            ))}
          </div>
          <Hint>
            Resolução e taxa de quadros são suas: é a sua máquina que codifica. Quem assiste escolhe
            entre as camadas que chegam.
          </Hint>
        </div>

        <div className="mt-row flex flex-wrap items-center gap-x-4 gap-y-2">
          <span className="text-text-faint">Áudio</span>
          <Toggle
            checked={audio}
            onChange={setAudio}
            label="Incluir o som do computador"
            describedBy="share-audio-hint"
          />
          <Hint id="share-audio-hint">
            Sai tudo o que o computador estiver tocando, <strong>menos o Discord</strong>. A voz das
            outras pessoas não volta para elas. Enquanto você transmite com áudio, o som das telas
            dos outros fica mudo aqui — senão ele seria capturado junto e reenviado.
          </Hint>
        </div>
      </div>
    </Dialog>
  );
}

const ARROWS: Record<string, number | undefined> = {
  ArrowRight: 1,
  ArrowLeft: -1,
};

/**
 * O texto longo continua existindo — ele é necessário, e é bom — mas sob
 * demanda. Quatro linhas de explicação empilhadas eram o que fazia o diálogo
 * parecer entulhado.
 */
function Hint({ children, id }: { children: React.ReactNode; id?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        aria-expanded={open}
        aria-label="O que isso quer dizer"
        onClick={() => {
          setOpen((value) => !value);
        }}
        className="text-text-faint hover:text-text"
      >
        <Icon name="info" size={15} />
      </button>
      {open && (
        <p id={id} className="selectable w-full text-xs text-text-muted">
          {children}
        </p>
      )}
    </>
  );
}

interface TabProps {
  active: boolean;
  count: number;
  icon: 'monitor' | 'window';
  children: React.ReactNode;
  onSelect: () => void;
}

function Tab({ active, count, icon, children, onSelect }: TabProps) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onSelect}
      className={`flex items-center gap-1.5 rounded-panel px-3 py-1 ${
        active ? 'bg-surface-3 text-text' : 'text-text-muted hover:text-text'
      }`}
    >
      <Icon name={icon} size={15} />
      {children}
      <span className="text-xs text-text-faint">{count}</span>
    </button>
  );
}

interface SourceCardProps {
  source: ShareSource;
  checked: boolean;
  loadThumbnail: (kind: SourceKind, id: string) => Promise<string | null>;
  onSelect: () => void;
  onConfirm: () => void;
}

function SourceCard({ source, checked, loadThumbnail, onSelect, onConfirm }: SourceCardProps) {
  const [thumbnail, setThumbnail] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    loadThumbnail(source.kind, source.id).then(
      (found) => {
        if (live) {
          setThumbnail(found);
        }
      },
      () => {
        // Uma fonte sem figura continua escolhível pelo título; um erro na tela
        // por causa de uma miniatura seria desproporcional.
      },
    );
    return () => {
      live = false;
    };
  }, [loadThumbnail, source.kind, source.id]);

  return (
    <button
      type="button"
      onClick={onSelect}
      onDoubleClick={onConfirm}
      aria-pressed={checked}
      title={source.title}
      className={`overflow-hidden rounded-panel border text-left ${
        checked ? 'border-accent bg-accent-soft' : 'border-border bg-surface-2 hover:border-line'
      }`}
    >
      <span className="flex aspect-video items-center justify-center bg-stage">
        {thumbnail === null ? (
          <span className="text-text-faint">
            <Icon name={source.kind === 'screen' ? 'monitor' : 'window'} size={28} />
          </span>
        ) : (
          <img src={thumbnail} alt="" className="h-full w-full object-contain" draggable={false} />
        )}
      </span>
      <span className="flex items-center gap-1.5 px-2 py-1.5">
        <span className="shrink-0 text-text-faint">
          <Icon name={source.kind === 'screen' ? 'monitor' : 'window'} size={14} />
        </span>
        <span className="truncate text-text">{source.title}</span>
      </span>
    </button>
  );
}
