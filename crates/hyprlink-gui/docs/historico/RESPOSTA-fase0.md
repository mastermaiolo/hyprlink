# Resposta à Fase 0 — decisões aprovadas e o que já mudou do lado da GUI

Bom reconhecimento. Antes das decisões: **arrumei o lado da GUI** em `~/Projectos/iced/hyprlink-gui`
(e o plugin em `~/Projectos/noctalia-plugins/hyprlink`) para resolver o que era meu. Volta a ler esses
dois diretórios antes da Fase 1.

## O que já mudou na GUI (não refaças)

- **Regra 2 cumprida no contrato** (`src/link/mod.rs`, futuro `hyprlink-proto`): `serde` Serialize +
  Deserialize em tudo; `Device` com `manufacturer/model/android/app_version` opcionais,
  `battery/rssi/latency_ms/addr: Option`, `fingerprint` em hex sem separadores, `paired_since` em
  segundos Unix; `PhoneStatus` estruturado (`network` enum, `cell_gen`, `carrier`, `wifi{ssid,rssi_dbm}`,
  bytes em vez de GB, `battery_temp_c`, `now_playing{position_ms,duration_ms}`; tudo opcional);
  `Packet.kind: String`; `Notice` é enum (`PingReply`, `ClipboardSent`, `Paired`, `Revoked`,
  `MirrorStarted`, `Failed{op, error: NotImplemented|Offline|Refused|Timeout}`);
  `PairingTicket{payload, code: Option, expires_in: Option}` com o payload real `fp|host:port|token`;
  `Levels{mic: Option, tap: Option}`; `Command::SetSpeakerMode(bool)` + `Event::SpeakerMode`.
- **Todos os textos para humanos estão em `src/fmt.rs`.** Os `label()` saíram do contrato.
- **Nomes de pacotes num só sítio:** `src/link/packets.rs`, separados em *reais* e *propostos*. Alinha-os com
  o PROTOCOL.md ali (um ficheiro) — mais nada na GUI tem nomes escritos à mão. Os `kdeconnect.*` e o mDNS
  desapareceram; o protocolo aparece como `hyprlink/1 · QUIC · mTLS`.
- **Coluna:** o «POR IMPLEMENTAR» saiu. O Retorno tem agora a definição **Modo coluna**
  (`hyprlink-speaker` como origem) e o `hyprlinkctl speaker on|off|toggle`.
- **Microfone sem medidor:** com `Levels.mic = None` a página mostra «—» e «sem medidor» em vez de inventar.
- **JSON v1 redefinido com `null`** (`src/snapshot.rs`). O v1 nunca foi publicado — o plugin ainda não
  estava ativo —, por isso fica v1, sem flags `*_known`. Há um teste golden (`cargo test`) com os campos e
  tipos. O plugin já lê este formato; os testes dele correm com `tests/run.sh` (31 verificações).

## Decisões

1. **Módulos sem página — (a), com a estrutura definida por mim.** A regra 1 continua; isto é a exceção
   autorizada. Índice novo (máximo 10 secções numeradas; teclas 1–9 e 0):

   | §  | Secção              | Conteúdo (telemóvel primeiro, PC depois) |
   |----|---------------------|------------------------------------------|
   | 01 | Capa                | como está |
   | 02 | Dispositivos        | + histórico de bateria do telemóvel (12 h) e alertas (baixa/carga completa) na ficha |
   | 03 | Secretária          | + Atalhos personalizados, `hyprctl dispatch` livre, janela ativa; + Trackpad (sensibilidade, scroll, aceleração, inversão, teclado virtual) |
   | 04 | Câmara & Ecrã       | Câmara funcional já (resolução, fps, codec, `/dev/video42`, teste de rede); Ecrã como segundo modo, desativado com «proposto» |
   | 05 | Áudio               | Volumes, modo de toque e não-incomodar do telemóvel **em cima**; Microfone e Retorno/Coluna; Mixer do PC no fim |
   | 06 | Notificações        | Do telemóvel: lista, filtro por app, pesquisa |
   | 07 | Partilha            | Clipboard (histórico, pesquisa, fixar) + Ficheiros (enviar, progresso, cancelar, histórico, pasta) |
   | 08 | Multimédia          | O que toca no telemóvel (quando houver) e MPRIS do PC ⏮ ⏯ ⏭ |
   | 09 | Sensores & Presença | Funde as duas; estado vazio «à espera do Android» até haver pacotes |
   | 10 | Diário              | como está |
   | —  | Definições          | No rodapé do rail, sem número: pasta de downloads, reiniciar o daemon, idioma |

   **As páginas novas são desenhadas no repositório da GUI e chegam-te em mock.** Tu não compões páginas
   novas; ligas as que chegarem. As Fases 1–2 não dependem disto e podem avançar já.

2. **Espelho — (a)+(b):** a §04 passa a «Câmara & Ecrã». A câmara (webcam real) é o modo ativo; o espelho
   de ecrã fica visível como segundo modo, desligado, «proposto — precisa de MediaProjection».
3. **Sensores, Presença e regras:** sim — `Notice::Failed{op, error: NotImplemented}` e estados vazios com os
   componentes existentes. Nada de dados inventados fora do mock.
4. **Nome do pacote: `phone.status`** (segue `módulo.ação`). Já está assim em `packets.rs`.
5. **JSON:** v1 com `null` — ver acima. Não há `v: 2` nem `*_known`.
6. **i18n:** só pt-PT nesta migração. As 5 línguas da GUI antiga ficam para uma fase própria, depois da
   paridade; os textos já estão concentrados em `fmt.rs` e nas vistas.

## Notas para as fases

- **Processos separados: sim.** `hyprlinkd` sem interface (systemd user unit); a GUI é o `iced::daemon` com o
  tray (`tray.rs`). O ksni e o truque `special:hyprlink` saem do daemon. Compatibilidade: durante a transição,
  `hyprlink-daemon` continua a existir como invólucro que arranca `hyprlinkd` e `hyprlink-gui --hidden`, para
  não partir `exec-once` antigos.
- **`hyprlinkctl` único:** sem `--json`, mantém os subcomandos e as respostas de texto antigas (`ok …` /
  `erro: …`) que o `contrib/` usa; com `--json`, o formato v1. O `bin/hyprlinkctl.rs` da GUI é a metade JSON.
- **Dados fáceis que já podes dar sem Android:** RTT com `quinn::Connection::rtt()`, débito com `stats()`,
  nível do microfone por RMS do PCM que o daemon já recebe (`Levels.mic`), `addr` com `remote_address()`.
- **Pareados offline:** grava em `paired_devices.json` o nome, fabricante, modelo, Android e data de
  pareamento recebidos no `core.hello`, para a lista de Dispositivos não ficar só com fingerprints.
- **Bugs que encontraste:** corrige-os na Fase 2, um commit cada, com teste quando der.
- **Regressão visual:** as capturas de referência foram atualizadas em `docs/screenshots/`.
