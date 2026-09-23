import type { PublicationSource } from '../api/types/PublicationSource';

export type { PublicationSource };

/**
 * A chave do produto desde o ADR-0038.
 *
 * A unidade deixou de ser a pessoa e passou a ser a **publicação**: o par
 * (pessoa, fonte). Quem transmite a tela e a câmera ao mesmo tempo aparece como
 * dois ladrilhos, com foco, volume, qualidade e "sair da tela" próprios.
 *
 * O id é texto, e não um objeto, porque ele é chave de `Record`, de `key` de
 * React e de `Map` — e comparar objetos em qualquer um dos três é o tipo de
 * defeito que só aparece quando a segunda fonte entra no ar.
 *
 * O separador é `:` porque não ocorre em UUID nem em id do Discord, que são as
 * duas coisas que aparecem do lado esquerdo.
 */
export type PublicationId = string & { readonly __publication?: unique symbol };

const SEPARATOR = ':';

export function publicationId(ownerId: string, source: PublicationSource): PublicationId {
  return `${ownerId}${SEPARATOR}${source}`;
}

/** O dono e a fonte de volta, ou `null` para um id que não é de publicação. */
export function parsePublicationId(
  id: PublicationId,
): { ownerId: string; source: PublicationSource } | null {
  const cut = id.lastIndexOf(SEPARATOR);
  if (cut <= 0) {
    return null;
  }
  const source = id.slice(cut + 1);
  if (source !== 'screen' && source !== 'camera') {
    return null;
  }
  return { ownerId: id.slice(0, cut), source };
}

export function ownerOfPublication(id: PublicationId): string {
  return parsePublicationId(id)?.ownerId ?? id;
}

export function sourceOfPublication(id: PublicationId): PublicationSource {
  return parsePublicationId(id)?.source ?? 'screen';
}

/**
 * A ordem em que as fontes de uma pessoa aparecem, sempre a mesma.
 *
 * Tela antes de câmera porque é a tela que se veio ver; e fixa porque ordem que
 * depende da chegada faz a grade remexer quando alguém liga a câmera.
 */
export const SOURCE_ORDER: readonly PublicationSource[] = ['screen', 'camera'];

/** Como a interface chama cada fonte, para leitor de tela e rótulo curto. */
export function sourceLabel(source: PublicationSource): string {
  return source === 'camera' ? 'Câmera' : 'Tela';
}
