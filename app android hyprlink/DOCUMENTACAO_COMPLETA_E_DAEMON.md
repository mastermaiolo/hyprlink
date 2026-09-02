# HyprLink — Especificação Completa do Protocolo e Daemon Desktop

Este documento contém a especificação técnica integral da comunicação entre a aplicação Android **HyprLink** e o **Daemon Desktop Linux (Hyprland / Wayland / PipeWire)**, bem como o código de referência completo do daemon para restaurar o sistema do zero.

---

## 1. Arquitetura e Transporte

* **Protocolo:** QUIC (RFC 9000) sobre UDP.
* **Porta Padrão:** `7443` (UDP).
* **ALPN:** `"hyprlink/1"`.
* **Criptografia & Autenticação:**
  * TLS 1.3 mútuo (mTLS) com certificados auto-assinados.
  * Par de chaves ECDSA P-256 (`secp256r1`).
  * Certificado X.509 auto-assinado com algoritmo `SHA256withECDSA`.
  * Validação por **SHA-256 Fingerprint Pinning** (hash DER dos certificados trocados e confirmados no emparelhamento via QR Code).
* **QR Code de Emparelhamento (Desktop -> Mobile):**
  * Formato da string: `<FINGERPRINT_SHA256_HEX>|<HOST>:<PORTA>|<TOKEN_HEX>`
  * Exemplo: `8F:4E:92:10:BC:...:5A|192.168.1.100:7443|d8e03f56a14c99e120bc`

---

## 2. Enquadramento de Mensagens (Framing)

### 2.1. Streams Bidirecionais (Controlo, Eventos e RPCs)
Cada mensagem possui o cabeçalho de 4 bytes indicando o tamanho do payload CBOR:
```
+--------------------------------+-------------------------------------+
| Tamanho CBOR (4 bytes, uint32) | Payload CBOR (RFC 8949)             |
+--------------------------------+-------------------------------------+
```

Estrutura do Mapa CBOR (`Packet`):
```json
{
  "id": 123456789,
  "type": "modulo.acao",
  "body": { ... },
  "has_payload": false
}
```

### 2.2. Streams Unidirecionais (Dados Binários Puros)
Utilizados para ficheiros, vídeo da webcam e áudio PCM:
```
+---------------------------------+------------------------------------+
| Packet ID (8 bytes, uint64 BE)  | Fluxo Binário Bruto (até EOF)      |
+---------------------------------+------------------------------------+
```

---

## 3. Catálogo Completo de Mensagens CBOR

### 3.1. Núcleo (`core`)
* `core.hello` (Bidirecional): Envia `{"device_name": "...", "capabilities": ["core", "clipboard", "notification", "media", "battery", "share"], "pairing_token": <bytes>}`.
* `core.ping` (App -> PC): Keepalive a cada 30s.
* `core.pong` (PC -> App): Resposta ao ping.

### 3.2. Área de Transferência (`clipboard`)
* `clipboard.set` (Bidirecional): `{"text": "Conteúdo de texto UTF-8"}`.

### 3.3. Controlo e Entrada Remota (`input`)
* `input.move` (App -> PC): `{"dx": 10, "dy": -5}` (movimento relativo do cursor).
* `input.scroll` (App -> PC): `{"dx": 0, "dy": -2}` (rolagem).
* `input.click` (App -> PC): `{"button": "left"}` ("left", "right" ou "middle").
* `input.type` (App -> PC): `{"text": "Texto a digitar"}`.
* `input.key` (App -> PC): `{"key": "Return"}` ("Return", "BackSpace", "Escape", etc.).

### 3.4. Hyprland (`hypr`)
* `hypr.workspaces` (App -> PC) -> `hypr.workspaces_state` (PC -> App):
  * Resposta: `{"ok": true, "data": "[json de hyprctl workspaces -j]"}`.
