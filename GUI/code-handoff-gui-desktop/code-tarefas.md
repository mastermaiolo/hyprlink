# Tarefas e contexto — GUI desktop HyprLink

## Decisões de design a não reverter

1. **Um acento por estado, em toda a janela.** Verde ligado · âmbar a negociar/aviso ·
   vermelho **só** erro real ou ausência de ligação · cinza inativo. Não colorir a barra
   do dispositivo nem a moldura da janela por estado além do acento único já definido.
2. **Módulo não implementado é cinza, nunca vermelho.** O `OFF` vermelho atual é o
   principal defeito a corrigir: hoje sugere avaria onde só há uma funcionalidade
   desligada.
3. **Estado = ponto indicador + label.** Não codificar estado apenas por cor de fundo.
4. **A consola do desktop passa a colorir severidade** (`[+]` `[i]` `[!]` `[x]`), como
   a do Android. Hoje é tudo `TEXT_2`.
5. **Subtítulo de módulo mostra dado real**, não descrição genérica: "3 itens
   sincronizados" em vez de "área de transferência". A descrição genérica só aparece no
   estado a emparelhar, quando não há dados.
6. **Ao abrir um módulo a sidebar recolhe para a régua de 44px.** Nada de sidebar de
   296px + conteúdo apertado.
7. **Dark-only.** Sem tema claro, sem janela de configurações separada — `10 CONFIG` é
   um ecrã de módulo como os outros.
8. Uma só animação: o ponto de estado a respirar.

## O que é real hoje vs. o que a GUI precisa de passar a mostrar

| Módulo | Backend | GUI atual | A fazer |
|---|---|---|---|
| 01 CLIP | funciona | `OFF` vermelho | estado real + histórico persistido |
| 02 FILES | funciona (SFTP) | só o seletor de pasta | fila, progresso, histórico |
| 03 NOTIF | funciona | `OFF` vermelho | espelho ao vivo + histórico + filtros por app |
| 04 MEDIA | funciona (MPRIS) | `OFF` vermelho | transporte, capa, players detetados |
| 05 BATT | funciona | `OFF` vermelho | percentagem, série de 12 h, alertas |
| 06 CONTROL | funciona (IPC) | `OFF` vermelho | workspaces, atalhos, campo `hyprctl` |
| 07 AUDIO | funciona (tap) | `OFF` vermelho | mixer, VU, marca dos 100% |
| 08 WEBCAM | não | `OFF` vermelho | ecrã em repouso cinza + `iniciar stream` |
| 09 TRACK | não | `OFF` vermelho | ecrã em repouso cinza + sliders |
| 10 CONFIG | parcial | linha na sidebar | ecrã com rede, dispositivos, daemon |

Dados nos mockups que são de exemplo e têm de vir do daemon: nomes de ficheiros,
mensagens de notificação, percentagens, contagens de histórico, latência, fingerprint,
token, coordenadas do cursor, série da bateria.

## Ordem de trabalho sugerida

**Fase A — desbloquear o essencial (nada de novo backend)**
1. `theme.rs` com os tokens de `code-spec-iced.md` §1; corrigir os três valores de acento.
2. Ligar o estado real de cada módulo à linha da sidebar (fim do `OFF` hardcoded) e
   trocar o vermelho por cinza no caso de não implementado.
3. Colorir a severidade na consola.
4. Reescrever a linha de módulo com nº · glifo · título · subtítulo real · indicador ·
   chevron; validar que os glifos existem na mono do sistema (ver aviso em §5).

**Fase B — navegação e detalhe**
5. `Screen::Module(ModuleId)` + régua recolhida + `‹ VOLTAR`.
6. Painel de detalhe na coluna direita do dashboard (versão compacta do ecrã de módulo).
7. Ecrãs de leitura, sem escrita nova no daemon: BATT, CONTROL, MEDIA, CONFIG.

**Fase C — histórico**
8. Camada de persistência partilhada por CLIP, FILES e NOTIF: retenção de 7 dias
   (configurável em CONFIG), pesquisa por texto, "apagar todos os históricos".
   Decisão aberta: SQLite (`rusqlite`) ou ficheiro JSON append-only por dia. SQLite
   se a pesquisa tiver de ser rápida com milhares de entradas.
9. Ecrãs CLIP, FILES e NOTIF com as respetivas listas.

**Fase D — o que falta de backend**
10. Progresso de transferência exposto ao GUI (hoje só vai ao log): canal de eventos
    `TransferProgress { id, bytes, total, rate }`.
11. WEBCAM: decoder de frames → `iced::widget::image`.
12. TRACK: espelho da posição do cursor (via `hyprctl cursorpos` ou o próprio `uinput`).

## Decisões que ficam do lado do Rust

- **Glifos dos ícones.** `≋ ◔ ◈ ⧉ ▮` podem não existir na mono do sistema. Testar cedo;
  se falharem, embutir uma fonte de ícones ou desenhar em `canvas`. Não substituir por
  emoji.
- **`letter-spacing` não existe no iced 0.14.** Os labels de secção do protótipo dependem
  dele. Aceitar sem espaçamento ou espaçar manualmente na string — decidir uma vez.
- **Persistência do histórico** (ver Fase C).
- **Altura dinâmica de linha no histórico de NOTIF.** Confirmar que `Scrollable` +
  `Text` com envolvimento dá a altura certa sem cortar; o mockup mostra o rodapé fixo
  "N entradas mais antigas" exatamente porque a janela é fixa em 800px.
- **Scroll na consola**: manter o comportamento de seguir o fim, mas parar de seguir
  quando o utilizador rola para cima (é uma das reclamações já registadas no histórico
  de notificações do próprio mockup).

## Fora de âmbito

Tema claro · janela de configurações separada · layer-shell / HUD flutuante (a janela
continua normal, presa a uma workspace) · redimensionamento · efeitos de blur desenhados
à mão.
