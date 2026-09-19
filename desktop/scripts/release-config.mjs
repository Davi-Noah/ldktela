// Monta o TAURI_CONFIG da build de produção e o imprime em stdout.
//
// O CSP versionado em `src-tauri/tauri.conf.json` só conhece servidores de
// desenvolvimento. O endereço público é de quem hospeda, não do repositório,
// então entra aqui, na hora da build: o `tauri-codegen` mescla o TAURI_CONFIG
// sobre o arquivo e embute o resultado no binário.
//
// Os valores vêm do ambiente ou, na falta dele, de `desktop/.env.production`
// (fora do git). Sem eles o script falha: um pacote cujo CSP não alcança o
// servidor abre, pinta a interface e não fala com ninguém.
import { readFileSync } from 'node:fs';

const desktop = new URL('..', import.meta.url);

function dotenv() {
  try {
    const text = readFileSync(new URL('.env.production', desktop), 'utf8');
    return Object.fromEntries(
      text
        .split(/\r?\n/)
        .map((line) => line.match(/^\s*([A-Z_]+)\s*=\s*(.*?)\s*$/))
        .filter((match) => match !== null)
        .map((match) => [match[1], match[2]]),
    );
  } catch {
    return {};
  }
}

const file = dotenv();
const pick = (name) => (process.env[name] || file[name] || '').trim();

const server = pick('VITE_SERVER_ORIGIN');
const livekit = pick('LIVEKIT_PUBLIC_URL');
if (server === '' || livekit === '') {
  console.error(
    'VITE_SERVER_ORIGIN e LIVEKIT_PUBLIC_URL precisam estar definidos, no ambiente\n' +
      'ou em desktop/.env.production (ver desktop/.env.production.example):\n' +
      '  VITE_SERVER_ORIGIN=http://<IP_PUBLICO>:8090\n' +
      '  LIVEKIT_PUBLIC_URL=ws://<IP_PUBLICO>:7880',
  );
  process.exit(1);
}

const api = new URL(server);
const media = new URL(livekit);
const socket = api.protocol === 'https:' ? 'wss:' : 'ws:';
const allowed = [
  `${api.protocol}//${api.host}`,
  `${socket}//${api.host}`,
  `${media.protocol}//${media.host}`,
];

const base = JSON.parse(readFileSync(new URL('src-tauri/tauri.conf.json', desktop), 'utf8'));
const csp = base.app.security.csp
  .split(';')
  .map((directive) => directive.trim())
  .map((directive) =>
    directive.startsWith('connect-src') ? `${directive} ${allowed.join(' ')}` : directive,
  )
  .join('; ');

process.stdout.write(JSON.stringify({ app: { security: { csp } } }));
