import { log } from '../../log';
import { markVersionSeen } from '../../platform/releaseNotes';
import { useReleaseNotesStore } from '../../store/releaseNotes';
import { Button } from '../../ui/Button';
import { Dialog } from '../../ui/Dialog';
import type { Block, Inline } from './notes';

/**
 * As novidades da versão que acabou de ser instalada (ADR-0040).
 *
 * Um modal, ao contrário do aviso de atualização: aquele pede uma decisão que
 * pode esperar, e este aparece uma vez só e explica o que mudou — inclusive
 * comportamento que, sem explicação, parece defeito.
 *
 * A versão só é marcada como vista ao **fechar**. O aplicativo abre direto na
 * bandeja; marcar ao montar consumiria as novidades numa janela que ninguém
 * abriu.
 */
export function ReleaseNotesDialog() {
  const pending = useReleaseNotesStore((state) => state.pending);
  if (pending === null) {
    return null;
  }

  const { version, notes } = pending;
  const close = () => {
    useReleaseNotesStore.getState().clear();
    markVersionSeen(version).catch((error: unknown) => {
      // Sem registro, as novidades voltam na próxima abertura. Incomoda uma vez
      // a mais, e não esconde nada.
      log.warn('novidades: não consegui registrar como vistas', { versão: version, error });
    });
  };

  return (
    <Dialog
      title={notes.title ?? `Novidades da versão ${version}`}
      onClose={close}
      width="max-w-lg"
      footer={
        <div className="flex justify-end">
          <Button variant="primary" onClick={close}>
            Entendi
          </Button>
        </div>
      }
    >
      <div className="pb-1">
        {notes.blocks.map((block, index) => (
          <NotesBlock key={index} block={block} />
        ))}
      </div>
    </Dialog>
  );
}

function NotesBlock({ block }: { block: Block }) {
  switch (block.kind) {
    case 'heading':
      return (
        <h3
          className={`mb-1.5 mt-4 font-semibold text-text first:mt-0 ${block.level === 3 ? 'text-xs uppercase tracking-wide text-text-muted' : ''}`}
        >
          <Inlines inlines={block.inlines} />
        </h3>
      );
    case 'paragraph':
      return (
        <p className="my-2 text-text-muted">
          <Inlines inlines={block.inlines} />
        </p>
      );
    case 'list': {
      const List = block.ordered ? 'ol' : 'ul';
      return (
        <List
          className={`my-2 space-y-1.5 pl-5 text-text-muted marker:text-text-faint ${block.ordered ? 'list-decimal' : 'list-disc'}`}
        >
          {block.items.map((item, index) => (
            <li key={index}>
              <Inlines inlines={item} />
            </li>
          ))}
        </List>
      );
    }
    case 'quote':
      return (
        <blockquote className="my-2 border-l-2 border-line pl-3 text-text-muted">
          <Inlines inlines={block.inlines} />
        </blockquote>
      );
    case 'code':
      return (
        <pre className="my-2 overflow-x-auto rounded-panel bg-surface-2 p-3 font-mono text-xs text-text">
          {block.text}
        </pre>
      );
    case 'rule':
      return <hr className="my-4 border-line" />;
  }
}

function Inlines({ inlines }: { inlines: Inline[] }) {
  return (
    <>
      {inlines.map((inline, index) => {
        switch (inline.kind) {
          case 'text':
            return <span key={index}>{inline.text}</span>;
          case 'strong':
            return (
              <strong key={index} className="font-semibold text-text">
                {inline.text}
              </strong>
            );
          case 'em':
            return <em key={index}>{inline.text}</em>;
          case 'code':
            return (
              <code
                key={index}
                className="rounded bg-surface-2 px-1 py-0.5 font-mono text-xs text-text"
              >
                {inline.text}
              </code>
            );
        }
      })}
    </>
  );
}
