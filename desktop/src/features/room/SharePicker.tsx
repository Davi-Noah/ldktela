import { useState } from 'react';
import type { CaptureRequest, CaptureSurface } from '../../media/tracks';
import { audioAvailableFor } from '../../media/tracks';
import { Button } from '../../ui/Button';

interface SharePickerProps {
  onCancel: () => void;
  onConfirm: (request: CaptureRequest) => void;
}

export function SharePicker({ onCancel, onConfirm }: SharePickerProps) {
  const [surface, setSurface] = useState<CaptureSurface>('monitor');
  const [audio, setAudio] = useState(false);
  const audioPossible = audioAvailableFor(surface);

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-scrim p-8">
      <div className="w-full max-w-md rounded-panel border border-border bg-surface-1 p-4">
        <h2 className="font-semibold text-text">O que você quer compartilhar?</h2>

        <div className="mt-group space-y-row">
          <Option
            checked={surface === 'monitor'}
            onSelect={() => {
              setSurface('monitor');
            }}
            title="Uma tela inteira"
            detail="Único modo em que o Windows entrega áudio."
          />
          <Option
            checked={surface === 'window'}
            onSelect={() => {
              setSurface('window');
              setAudio(false);
            }}
            title="Uma janela"
            detail="Sem áudio: capturar o som de uma janela só ainda não existe."
          />
        </div>

        <label
          className={`mt-group flex items-start gap-2 ${audioPossible ? 'text-text' : 'text-text-faint'}`}
        >
          <input
            type="checkbox"
            checked={audio && audioPossible}
            disabled={!audioPossible}
            onChange={(event) => {
              setAudio(event.target.checked);
            }}
            className="mt-1 accent-accent"
          />
          <span>
            Incluir o áudio do sistema
            {audioPossible && (
              // RF-30: the warning is the honest part, and it is not a footnote.
              <span className="mt-1 block text-warning">
                O Windows só entrega o áudio do sistema inteiro. A voz de todo mundo no Discord vai
                junto e volta com atraso para eles. Fone de ouvido não resolve.
              </span>
            )}
          </span>
        </label>

        <div className="mt-group flex justify-end gap-2">
          <Button onClick={onCancel}>Cancelar</Button>
          <Button
            variant="primary"
            onClick={() => {
              onConfirm({ surface, audio: audio && audioPossible });
            }}
          >
            Escolher tela
          </Button>
        </div>
      </div>
    </div>
  );
}

interface OptionProps {
  checked: boolean;
  onSelect: () => void;
  title: string;
  detail: string;
}

function Option({ checked, onSelect, title, detail }: OptionProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={checked}
      className={`block w-full rounded-panel border px-3 py-2 text-left ${
        checked ? 'border-accent bg-accent-soft' : 'border-border bg-surface-2'
      }`}
    >
      <span className="block text-text">{title}</span>
      <span className="block text-text-muted">{detail}</span>
    </button>
  );
}
