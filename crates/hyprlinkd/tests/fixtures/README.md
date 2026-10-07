# Perfis de teste (`compat.rs`)

Cada pasta imita uma máquina: `sys/`, `proc/`, `etc/`, `dev/` (árvores falsas),
`tools.txt` (executáveis presentes, um por linha), `hypr.txt` (`lua`, `classic`
ou `absent`: como responde o `hyprctl dispatch`) e `vars.txt` (`CHAVE=valor`).
Os dados pessoais (`zeca`, `pc-do-zeca`, `/home/zeca`) são inventados de
propósito: o teste de redação garante que não saem no relatório.

| Perfil | Imita |
|---|---|
| `amd-so` | portátil AMD (k10temp, amdgpu), PipeWire, Noctalia v5, Hyprland Lua |
| `intel-so` | Intel (coretemp, i915), kernel lts, sem shell, Hyprland clássico |
| `amd-nvidia-hibrido` | AMD + NVIDIA, Ryoku, bateria no limite de carga |
| `nvidia-desktop` | desktop NVIDIA, PulseAudio, sem bateria, só `acpitz` |
| `desktop-sem-bateria` | desktop AMD sem `power_supply`, Caelestia |
| `duas-baterias` | duas baterias do sistema + a de um rato (`scope=Device`), Noctalia v4 |

Para os refazer: `python3 gerar.py` (sobrescreve as pastas-perfil);
os ficheiros gerados estão versionados.

## Grafos do PipeWire (EasyEffects, `gerar_pw.py`)

Pastas com `pw-dump.json` (+ `bypass.txt`, resposta de `get_global_bypass`: `1`
ligado, `2` desligado; + `easyeffectsrc`, só quando existe): `ee-predefinido`
(o caso do Maggio: predefinido = `easyeffects_sink`, ele toca no `alsa_output`),
`ee-saida-diferente` (toca num sink Bluetooth, predefinido do sistema = `alsa_output`,
«Usar predefinido» desligado), `ee-bypass`, `ee-ocioso` (sem ligações para o sink
real), `sem-ee`, `ee-coluna-ok`, `ee-coluna-desviada`, `coluna-desviada-sem-ee`.
A forma vem de um `pw-dump` real; o MAC do sink Bluetooth é inventado.
