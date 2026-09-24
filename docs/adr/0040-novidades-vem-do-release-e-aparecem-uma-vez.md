# ADR-0040 — As novidades vêm do release no GitHub e aparecem uma vez, depois de atualizar

- **Status:** Aceito
- **Data:** 2026-09-23
- **Decorre de:** RF-28 (atualização assinada pelo GitHub Releases)

## Contexto

O aplicativo se atualiza sozinho (RF-28), e quem é atualizado não tem como saber o que mudou.
Isso pesou na 2.0.1: ela muda um comportamento — o aviso sonoro se cala enquanto se transmite a
tela inteira com áudio — que, sem explicação, parece defeito.

As notas de cada versão já existem: são escritas na página do release no GitHub, que é criado
como rascunho pelo `release.yml`, editado à mão e só então publicado.

## Decisão

1. **O texto vem do release publicado no GitHub, buscado pela tag da versão em execução**
   (`GET /repos/gbrlevi/ldktela/releases/tags/v{versão}`), na primeira vez que ela abre.
   A fonte é a mesma página que a pessoa encontraria no navegador, escrita uma vez só.

2. **Aparece uma vez, e só depois de uma atualização.** O core guarda a última versão cujas
   novidades foram vistas, num arquivo no diretório de dados do aplicativo. O modal aparece
   quando a versão em execução é mais nova que a guardada. Instalação nova não mostra nada.

3. **Sem registro guardado, o que decide é o cofre.** A primeira versão com este recurso não tem
   como saber se veio de uma atualização. Se já havia um refresh token no cofre ao abrir, a
   pessoa usava uma versão anterior, e o modal aparece. Se não havia, é instalação nova: a versão
   é só registrada.

4. **A versão só é marcada como vista quando o modal é fechado.** O aplicativo abre direto na
   bandeja, e o modal pode ficar montado numa janela que ninguém abriu. Sem rede, ou com o GitHub
   fora do ar, nada é marcado, e a próxima abertura tenta de novo. Um release sem texto, ou uma
   versão sem release (build de desenvolvimento), marca a versão como vista e não mostra nada.

5. **O markdown é interpretado por um leitor nosso, que produz elementos React e nunca HTML.**
   Ele entende o subconjunto que as notas usam: títulos, parágrafos, listas, citação, bloco de
   código, negrito, itálico e código em linha. Um link aparece como o próprio texto. O `marked`
   saiu na S1 e não volta para isto, e HTML vindo da rede nunca é injetado no WebView.

6. **O CSP ganha `https://api.github.com` em `connect-src`**, no `tauri.conf.json` versionado. O
   `release-config.mjs` acrescenta os servidores de produção a esse valor, então a liberação chega
   ao pacote sem outro passo.

## Consequências

- **O texto do release passa a ser interface do produto.** Ele aparece dentro do aplicativo,
  em português, para quem usa. Escreva-o para essas pessoas, não para quem mantém o código.
  Um `# título` na primeira linha vira o título do modal.
- Publicar o release é o que libera as novidades. Um rascunho não é visível pela API sem
  autenticação, e isso é o certo: o atualizador também só entrega o que foi publicado.
- A API do GitHub sem autenticação permite 60 pedidos por hora por IP. Um pedido por atualização
  por máquina fica muito longe disso.
- Quem instala uma versão nova pelo `.msi`, fora do atualizador, também vê as novidades. O que
  conta é a versão guardada ter mudado, não o caminho da instalação.

## Alternativas rejeitadas

- **As notas do `latest.json` (`update.body`).** O `tauri-action` preenche esse campo com o
  `releaseBody` da build, que é vazio aqui: o texto é escrito depois, no rascunho, e editar o
  release não reescreve o `latest.json` já enviado. Além disso, `update.body` só existe antes de
  instalar, quando ainda não há o que mostrar.
- **Guardar as notas antes de instalar e mostrá-las no reinício.** Não cobre a atualização que
  traz este recurso (a versão anterior não tem o código que guarda), nem a instalação pelo `.msi`.
- **Embutir as notas no binário.** A build acontece antes de o texto ser escrito.
- **`localStorage` para a versão vista.** Proibido pelo `CLAUDE.md` §2.8, e não sobrevive a uma
  limpeza de dados do WebView2, o que faria as novidades reaparecerem sem motivo.
- **Uma biblioteca de markdown com saída em HTML.** Seria a única dependência nova do recurso, e
  injetaria HTML vindo da rede no WebView.