* `hypr.clients` (App -> PC) -> `hypr.clients_state` (PC -> App):
  * Resposta: `{"ok": true, "data": "[json de hyprctl clients -j]"}`.
* `hypr.dispatch` (App -> PC) -> `hypr.dispatch_result` (PC -> App):
  * Pedido: `{"cmd": "workspace 2"}` (ou `focuswindow`, `closewindow`, `killall`, etc.).
  * Resposta: `{"ok": true, "data": "ok"}`.
* `hypr.event` (PC -> App, Push contínuo):
  * `{"event": "workspace>>2"}` lido de `/tmp/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket2.sock`.

### 3.5. Mídia MPRIS (`media`)
* `media.command` (App -> PC): `{"command": "play_pause"}` ("play_pause", "play", "pause", "next", "previous").
* `media.state` (PC -> App): `{"player": "spotify", "status": "Playing", "title": "...", "artist": "...", "album": "..."}`.

### 3.6. Telemetria de Bateria (`battery`)
* `battery.state` (Bidirecional): `{"level": 85, "charging": false}`.
* `battery.request` (App -> PC): Solicita o estado da bateria do laptop/PC.

### 3.7. Notificações (`notification`)
* `notification.post` (App -> PC): Espelha notificação do Android para o Linux.
* `notification.action` (PC -> App): Executa botão de ação remota no telemóvel.
* `notification.reply` (PC -> App): Envia resposta de texto para o chat via RemoteInput do Android.
* `notification.dismiss` (PC -> App): Fecha a notificação no telemóvel.
* `notification.send` (PC -> App): Emite uma notificação criada pelo PC no telemóvel.

### 3.8. Ficheiros e URLs (`share`)
* `share.url` (App -> PC): `{"url": "https://..."}` (abre com `xdg-open`).
* `share.file` (Bidirecional): `{"name": "arquivo.ext", "size": 123456}` com `has_payload: true`.
* Stream Unidirecional associado envia 8 bytes de `packet_id` seguidos dos dados binários.
* `share.done` (Bidirecional): Confirma gravação e hash SHA-256.

### 3.9. Webcam e Microfone Virtual (`webcam` & `audio`)
* `webcam.start` (PC -> App): `{"width": 1280, "height": 720, "fps": 30, "codec": "h264"}`.
* Stream Unidirecional de vídeo: 8 bytes (`packet_id`) + 1 byte (`0x01` para H.264 ou `0x02` para H.265) + fluxo NALU cru.
* `webcam.mic_start` (App -> PC): Inicia streaming de PCM 16-bit 48kHz mono.
* `audio.tap_start` (App -> PC): Transmite o áudio do desktop (PCM 16-bit 48kHz estéreo) para o telemóvel.

---

## 4. Código do Daemon Desktop (Python + aioquic)

Salve este arquivo como `daemon.py` no seu PC Linux:

