# Handoff: GUI desktop do HyprLink (Rust + iced)

## Visão geral
Redesenho da janela nativa do HyprLink no Linux (Hyprland/Wayland) para que use a mesma
linguagem visual do app Android já existente. Cobre dois estados de ligação e os dez
módulos abertos em detalhe.

O objetivo funcional do redesenho: a grade de módulos deixa de mostrar `OFF` hardcoded
(herança da Fase 2) e passa a refletir o estado real do daemon; cada módulo passa a ter
ecrã próprio, acessível por clique; CLIP, FILES e NOTIF ganham histórico.

## Sobre os ficheiros de design
Os ficheiros HTML deste pacote são **referências de design**, não código para copiar.
Foram feitos em HTML porque é o meio de prototipagem — o alvo é **Rust + iced 0.14**,
arquitetura Elm (`Message` → `update` → `view`), na janela nativa já existente
(`iced::application`, 900×800, `decorations: false`, `transparent: true`).

Não há tradução automática de CSS para iced. O documento `code-spec-iced.md` descreve
cada ecrã já em termos de widgets iced (`Row`, `Column`, `Container`, `Scrollable`),
com as medidas e cores literais. Use esse documento como fonte da verdade e as imagens
em `code-referencia-visual/` para confirmar proporções e hierarquia.

## Fidelidade
**Alta fidelidade.** Cores, tipografia, espaçamentos e raios são finais e estão listados
em `code-spec-iced.md` §1–§3. Os *dados* mostrados são de exemplo (nomes de ficheiros,
mensagens, percentagens) — devem vir do daemon.

## Ficheiros

| Ficheiro | O que é |
|---|---|
| `code-spec-iced.md` | Especificação de implementação: tokens, geometria, árvore de widgets por ecrã, enum `Message` |
| `code-tarefas.md` | Tarefas por módulo, o que é real vs. mock, decisões de design a não reverter |
| `code-referencia-visual/*.png` | Captura de cada ecrã (12 imagens) |
| `GUI Desktop.dc.html` | Protótipo original, todos os ecrãs numa página (abrir num browser; precisa do `support.js` ao lado) |
| `Referencia Visual.html` | Biblioteca de componentes e sistema de cor do app Android (origem da linguagem visual) |
| `prompt-ai-studio-android.md` | Especificação do app Android, para contexto da outra ponta do protocolo |

As imagens em `code-referencia-visual/` estão em 900×800 reais, 1:1 com a janela.

## Ordem de leitura sugerida
1. `code-README.md` (este ficheiro)
2. `code-referencia-visual/01-dashboard-conectado.png` e `02-dashboard-pareando.png`
3. `code-spec-iced.md` §1 (tokens), §3 (geometria) e §4 (dashboard)
4. `code-tarefas.md` para escolher por onde começar
