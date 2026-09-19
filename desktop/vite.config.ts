// `loadEnv` sai do vite; `defineConfig` sai do vitest, que é quem conhece o
// campo `test`. Importar os dois do mesmo lugar não funciona.
import { loadEnv } from 'vite';
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
// Importado, e não lido com `node:fs`: ler o arquivo custaria `@types/node` só
// para duas linhas de configuração (CLAUDE.md §2.11).
import tauriConfig from './src-tauri/tauri.conf.json';

/**
 * Onde o aplicativo instalado procura o servidor.
 *
 * Em desenvolvimento, `config.ts` cai em `127.0.0.1:8080` sozinho e está certo.
 * Num pacote, esse mesmo silêncio produz um `.msi` que aponta para a máquina de
 * quem instalou: nada pareia, nada conecta, e o instalador parece simplesmente
 * não funcionar. Já esteve assim — `VITE_SERVER_ORIGIN` só existia num
 * `.env.local` fora do git, e o fluxo de release não passava valor nenhum. Hoje
 * vem de `.env.production` (fora do git, ver `.env.production.example`) ou, no
 * CI, de uma variável do repositório.
 *
 * Então a build de produção falha aqui, antes de existir instalador para
 * distribuir. Um pacote que não alcança o servidor não é um pacote.
 */
function requireServerOrigin(mode: string): string {
  const origin = loadEnv(mode, '.', 'VITE_').VITE_SERVER_ORIGIN?.trim();
  if (origin === undefined || origin.length === 0) {
    throw new Error(
      'VITE_SERVER_ORIGIN não está definido.\n' +
        'Sem ele o aplicativo empacotado aponta para http://127.0.0.1:8080, que é a\n' +
        'máquina de quem instalou. Defina no ambiente da build, ou em desktop/.env.production\n' +
        '(copie de desktop/.env.production.example):\n' +
        '  VITE_SERVER_ORIGIN=http://<IP_PUBLICO_DO_SERVIDOR>:8090\n' +
        'Ver docs/deploy-oracle.md.',
    );
  }
  return origin.replace(/\/+$/, '');
}

/**
 * O CSP do Tauri e o endereço do servidor precisam concordar, e nada os obriga a
 * isso: são dois arquivos diferentes, mexidos em momentos diferentes.
 *
 * Quando discordam, o aplicativo abre, a interface pinta, e toda requisição é
 * bloqueada pelo WebView — sem erro de rede, sem mensagem, só nada acontecendo.
 * É o modo de falha mais caro de diagnosticar que este projeto tem, e é barato
 * de impedir.
 *
 * O CSP que vale é o do TAURI_CONFIG, quando existe: o endereço público não é
 * versionado, e `scripts/release-config.mjs` o acrescenta na hora da build.
 */
function assertCspAllows(mode: string, origin: string): void {
  const merged = loadEnv(mode, '.', 'TAURI_CONFIG').TAURI_CONFIG;
  const csp = merged === undefined ? tauriConfig.app.security.csp : cspOf(JSON.parse(merged));
  if (typeof csp !== 'string') {
    return;
  }
  const host = new URL(origin).host;
  if (!csp.includes(host)) {
    throw new Error(
      `O CSP do pacote não permite ${host}.\n` +
        'O aplicativo abriria normalmente e toda requisição seria bloqueada em silêncio.\n' +
        'Gere o TAURI_CONFIG com `node scripts/release-config.mjs` (o `just build-app` já faz isso).',
    );
  }
}

function cspOf(config: unknown): unknown {
  if (typeof config !== 'object' || config === null || !('app' in config)) {
    return undefined;
  }
  const { app } = config;
  if (typeof app !== 'object' || app === null || !('security' in app)) {
    return undefined;
  }
  const { security } = app;
  return typeof security === 'object' && security !== null && 'csp' in security
    ? security.csp
    : undefined;
}

export default defineConfig(({ command, mode }) => {
  if (command === 'build') {
    const origin = requireServerOrigin(mode);
    assertCspAllows(mode, origin);
  }

  return {
    plugins: [react(), tailwindcss()],
    clearScreen: false,
    server: { port: 1420, strictPort: true },
    build: { target: 'chrome110', sourcemap: true },
    test: {
      environment: 'jsdom',
      globals: true,
      setupFiles: ['./src/test/setup.ts'],
      passWithNoTests: true,
      include: ['src/**/*.test.{ts,tsx}'],
    },
  };
});
