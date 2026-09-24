import { afterEach, describe, expect, it, vi } from 'vitest';
import tauriConfig from '../../src-tauri/tauri.conf.json';
import { RELEASES_API } from '../config';
import { fetchReleaseText } from './releaseNotes';

/**
 * As novidades vêm do release publicado no GitHub (ADR-0040). O que se prova
 * aqui é a fronteira com a rede: de onde o texto vem e o que cada resposta
 * significa para o registro de "já vista".
 */
describe('o texto do release', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /**
   * Notas de um repositório e atualizações de outro mostrariam a novidade
   * errada — ou nenhuma. O repositório está escrito em dois lugares; este teste
   * não deixa os dois se separarem.
   */
  it('vem do mesmo repositório que entrega as atualizações', () => {
    const endpoint = tauriConfig.plugins.updater.endpoints[0] ?? '';
    const updater = /github\.com\/([^/]+\/[^/]+)\/releases/.exec(endpoint)?.[1];
    const notes = /repos\/([^/]+\/[^/]+)\/releases$/.exec(RELEASES_API)?.[1];
    expect(updater).toBeDefined();
    expect(notes).toBe(updater);
  });

  /** Sem esta entrada, o CSP do WebView recusa o pedido e as novidades nunca aparecem. */
  it('é alcançável pelo CSP do WebView', () => {
    const connect =
      tauriConfig.app.security.csp
        .split(';')
        .map((directive) => directive.trim())
        .find((directive) => directive.startsWith('connect-src')) ?? '';
    expect(connect.split(/\s+/)).toContain(new URL(RELEASES_API).origin);
  });

  it('é pedido pela tag da versão em execução', async () => {
    const fetchMock = vi.fn(() => Promise.resolve(Response.json({ body: '## Corrigido' })));
    vi.stubGlobal('fetch', fetchMock);

    await expect(fetchReleaseText('2.0.1')).resolves.toEqual({
      version: '2.0.1',
      markdown: '## Corrigido',
    });
    expect(fetchMock).toHaveBeenCalledWith(`${RELEASES_API}/tags/v2.0.1`, expect.anything());
  });

  /** Build de desenvolvimento, ou versão sem release: nada a mostrar, e pode marcar. */
  it('volta vazio quando a versão não tem release', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(new Response(null, { status: 404 })));
    await expect(fetchReleaseText('9.9.9')).resolves.toBeNull();
  });

  it('volta vazio quando o release não tem texto', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(Response.json({ body: '  ' })));
    await expect(fetchReleaseText('2.0.1')).resolves.toBeNull();
  });

  /**
   * Limite de pedidos, GitHub fora do ar: não dá para saber se há novidade.
   * Falhar, em vez de voltar vazio, é o que impede a versão de ser marcada como
   * vista — e a próxima abertura tenta de novo.
   */
  it('falha quando não deu para saber, para a próxima abertura tentar de novo', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(new Response(null, { status: 403 })));
    await expect(fetchReleaseText('2.0.1')).rejects.toThrow();
  });
});