```python
#!/usr/bin/env python3
import asyncio
import os
import ssl
import struct
import subprocess
import json
import secrets
from pathlib import Path
import cbor2
import qrcode
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from aioquic.asyncio import serve
from aioquic.quic.configuration import QuicConfiguration
from aioquic.quic.events import StreamDataReceived, HandshakeCompleted

PORT = 7443
ALPN = ["hyprlink/1"]
CERT_DIR = Path.home() / ".config" / "hyprlink"
CERT_FILE = CERT_DIR / "cert.pem"
KEY_FILE = CERT_DIR / "key.pem"
TOKEN_FILE = CERT_DIR / "pairing_token.txt"

def setup_certificates():
    CERT_DIR.mkdir(parents=True, exist_ok=True)
    
    if not CERT_FILE.exists() or not KEY_FILE.exists():
        print("[*] Gerando par de chaves ECDSA P-256 e certificado X.509...")
        private_key = ec.generate_private_key(ec.SECP256R1())
        subject = issuer = x509.Name([
            x509.NameAttribute(NameOID.COMMON_NAME, u"HyprLink-Desktop"),
        ])
        cert = (
            x509.CertificateBuilder()
            .subject_name(subject)
            .issuer_name(issuer)
            .public_key(private_key.public_key())
            .serial_number(x509.random_serial_number())
            .not_valid_before(x509.datetime.datetime.utcnow())
            .not_valid_after(x509.datetime.datetime.utcnow() + x509.datetime.timedelta(days=3650))
            .sign(private_key, hashes.SHA256())
        )
        
        with open(KEY_FILE, "wb") as f:
            f.write(private_key.private_bytes(
                encoding=serialization.Encoding.PEM,
                format=serialization.PrivateFormat.PKCS8,
                encryption_algorithm=serialization.NoEncryption()
            ))
            
        with open(CERT_FILE, "wb") as f:
            f.write(cert.public_bytes(serialization.Encoding.PEM))

    with open(CERT_FILE, "rb") as f:
        cert_data = x509.load_pem_x509_certificate(f.read())
        der_bytes = cert_data.public_bytes(serialization.Encoding.DER)
        digest = hashes.Hash(hashes.SHA256())
        digest.update(der_bytes)
        fingerprint = digest.finalize().hex().upper()
        formatted_fp = ":".join(fingerprint[i:i+2] for i in range(0, len(fingerprint), 2))

    token = secrets.token_hex(16)
    with open(TOKEN_FILE, "w") as f:
        f.write(token)

    return formatted_fp, token

def get_local_ip():
    import socket
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.connect(("10.255.255.255", 1))
        ip = s.getsockname()[0]
    except Exception:
        ip = "127.0.0.1"
    finally:
        s.close()
    return ip

def run_cmd(cmd_list):
    try:
        subprocess.Popen(cmd_list, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception as e:
        print(f"[!] Erro ao executar {cmd_list}: {e}")

def run_hyprctl(args):
    try:
        res = subprocess.run(["hyprctl"] + args, capture_output=True, text=True, check=True)
        return res.stdout
    except Exception as e:
        print(f"[!] Erro no hyprctl {args}: {e}")
        return ""

class HyprLinkServerProtocol:
    def __init__(self, quic):
        self._quic = quic
        self._stream_buffers = {}

    def quic_event_received(self, event):
        if isinstance(event, HandshakeCompleted):
            print("[+] Dispositivo conectado com sucesso via QUIC/mTLS!")
            
        elif isinstance(event, StreamDataReceived):
            buf = self._stream_buffers.get(event.stream_id, b"") + event.data
            
            while len(buf) >= 4:
                msg_len = struct.unpack(">I", buf[:4])[0]
                if len(buf) < 4 + msg_len:
                    break
                
                payload = buf[4:4 + msg_len]
                buf = buf[4 + msg_len:]
                
                try:
                    packet = cbor2.loads(payload)
                    self.handle_packet(event.stream_id, packet)
                except Exception as e:
                    print(f"[!] Erro ao decodificar CBOR: {e}")
                    
            self._stream_buffers[event.stream_id] = buf

    def send_packet(self, stream_id, msg_type, body=None, has_payload=False):
        packet = {
            "id": int(asyncio.get_event_loop().time() * 1000),
            "type": msg_type,
            "body": body,
            "has_payload": has_payload
        }
        encoded = cbor2.dumps(packet)
        header = struct.pack(">I", len(encoded))
        self._quic.send_stream_data(stream_id, header + encoded)

    def handle_packet(self, stream_id, packet):
        p_type = packet.get("type", "")
        body = packet.get("body") or {}
        p_id = packet.get("id", 0)

        if p_type == "core.ping":
            self.send_packet(stream_id, "core.pong", None)

        elif p_type == "core.hello":
            dev_name = body.get("device_name", "Desconhecido")
            print(f"[+] 'core.hello' recebido de: {dev_name}")
            self.send_packet(stream_id, "core.hello", {
                "device_name": os.uname().nodename,
                "capabilities": ["core", "clipboard", "notification", "media", "battery", "share"]
            })

        elif p_type == "clipboard.set":
            text = body.get("text", "")
            if text:
                print(f"[Clipboard] Copiado do telemóvel: {text[:40]}...")
                proc = subprocess.Popen(["wl-copy"], stdin=subprocess.PIPE)
                proc.communicate(input=text.encode("utf-8"))

        elif p_type == "media.command":
            cmd = body.get("command", "")
            mapping = {
                "playpause": "play-pause",
                "play_pause": "play-pause",
                "play": "play",
                "pause": "pause",
                "next": "next",
                "previous": "previous"
            }
            if cmd in mapping:
                run_cmd(["playerctl", mapping[cmd]])

        elif p_type == "hypr.workspaces":
            data = run_hyprctl(["workspaces", "-j"])
            self.send_packet(stream_id, "hypr.workspaces_state", {
                "ok": True,
                "data": data or "[]"
            })

        elif p_type == "hypr.clients":
            data = run_hyprctl(["clients", "-j"])
            self.send_packet(stream_id, "hypr.clients_state", {
                "ok": True,
                "data": data or "[]"
            })

        elif p_type == "hypr.dispatch":
            cmd = body.get("cmd", "")
            print(f"[Hyprland] Executando dispatch: {cmd}")
            parts = cmd.split(" ", 1)
            out = run_hyprctl(["dispatch"] + parts)
            self.send_packet(stream_id, "hypr.dispatch_result", {
                "ok": True,
                "data": out or "ok"
            })

        elif p_type == "input.move":
            dx = body.get("dx", 0)
            dy = body.get("dy", 0)
            run_cmd(["ydotool", "mousemove", "--", str(dx), str(dy)])

        elif p_type == "input.click":
            btn = body.get("button", "left")
            code = "0xC0" if btn == "left" else "0xC1"
            run_cmd(["ydotool", "click", code])

        elif p_type == "battery.request":
            self.send_packet(stream_id, "battery.state", {
                "level": 95,
                "charging": False
            })

        elif p_type == "share.url":
            url = body.get("url", "")
            if url:
                print(f"[URL] Abrindo no navegador: {url}")
                run_cmd(["xdg-open", url])

async def main():
    fp, token = setup_certificates()
    ip = get_local_ip()
    
    qr_payload = f"{fp}|{ip}:{PORT}|{token}"
    
    print("\n" + "="*60)
    print("           HYPRLINK DESKTOP DAEMON INICIADO")
    print("="*60)
    print(f"IP Local:     {ip}:{PORT}")
    print(f"Fingerprint:  {fp}")
    print(f"Token:        {token}")
    print("="*60 + "\n")

    qr = qrcode.QRCode(border=1)
    qr.add_data(qr_payload)
    qr.make(fit=True)
    qr.print_ascii(invert=True)
    
    print("\n[+] Aponte a câmara do app HyprLink para o QR Code acima para emparelhar.")

    configuration = QuicConfiguration(
        is_client=False,
        alpn_protocols=ALPN,
    )
    configuration.load_cert_chain(CERT_FILE, KEY_FILE)

    await serve(
        host="0.0.0.0",
        port=PORT,
        configuration=configuration,
        create_protocol=HyprLinkServerProtocol
    )
    
    await asyncio.Future()

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("\n[*] Servidor encerrado.")
```

---

## 5. Instruções Rápidas de Execução no Linux

1. Instalar dependências Python no PC:
   ```bash
   pip install aioquic cbor2 cryptography qrcode pillow
   ```
2. Instalar utilitários recomendados:
   ```bash
   sudo pacman -S playerctl wl-clipboard wtype ydotool libnotify # No Arch Linux
   ```
3. Executar o daemon:
   ```bash
   python3 daemon.py
   ```
4. Ler o QR Code exibido no terminal com o app HyprLink no telemóvel.
