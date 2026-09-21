import { log } from '../log';

/**
 * Os avisos sonoros de uma tela que entra e de uma tela que sai.
 *
 * **Sintetizados, e não arquivos.** Duas senoides com envelope são vinte linhas
 * e nenhum byte de mídia no repositório — que é público e GPL, onde um som
 * baixado traria uma licença junto para ninguém conferir depois. Sintetizar
 * também deixa volume, duração e altura no código, onde se discute o que
 * incomoda, em vez de dentro de um `.mp3` que só se ajusta regravando.
 *
 * Os dois usam o mesmo par de notas, uma quinta justa — o intervalo mais
 * consonante depois da oitava. **Subindo quando uma tela entra, descendo quando
 * ela sai**: é a mesma convenção de porta que abre e porta que fecha, e dispensa
 * o usuário aprender qual bipe é qual.
 */

const E5 = 659.25;
const B5 = 987.77;

interface Chime {
  /** Em ordem de execução: a primeira nota dá o sentido do movimento. */
  readonly tones: readonly { hz: number; delay: number }[];
  /**
   * Baixo de propósito. Isto toca por cima de jogo, voz e da tela de alguém; um
   * aviso que se sobrepõe ao que a pessoa estava ouvindo é o aviso que ela
   * desliga.
   */
  readonly peak: number;
  readonly decay: number;
}

const CHIMES: Record<ChimeKind, Chime> = {
  start: {
    tones: [
      { hz: E5, delay: 0 },
      { hz: B5, delay: 0.08 },
    ],
    peak: 0.06,
    decay: 0.45,
  },
  // Sair é mais discreto que entrar: uma tela que acabou não pede atenção
  // nenhuma, só fecha o assunto. Daí o volume menor e a cauda mais curta.
  stop: {
    tones: [
      { hz: B5, delay: 0 },
      { hz: E5, delay: 0.08 },
    ],
    peak: 0.045,
    decay: 0.35,
  },
};

export type ChimeKind = 'start' | 'stop';

/** Ataque curto, mas não instantâneo: ligar uma senoide em zero estala. */
const ATTACK = 0.012;

/**
 * Duas telas que começam juntas são um aviso, não dois.
 *
 * Também é o que segura a retomada do gateway: `SHARE_START` e `SHARE_STOP`
 * entram no buffer de resume (`docs/websocket.md` §7), então reconectar depois
 * de uma queda entrega os eventos perdidos de uma vez — e sem isto a volta
 * tocaria uma sequência de sinos.
 */
const BURST_MS = 1500;

export function shouldChime(options: {
  publisherId: string;
  selfId: string | undefined;
  now: number;
  lastAt: number | null;
}): boolean {
  // Avisar alguém da própria transmissão é a definição de ruído.
  if (options.selfId !== undefined && options.publisherId === options.selfId) {
    return false;
  }
  return options.lastAt === null || options.now - options.lastAt >= BURST_MS;
}

// Uma marca por tipo, e não uma só: entrar e sair são avisos diferentes, e uma
// tela que acaba no instante em que outra começa precisa que os dois soem.
const lastAt: Record<ChimeKind, number | null> = { start: null, stop: null };
let context: AudioContext | null = null;

/** Called when someone's screen goes live or ends. Decides, then plays. */
export function chimeForShare(
  kind: ChimeKind,
  publisherId: string,
  selfId: string | undefined,
): void {
  const now = Date.now();
  if (!shouldChime({ publisherId, selfId, now, lastAt: lastAt[kind] })) {
    return;
  }
  lastAt[kind] = now;
  play(CHIMES[kind]);
}

/**
 * Acorda o áudio no primeiro gesto do usuário.
 *
 * Um `AudioContext` criado sem gesto nasce suspenso, e o primeiro sino sairia
 * mudo — justamente o primeiro, que é quando a pessoa ainda não sabe que existe
 * um. Um clique ou uma tecla em qualquer lugar do aplicativo resolve, e depois
 * disso o ouvinte se remove.
 */
export function primeChime(): void {
  const wake = () => {
    ensureContext();
  };
  window.addEventListener('pointerdown', wake, { once: true });
  window.addEventListener('keydown', wake, { once: true });
}

function ensureContext(): AudioContext | null {
  // Sem `AudioContext` — jsdom nos testes, ou um runtime que não o traz — o
  // aplicativo continua inteiro, só sem aviso sonoro.
  if (typeof AudioContext === 'undefined') {
    return null;
  }
  context ??= new AudioContext();
  if (context.state === 'suspended') {
    context.resume().catch((error: unknown) => {
      log.debug('som: o navegador não liberou o áudio ainda', { error: String(error) });
    });
  }
  return context;
}

function play(chime: Chime): void {
  const audio = ensureContext();
  if (audio === null) {
    return;
  }
  try {
    const start = audio.currentTime;
    for (const tone of chime.tones) {
      const oscillator = audio.createOscillator();
      const gain = audio.createGain();
      oscillator.type = 'sine';
      oscillator.frequency.value = tone.hz;

      const at = start + tone.delay;
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(chime.peak, at + ATTACK);
      // Exponencial, como o ouvido mede intensidade: uma rampa linear até zero
      // soa como um corte, e não como uma nota que acaba.
      gain.gain.exponentialRampToValueAtTime(0.0001, at + chime.decay);

      oscillator.connect(gain).connect(audio.destination);
      oscillator.start(at);
      oscillator.stop(at + chime.decay + 0.02);
    }
  } catch (error) {
    log.warn('som: não consegui tocar o aviso', { error: String(error) });
  }
}
