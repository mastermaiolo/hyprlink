#!/usr/bin/env python3
"""Gera os perfis de grafo PipeWire (`pw-dump.json`) do EasyEffects.

A forma (objetos `Node`, `Link`, `Metadata`) é a de um `pw-dump` real com o
EasyEffects a processar uma stream (medido em 2026-10-08: easyeffects_sink →
ee_soe_spectrum → ee_soe_output_level → alsa_output…); só ficaram as
propriedades que a deteção lê e os nomes são genéricos. Os ficheiros gerados
estão versionados; `python3 gerar_pw.py` refá-los."""
import json, os, shutil

root = os.path.dirname(os.path.abspath(__file__))
ALSA = 'alsa_output.pci-0000_03_00.6.analog-stereo'
HDMI = 'alsa_output.pci-0000_03_00.1.hdmi-stereo'
BT = 'bluez_output.AA_BB_CC_01_02_03.1'  # MAC inventado: o doctor tem de o esconder
SPK = 'hyprlink-speaker'


def node(i, name, cls=None, state='idle', prio=None, group=None, **extra):
    p = {'node.name': name}
    if cls: p['media.class'] = cls
    if prio is not None: p['priority.session'] = prio
    if group: p['node.group'] = p['node.link-group'] = group
    p.update(extra)
    return {'id': i, 'type': 'PipeWire:Interface:Node', 'info': {'state': state, 'props': p}}


def link(i, a, b, state='active'):
    return {'id': i, 'type': 'PipeWire:Interface:Link',
            'info': {'output-node-id': a, 'input-node-id': b, 'state': state}}


def default(sink, source='alsa_input.pci-0000_03_00.6.analog-stereo'):
    m = [{'subject': 0, 'key': 'default.audio.sink', 'type': 'Spa:String:JSON', 'value': {'name': sink}},
         {'subject': 0, 'key': 'default.audio.source', 'type': 'Spa:String:JSON', 'value': {'name': source}}]
    return {'id': 40, 'type': 'PipeWire:Interface:Metadata', 'props': {'metadata.name': 'default'}, 'metadata': m}


def ee_nodes():
    return [
        node(89, 'easyeffects_sink', 'Audio/Sink', 'running', 0, 'ee_sink_group', **{'node.virtual': True}),
        node(90, 'easyeffects_source', 'Audio/Source/Virtual', 'suspended', 0, 'ee_source_group', **{'node.virtual': True}),
        node(99, 'ee_sie_output_level', group='ee_source_group'),
        node(104, 'ee_sie_spectrum', group='ee_source_group'),
        node(115, 'ee_soe_output_level', state='running', group='ee_sink_group'),
        node(120, 'ee_soe_spectrum', state='running', group='ee_sink_group'),
    ]


def ee_chain(dest_id, first=1):
    """app (128) → easyeffects_sink → spectrum → output_level → destino."""
    return [link(first, 128, 89), link(first + 1, 89, 120), link(first + 2, 120, 115), link(first + 3, 115, dest_id)]


APP = node(128, 'player', 'Stream/Output/Audio', 'running', **{'application.name': 'player', 'media.role': 'Music'})
ALSA_N = lambda state='running', prio=1009: node(65, ALSA, 'Audio/Sink', state, prio)
ALSA_IN = node(66, 'alsa_input.pci-0000_03_00.6.analog-stereo', 'Audio/Source', 'idle', 2009)

perfis = {}

# 1. O caso do Maggio: predefinido = easyeffects_sink; EE toca no alsa_output.
perfis['ee-predefinido'] = ([ALSA_N(), ALSA_IN, APP] + ee_nodes() + ee_chain(65), default('easyeffects_sink'), '2', None)

# 2. EE em bypass (mesmo grafo; o socket diz 1).
perfis['ee-bypass'] = (perfis['ee-predefinido'][0], default('easyeffects_sink'), '1', None)

# 3. EE a tocar noutro sink (Bluetooth) com «Usar predefinido» desligado; o predefinido do sistema é o alsa_output.
perfis['ee-saida-diferente'] = (
    [ALSA_N('idle'), ALSA_IN, node(70, BT, 'Audio/Sink', 'running', 1010), APP] + ee_nodes() + ee_chain(70),
    default(ALSA), '2',
    '[StreamOutputs]\nuseDefaultOutputDevice=false\noutputDevice=' + BT + '\n')

# 4. EE aberto mas sem streams a tocar: sem ligações para o destino (o EE desliga os filtros por inatividade).
perfis['ee-ocioso'] = ([ALSA_N('idle'), ALSA_IN] + ee_nodes()[:2] + [n for n in ee_nodes()[2:]], default('easyeffects_sink'), '2', None)
perfis['ee-ocioso'][0][2:] = [dict(n, info=dict(n['info'], state='idle')) if n['id'] in (89, 115, 120) else n for n in perfis['ee-ocioso'][0][2:]]

# 5. Sem EasyEffects: a app toca direto no alsa_output, que é o predefinido.
perfis['sem-ee'] = ([ALSA_N(), ALSA_IN, APP], default(ALSA), None, None)
perfis['sem-ee'] = (perfis['sem-ee'][0] + [link(1, 128, 65)], default(ALSA), None, None)

# 6. Modo coluna com o EE a seguir o predefinido (hyprlink-speaker): tudo bem.
SPK_N = node(80, SPK, 'Audio/Sink', 'running', 1000)
perfis['ee-coluna-ok'] = ([ALSA_N('idle'), ALSA_IN, SPK_N, APP] + ee_nodes() + ee_chain(80), default(SPK), '2', None)

# 7. Modo coluna, mas o EE continua a tocar no alsa_output («Usar predefinido» desligado): a coluna fica muda.
perfis['ee-coluna-desviada'] = ([ALSA_N(), ALSA_IN, SPK_N, APP] + ee_nodes() + ee_chain(65), default(SPK), '2',
                                 '[StreamOutputs]\nuseDefaultOutputDevice=false\noutputDevice=' + ALSA + '\n')

# 8. Modo coluna sem EE, com uma stream a tocar noutro sink (não no hyprlink-speaker).
perfis['coluna-desviada-sem-ee'] = ([ALSA_N(), ALSA_IN, SPK_N, APP, link(1, 128, 65)], default(SPK), None, None)

for nome, (objs, meta, bypass, rc) in perfis.items():
    d = f'{root}/{nome}'
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    json.dump(list(objs) + [meta], open(f'{d}/pw-dump.json', 'w'), indent=1)
    open(f'{d}/pw-dump.json', 'a').write('\n')
    if bypass is not None:
        open(f'{d}/bypass.txt', 'w').write(bypass + '\n')
    if rc is not None:
        open(f'{d}/easyeffectsrc', 'w').write(rc)
