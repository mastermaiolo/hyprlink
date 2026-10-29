# hyprlinkctl

CLI do HyprLink para scripts, atalhos e plugins de barra (Noctalia, Waybar…).
Um binário, duas metades:

- **Texto** (sem `--json`): o protocolo antigo `ok …` / `erro: …` pelo
  `$XDG_RUNTIME_DIR/hyprlink/cmd.sock` (`src/legacy.rs`). É usado pelo `contrib/`:
  `status ping mic tap speaker headset send dispatch lock notif url phone-url
  phone-app phone-media`.
- **JSON v1** (`--json`, `watch`, `clipboard`, `pair`, `mirror`, `ws`, `open`, `find`): fala com o
  `hyprlinkd` real pelo `$XDG_RUNTIME_DIR/hyprlink.sock` (`src/real.rs`). O formato do
  `watch` é o `Snapshot` de `hyprlink-proto` (`src/snapshot.rs`), uma linha por
  instantâneo.

```sh
hyprlinkctl watch --json [--interval MS]   # 1.ª linha em < 1 s; depois a cada 500 ms
hyprlinkctl status --json
hyprlinkctl ping --json
hyprlinkctl mic|tap|speaker on|off|toggle --json
hyprlinkctl ws <1-10> --json
hyprlinkctl find [sound|vibrate|torch|all] [--secs N] [--json]   # por omissão all, 30 s (5–60)
hyprlinkctl find stop [--json]
hyprlinkctl doctor [--json] [--no-dispatch]  # relatório de compatibilidade (só lê)
hyprlinkctl status --waybar                # {text,tooltip,class} para a Waybar (lê o status.json)
```

`find` espera até 5 s pelo `phone.find.state` do telemóvel: `{"ok":true,"find_phone":{"active":true,
"until_ms":…}}`. Erros: `offline` (sem telemóvel), `refused` (limite do daemon: 1 a cada 3 s,
10 por minuto), `no_reply` (a app não respondeu: versão antiga?). `find stop` nunca é limitado.

`status --waybar` não fala com o daemon: lê o `$XDG_RUNTIME_DIR/hyprlink/status.json`, sai
sempre com 0 e escapa `& < >` (a Waybar lê markup Pango). O `contrib/waybar-hyprlink.sh` usa-o
quando o `hyprlinkctl` instalado o tem, e senão faz o mesmo com `jq`.

## Daemon parado

- `watch --json` não termina: emite `{"v":1,"daemon":"down","link":"disconnected",…}`
  a cada intervalo e volta a tentar ligar.
- Os comandos de uma só vez respondem
  `{"ok":false,"error":"daemon_down","message":…}` e saem com código 1.
- `status --json` responde com o instantâneo `"daemon":"down"` (o mesmo formato do
  `watch`) e também sai com código 1.

## Simulador

`HYPRLINK_MOCK=1` (ou `--mock`) põe a metade JSON a falar com o simulador do
`hyprlink-proto` (feature `mock`, ligada por omissão). Os instantâneos dizem
`"daemon":"mock"` e os comandos guardam o estado em `$XDG_RUNTIME_DIR/hyprlink-mock.json`.
Serve para capturas de ecrã, para as fixtures do plugin do Noctalia e para testes.
Sem isso, o binário fala sempre com o daemon real.

## Testes

`cargo test -p hyprlinkctl`. O `tests/json_real.rs` corre o binário contra um daemon
falso no socket, com o daemon parado, e com `HYPRLINK_MOCK=1`.
