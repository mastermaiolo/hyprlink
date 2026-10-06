# Tarefa: adotar a nova GUI iced (HYPRLINK) no projeto HyprLink, ligada ao daemon real

Responde e escreve todos os textos de interface em português europeu (pt-PT).

## Contexto

- **Este repositório** é o HyprLink: daemon Rust no Linux (QUIC + mTLS na porta 7443; controlo em CBOR
  sobre stream bidirecional; dados em streams unidirecionais; PipeWire/GStreamer; `mic.rs`, `tap.rs`)
  e app Android em Kotlin. Pode já existir uma GUI antiga — descobre.
- **A nova GUI** está em `~/Projectos/iced/hyprlink-gui` (iced 0.14). Lê primeiro o `README.md` dela e as
  capturas em `docs/screenshots/`. Estrutura:
  - `theme.rs` (design system), `ui.rs` (componentes), `graphics.rs` (canvas), `views.rs` (8 páginas),
    `app.rs` (estado/mensagens/moldura), `tray.rs` (StatusNotifierItem com ksni, `iced::daemon`),
    `host.rs` (dados reais deste PC), `snapshot.rs` (JSON v1 para plugins de shell),
    `link/mod.rs` (contrato `Command`/`Event`/`trait Transport`), `link/mock.rs` (daemon simulado),
    `bin/hyprlinkctl.rs` (CLI).
- **Plugin do Noctalia v5** em `~/Projectos/noctalia-plugins/hyprlink` (Luau). Consome
  `hyprlinkctl watch --json` e chama `hyprlinkctl mic|tap|mirror|ping|clipboard|open … --json`.

## Objetivo

A nova GUI passa a ser a GUI oficial do HyprLink, a falar com o `hyprlinkd` real, **com fidelidade
visual total** às capturas de referência. O simulador continua disponível para trabalho de design.

## Regras que não se negociam

1. **Fidelidade visual.** Não alteres `theme.rs` (cores, fontes, escala, espaçamentos, estilos), `ui.rs`
   nem a composição das vistas, a não ser que eu peça. Funcionalidades novas usam os componentes
   existentes (`opener`, `subhead`, `stat`, `kv`, `setting`, `chip`, `btn`, `tag`…). Nada de cores ou
   tamanhos soltos.
2. **O daemon só envia dados.** Nunca frases para a UI, nunca strings formatadas. Unidades fixas:
   latência em ms (`f32`), níveis de áudio lineares 0–1 a ≥ 30 Hz, débito em kbps, RSSI em dBm,
   sensores em SI a ~20 Hz, nome do aparelho em bruto (a GUI põe em maiúsculas), certificado em
   hex/bytes (a GUI agrupa). Valor em falta = `None` (a GUI mostra "—"). Históricos ficam na GUI.
3. **Primeiro o outro lado.** Na GUI do PC, os dados do telemóvel vêm antes dos deste PC — no ecrã, no
   tray, na tooltip e na ordem dos campos do JSON. (Na app Android é o inverso.)
4. **Contrato JSON v1 é estável.** A referência é `src/snapshot.rs` e o teste golden `v1_shape`
   (`cargo test`); o plugin do Noctalia lê esses campos (`tests/run.sh` no plugin). Desconhecido = `null`.
   Só podes acrescentar campos; mudança incompatível = `v: 2` e aviso explícito.
5. **O tray fica**: `iced::daemon`, ksni, estados Passive (sem telemóvel) / Active / NeedsAttention
   (emparelhamento), menu e tooltip atuais.
6. **Não apagues nada** da GUI antiga (se existir) antes de haver paridade comprovada; a remoção é um
   commit separado e só depois de eu aprovar.

## Fases

### Fase 0 — Reconhecimento (sem escrever código)
Lê os dois projetos e entrega-me:
- mapa do daemon: estado interno, fontes de eventos, onde vivem mic/tap/mirror/sensores/presença/pairing;
- tabela `Command`/`Event` da GUI → equivalente real no daemon (existe / parcial / falta);
- o que falta do lado Android (ex.: estado do telemóvel — rede, armazenamento, memória, temperatura,
  ecrã, notificações, "a tocar" — que proponho como pacote `hyprland.phone.status`);
- GUI antiga: o que faz e o que a nova ainda não cobre;
- plano das fases seguintes, com riscos.
**Para aqui e espera pela minha aprovação.**

### Fase 1 — Workspace
- Workspace Cargo: `crates/hyprlink-proto` (tipos de `link/mod.rs` + `snapshot.rs`, serde),
  `crates/hyprlinkd` (o atual), `crates/hyprlink-gui` (a nova), `crates/hyprlinkctl`.
- No proto: `Packet.kind` deixa de ser `&'static str` (String ou enum de tipos de pacote);
  `Event::Notice(String)` passa a `enum Notice { PingReply{device, rtt_ms}, ClipboardSent(id),
  Paired(id), Revoked(id), MirrorStarted, … }` e a GUI escreve os textos.
- O simulador passa a feature `mock` (ou `HYPRLINK_MOCK=1`). Tudo compila e a GUI arranca em mock
  com aspeto idêntico às capturas.

### Fase 2 — IPC no daemon
- `ipc.rs`: socket Unix em `$XDG_RUNTIME_DIR/hyprlink.sock`, permissões 0600, frames CBOR com prefixo
  de tamanho (u32 big-endian). Mensagens: `Hello{proto_version}`, estado completo ao ligar,
  depois `Event`s; o cliente envia `Command`s. Vários clientes ao mesmo tempo.
- Coalescer eventos de alta frequência (níveis, sensores) por cliente para não acumular atraso.
- Ligar cada `Command` aos módulos existentes; o que ainda não existe responde com erro tipado
  (a GUI mostra-o, ex.: `hyprlink-speaker` = "por implementar").

### Fase 3 — GUI e hyprlinkctl contra o daemon real
- Trocar o `Transport` síncrono por uma `Subscription::run` assíncrona: liga, reconecta com backoff,
  entrega `Event`s e um canal para `Command`s.
- Estados de ligação visíveis com os componentes atuais: a ligar, daemon parado, versão incompatível,
  sem telemóvel (masthead + estados vazios das páginas + tray).
- `hyprlinkctl` com `Backend` real sobre o mesmo socket; CLI e JSON v1 inalterados.
- Single-instance: `hyprlinkctl open` foca a janela existente em vez de lançar outra instância.

### Fase 4 — Paridade e limpeza
- Checklist de paridade com a GUI antiga; depois da minha aprovação, remoção num commit à parte.
- systemd user unit para o `hyprlinkd`; `exec-once = hyprlink-gui --hidden` documentado.

## Verificação obrigatória em cada fase
- `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.
- Testes: round-trip CBOR e JSON dos tipos do proto; teste de integração do IPC (daemon com estado
  falso ↔ cliente real); teste golden do JSON v1 (campos e tipos da regra 4).
- Regressão visual: arranca a GUI em mock (Xvfb + `ICED_BACKEND=tiny-skia`, `HYPRLINK_SECTION=1..8`,
  `HYPRLINK_WINDOW_H` para a página inteira) e compara com `docs/screenshots/`. Diferenças fora das
  zonas com dados vivos têm de ser explicadas.
- Branch `feat/iced-gui`, commits pequenos por fase. No fim de cada fase: o que mudou, o que foi
  verificado (com output), o que ficou em aberto.
