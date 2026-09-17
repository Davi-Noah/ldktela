/** Formatação de número para a sala. Fora dos componentes por causa do
    fast refresh, que só funciona em arquivo que exporta apenas componentes. */

/** Tempo no ar (RF-34): `12:34`, e `1:02:03` a partir de uma hora. */
export function elapsed(total: number): string {
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}

/** Bitrate legível: vira Mb/s quando passa de mil, para a pílula não crescer. */
export function kbps(value: number): string {
  return value >= 1000 ? `${(value / 1000).toFixed(1)} Mb/s` : `${value} kb/s`;
}
