//! Tradução da GUI. Fica de fora o console de diagnóstico (log técnico
//! gerado pelo daemon inteiro via `push_log`, centenas de chamadas em todos
//! os módulos do backend — traduzir isso exigiria reescrever o daemon
//! inteiro, não só a GUI, e mistura termos de protocolo que não traduzem
//! bem de qualquer forma).
//!
//! ponytail: idioma é estado global de processo (`AtomicU8`), não passado
//! por parâmetro em cada função de tela — `view()` roda inteiro numa única
//! thread a cada frame (iced não renderiza duas telas ao mesmo tempo), então
//! não há corrida de verdade; a alternativa (passar `Lang` em ~30 funções
//! de widget) é muito mais código pra zero ganho real aqui.

use std::sync::atomic::{AtomicU8, Ordering};

pub use crate::config::Lang;

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::PtPt, Lang::PtBr, Lang::EnGb, Lang::EsEs, Lang::Zh];

    pub fn flag(self) -> &'static str {
        match self {
            Lang::PtPt => "🇵🇹",
            Lang::PtBr => "🇧🇷",
            Lang::EnGb => "🇬🇧",
            Lang::EsEs => "🇪🇸",
            Lang::Zh => "🇨🇳",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::PtPt => "PT-PT",
            Lang::PtBr => "PT-BR",
            Lang::EnGb => "EN-UK",
            Lang::EsEs => "ES-ES",
            Lang::Zh => "ZH",
        }
    }

    fn index(self) -> usize {
        match self {
            Lang::PtPt => 0,
            Lang::PtBr => 1,
            Lang::EnGb => 2,
            Lang::EsEs => 3,
            Lang::Zh => 4,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Chamado uma vez no início de `view()` — o resto da árvore de widgets só
/// chama `t()` sem precisar saber de `Lang`.
pub fn set_current(lang: Lang) {
    CURRENT.store(lang.index() as u8, Ordering::Relaxed);
}

fn current_index() -> usize {
    CURRENT.load(Ordering::Relaxed) as usize
}

/// Tabela (PT-BR, EN-UK, ES-ES, ZH), chaveada pelo texto em PT-PT usado no
/// código — se `t()` não achar a chave, devolve ela mesma (PT-PT é sempre
/// um fallback válido).
///
/// ponytail: busca linear numa tabela de ~150 entradas, chamada algumas
/// dezenas de vezes por frame — não é gargalo (frame só redesenha em
/// resposta a evento/tick, não a 60fps constante). Se um dia doer, vira
/// `HashMap` construído uma vez com `LazyLock`.
#[rustfmt::skip]
const TABLE: &[(&str, &str, &str, &str, &str)] = &[
    ("CLIP", "CLIP", "CLIP", "PORTAPAPELES", "剪贴板"),
    ("FILES", "FILES", "FILES", "ARCHIVOS", "文件"),
    ("NOTIFICAÇÕES", "NOTIFICAÇÕES", "NOTIFICATIONS", "NOTIFICACIONES", "通知"),
    ("MEDIA", "MEDIA", "MEDIA", "MULTIMEDIA", "媒体"),
    ("BATERIA", "BATERIA", "BATTERY", "BATERÍA", "电池"),
    ("CONTROL", "CONTROL", "CONTROL", "CONTROL", "控制"),
    ("AUDIO", "AUDIO", "AUDIO", "AUDIO", "音频"),
    ("WEBCAM", "WEBCAM", "WEBCAM", "CÁMARA WEB", "摄像头"),
    ("TRACK", "TRACK", "TRACK", "SEGUIMIENTO", "轨迹"),
    ("CONFIGURAÇÕES", "CONFIGURAÇÕES", "SETTINGS", "AJUSTES", "设置"),
    ("Aguardando conexão", "Aguardando conexão", "Waiting for connection", "Esperando conexión", "等待连接"),
    ("Nada sincronizado ainda", "Nada sincronizado ainda", "Nothing synced yet", "Nada sincronizado todavía", "尚未同步"),
    ("Recebe em {}", "Recebe em {}", "Receiving to {}", "Recibe en {}", "接收至 {}"),
    ("Telemóvel em {}%", "Celular em {}%", "Phone at {}%", "Móvil al {}%", "手机电量 {}%"),
    ("Workspace {}", "Workspace {}", "Workspace {}", "Espacio de trabajo {}", "工作区 {}"),
    ("Nenhuma notificação ainda", "Nenhuma notificação ainda", "No notifications yet", "Ninguna notificación todavía", "尚无通知"),
    ("Nenhum leitor ativo", "Nenhum leitor ativo", "No active player", "Ningún reproductor activo", "没有活动的播放器"),
    ("Aguardando bateria do telemóvel", "Aguardando bateria do celular", "Waiting for phone battery", "Esperando batería del móvil", "等待手机电量"),
    ("Hyprland IPC", "Hyprland IPC", "Hyprland IPC", "IPC de Hyprland", "Hyprland IPC"),
    ("Audio tap ativo", "Audio tap ativo", "Audio tap active", "Captura de audio activa", "音频监听已启用"),
    ("Mixer e audio tap", "Mixer e audio tap", "Mixer and audio tap", "Mezclador y captura de audio", "混音器与音频监听"),
    ("Stream ativo · /dev/video42", "Stream ativo · /dev/video42", "Stream active · /dev/video42", "Transmisión activa · /dev/video42", "流媒体已启用 · /dev/video42"),
    ("Câmara remota do PC", "Câmara remota do PC", "Remote PC camera", "Cámara remota del PC", "PC 远程摄像头"),
    ("Stream ativo — a escrever em /dev/video42, use como webcam em qualquer app (Chrome, OBS, etc.)", "Stream ativo — escrevendo em /dev/video42, use como webcam em qualquer app (Chrome, OBS, etc.)", "Stream active — writing to /dev/video42, use it as a webcam in any app (Chrome, OBS, etc.)", "Transmisión activa — escribiendo en /dev/video42, úsala como cámara web en cualquier app (Chrome, OBS, etc.)", "流媒体已启用——正在写入 /dev/video42，可在任意应用中作为摄像头使用（Chrome、OBS 等）"),
    ("Parado. Ao iniciar, o telemóvel é trazido pro primeiro plano e passa a filmar em {}@{}fps ({}).", "Parado. Ao iniciar, o celular é trazido pro primeiro plano e passa a filmar em {}@{}fps ({}).", "Stopped. When started, the phone is brought to the foreground and starts filming at {}@{}fps ({}).", "Detenido. Al iniciar, el móvil pasa a primer plano y empieza a filmar a {}@{}fps ({}).", "已停止。启动后，手机将被切换到前台并开始以 {}@{}fps（{}）拍摄。"),
    ("medindo…", "medindo…", "measuring…", "midiendo…", "测量中…"),
    ("Testando em 1920×1080@30 (baseline — não testa 2K/4K, ver nota abaixo) · {} · {}s restantes", "Testando em 1920×1080@30 (baseline — não testa 2K/4K, ver nota abaixo) · {} · {}s restantes", "Testing at 1920×1080@30 (baseline — doesn't test 2K/4K, see note below) · {} · {}s remaining", "Probando a 1920×1080@30 (base — no prueba 2K/4K, ver nota abajo) · {} · {}s restantes", "正在以 1920×1080@30 测试（基准——不测试 2K/4K，见下方说明）· {} · 剩余 {} 秒"),
    ("Sugestão: {} @ {}fps  ·  medido: {} Mbps na rede, {} núcleos de CPU", "Sugestão: {} @ {}fps  ·  medido: {} Mbps na rede, {} núcleos de CPU", "Suggestion: {} @ {}fps  ·  measured: {} Mbps on the network, {} CPU cores", "Sugerencia: {} @ {}fps  ·  medido: {} Mbps en la red, {} núcleos de CPU", "建议：{} @ {}fps · 实测：网络 {} Mbps，{} 个 CPU 核心"),
    ("último uso · {} · {}", "último uso · {} · {}", "last used · {} · {}", "último uso · {} · {}", "上次使用 · {} · {}"),
    ("Rato/teclado virtual pronto (uinput)", "Mouse/teclado virtual pronto (uinput)", "Virtual mouse/keyboard ready (uinput)", "Ratón/teclado virtual listo (uinput)", "虚拟鼠标/键盘就绪 (uinput)"),
    ("Permissões e dispositivos", "Permissões e dispositivos", "Permissions and devices", "Permisos y dispositivos", "权限与设备"),
    ("LINK ATIVO", "LINK ATIVO", "LINK ACTIVE", "ENLACE ACTIVO", "链接已连接"),
    ("CONECTANDO", "CONECTANDO", "CONNECTING", "CONECTANDO", "连接中"),
    ("AGUARDANDO PAREAMENTO", "AGUARDANDO PAREAMENTO", "WAITING TO PAIR", "ESPERANDO EMPAREJAMIENTO", "等待配对"),
    ("‹ VOLTAR", "‹ VOLTAR", "‹ BACK", "‹ VOLVER", "‹ 返回"),
    ("console de diagnóstico", "console de diagnóstico", "diagnostic console", "consola de diagnóstico", "诊断控制台"),
    ("copiar log", "copiar log", "copy log", "copiar registro", "复制日志"),
    ("APONTE A CÂMARA DO TELEMÓVEL", "APONTE A CÂMARA DO CELULAR", "POINT YOUR PHONE'S CAMERA", "APUNTA LA CÁMARA DEL MÓVIL", "用手机摄像头扫描"),
    ("Abra o HyprLink no Android e escaneie o código", "Abra o HyprLink no Android e escaneie o código", "Open HyprLink on Android and scan the code", "Abre HyprLink en Android y escanea el código", "在 Android 上打开 HyprLink 并扫描二维码"),
    ("FINGERPRINT", "FINGERPRINT", "FINGERPRINT", "HUELLA", "指纹"),
    ("HOST : PORTA", "HOST : PORTA", "HOST : PORT", "HOST : PUERTO", "主机 : 端口"),
    ("TOKEN", "TOKEN", "TOKEN", "TOKEN", "令牌"),
    ("copiar", "copiar", "copy", "copiar", "复制"),
    ("HISTÓRICO · {} NESTA SESSÃO", "HISTÓRICO · {} NESTA SESSÃO", "HISTORY · {} THIS SESSION", "HISTORIAL · {} EN ESTA SESIÓN", "历史记录 · 本次会话 {} 条"),
    ("{} no histórico desta sessão", "{} no histórico desta sessão", "{} in this session's history", "{} en el historial de esta sesión", "本次会话历史记录 {} 条"),
    ("Nada sincronizado ainda nesta sessão.", "Nada sincronizado ainda nesta sessão.", "Nothing synced yet this session.", "Nada sincronizado todavía en esta sesión.", "本次会话尚未同步任何内容。"),
    ("pesquisar…", "pesquisar…", "search…", "buscar…", "搜索…"),
    ("limpar", "limpar", "clear", "borrar", "清除"),
    ("fixar", "fixar", "pin", "fijar", "置顶"),
    ("fixado", "fixado", "pinned", "fijado", "已置顶"),
    ("PC → telemóvel", "PC → celular", "PC → phone", "PC → móvil", "电脑 → 手机"),
    ("telemóvel → PC", "celular → PC", "phone → PC", "móvil → PC", "手机 → 电脑"),
    ("Transferência de ficheiros sobre QUIC", "Transferência de arquivos sobre QUIC", "File transfer over QUIC", "Transferencia de archivos sobre QUIC", "基于 QUIC 的文件传输"),
    ("PASTA DE DESTINO", "PASTA DE DESTINO", "DESTINATION FOLDER", "CARPETA DE DESTINO", "目标文件夹"),
    ("escolher pasta", "escolher pasta", "choose folder", "elegir carpeta", "选择文件夹"),
    ("enviar ficheiro…", "enviar arquivo…", "send file…", "enviar archivo…", "发送文件…"),
    ("cancelar", "cancelar", "cancel", "cancelar", "取消"),
    ("recebendo", "recebendo", "receiving", "recibiendo", "接收中"),
    ("enviando", "enviando", "sending", "enviando", "发送中"),
    ("HISTÓRICO · {} TRANSFERÊNCIA(S)", "HISTÓRICO · {} TRANSFERÊNCIA(S)", "HISTORY · {} TRANSFER(S)", "HISTORIAL · {} TRANSFERENCIA(S)", "历史记录 · {} 次传输"),
    ("Nenhuma transferência ainda nesta sessão.", "Nenhuma transferência ainda nesta sessão.", "No transfers yet this session.", "Ninguna transferencia todavía en esta sesión.", "本次会话尚无传输记录。"),
    ("Sem \"pausar\" — o protocolo só permite continuar ou cancelar uma transferência em andamento.", "Sem \"pausar\" — o protocolo só permite continuar ou cancelar uma transferência em andamento.", "No \"pause\" — the protocol only allows continuing or cancelling a transfer in progress.", "Sin \"pausar\" — el protocolo solo permite continuar o cancelar una transferencia en curso.", "不支持“暂停”——协议只允许继续或取消进行中的传输。"),
    ("recebido", "recebido", "received", "recibido", "已接收"),
    ("enviado", "enviado", "sent", "enviado", "已发送"),
    ("falhou", "falhou", "failed", "falló", "失败"),
    (" · 1 a transferir", " · 1 a transferir", " · 1 transferring", " · 1 transfiriendo", " · 1 项传输中"),
    ("cancelado pelo usuário", "cancelado pelo usuário", "cancelled by user", "cancelado por el usuario", "用户已取消"),
    ("{} espelhada(s) nesta sessão", "{} espelhada(s) nesta sessão", "{} mirrored this session", "{} reflejada(s) en esta sesión", "本次会话已镜像 {} 条"),
    ("todas", "todas", "all", "todas", "全部"),
    ("Nenhuma notificação espelhada ainda nesta sessão.", "Nenhuma notificação espelhada ainda nesta sessão.", "No notifications mirrored yet this session.", "Ninguna notificación reflejada todavía en esta sesión.", "本次会话尚未镜像任何通知。"),
    ("MPRIS", "MPRIS", "MPRIS", "MPRIS", "MPRIS"),
    ("Nenhum leitor MPRIS ativo", "Nenhum leitor MPRIS ativo", "No active MPRIS player", "Ningún reproductor MPRIS activo", "没有活动的 MPRIS 播放器"),
    ("Volume, shuffle/repeat e lista de players ainda não existem no protocolo — só o essencial (título/artista/álbum + transporte) é real hoje.", "Volume, shuffle/repeat e lista de players ainda não existem no protocolo — só o essencial (título/artista/álbum + transporte) é real hoje.", "Volume, shuffle/repeat and player list don't exist in the protocol yet — only the essentials (title/artist/album + transport) are real today.", "Volumen, aleatorio/repetir y lista de reproductores todavía no existen en el protocolo — solo lo esencial (título/artista/álbum + transporte) es real hoy.", "音量、随机播放/循环和播放器列表尚未在协议中实现——目前只有基本信息（标题/艺术家/专辑+播放控制）是真实的。"),
    ("Telemetria do telemóvel e do PC", "Telemetria do celular e do PC", "Phone and PC telemetry", "Telemetría del móvil y del PC", "手机与电脑的电量遥测"),
    ("TELEMÓVEL", "CELULAR", "PHONE", "MÓVIL", "手机"),
    ("PC", "PC", "PC", "PC", "电脑"),
    ("a carregar", "carregando", "charging", "cargando", "充电中"),
    ("na bateria", "na bateria", "on battery", "en batería", "使用电池中"),
    ("nível conhecido, carregando desconhecido", "nível conhecido, carregando desconhecido", "level known, charging unknown", "nivel conocido, carga desconocida", "电量已知，充电状态未知"),
    ("aguardando dado real", "aguardando dado real", "waiting for real data", "esperando dato real", "等待真实数据"),
    ("ÚLTIMAS 12H", "ÚLTIMAS 12H", "LAST 12H", "ÚLTIMAS 12H", "过去 12 小时"),
    ("telemóvel, quando disponível", "celular, quando disponível", "phone, when available", "móvil, cuando disponible", "手机（如有数据）"),
    ("Sem histórico ainda nesta sessão — volte daqui a pouco.", "Sem histórico ainda nesta sessão — volte daqui a pouco.", "No history yet this session — check back soon.", "Sin historial todavía en esta sesión — vuelve en un rato.", "本次会话暂无历史数据——请稍后再来查看。"),
    ("ALERTAS NO DESKTOP", "ALERTAS NO DESKTOP", "DESKTOP ALERTS", "ALERTAS EN EL ESCRITORIO", "桌面提醒"),
    ("avisar abaixo de 20% (telemóvel)", "avisar abaixo de 20% (celular)", "warn below 20% (phone)", "avisar por debajo del 20% (móvil)", "低于 20% 时提醒（手机）"),
    ("avisar quando carregada a 100%", "avisar quando carregada a 100%", "warn when charged to 100%", "avisar al cargar al 100%", "充满 100% 时提醒"),
    ("ativo", "ativo", "on", "activo", "开启"),
    ("desligado", "desligado", "off", "desactivado", "关闭"),
    ("Temperatura, saúde e ciclos não existem no protocolo — só nível e carregamento chegam do telemóvel hoje.", "Temperatura, saúde e ciclos não existem no protocolo — só nível e carregamento chegam do celular hoje.", "Temperature, health and cycle count don't exist in the protocol — only level and charging come from the phone today.", "Temperatura, salud y ciclos no existen en el protocolo — solo nivel y carga llegan del móvil hoy.", "协议中没有温度、健康度和循环次数——目前手机只提供电量和充电状态。"),
    ("WORKSPACES", "WORKSPACES", "WORKSPACES", "ESPACIOS DE TRABAJO", "工作区"),
    ("A carregar workspaces…", "Carregando workspaces…", "Loading workspaces…", "Cargando espacios de trabajo…", "正在加载工作区…"),
    ("Aguardando o primeiro hypr.event…", "Aguardando o primeiro hypr.event…", "Waiting for the first hypr.event…", "Esperando el primer hypr.event…", "等待第一个 hypr.event…"),
    ("ATALHOS", "ATALHOS", "SHORTCUTS", "ATAJOS", "快捷方式"),
    ("Nenhum atalho configurado ainda.", "Nenhum atalho configurado ainda.", "No shortcuts configured yet.", "Ningún atajo configurado todavía.", "尚未配置任何快捷方式。"),
    ("executar", "executar", "run", "ejecutar", "执行"),
    ("remover", "remover", "remove", "eliminar", "移除"),
    ("nome", "nome", "name", "nombre", "名称"),
    ("comando (ex: workspace 3)", "comando (ex: workspace 3)", "command (e.g. workspace 3)", "comando (ej: workspace 3)", "命令（例如 workspace 3）"),
    ("hyprctl dispatch …", "hyprctl dispatch …", "hyprctl dispatch …", "hyprctl dispatch …", "hyprctl dispatch …"),
    ("adicionar", "adicionar", "add", "añadir", "添加"),
    ("janela(s)", "janela(s)", "window(s)", "ventana(s)", "个窗口"),
    ("foco", "foco", "focus", "foco", "焦点"),
    ("Mixer do PC e do telemóvel", "Mixer do PC e do celular", "PC and phone mixer", "Mezclador del PC y del móvil", "电脑与手机的混音器"),
    ("SAÍDA ENCAMINHADA", "SAÍDA ENCAMINHADA", "FORWARDED OUTPUT", "SALIDA REENVIADA", "转发输出"),
    ("Tap parado", "Tap parado", "Tap stopped", "Captura detenida", "监听已停止"),
    ("KB enviados", "KB enviados", "KB sent", "KB enviados", "已发送 KB"),
    ("SAÍDAS (PC)", "SAÍDAS (PC)", "OUTPUTS (PC)", "SALIDAS (PC)", "输出设备（电脑）"),
    ("A carregar saídas de som…", "Carregando saídas de som…", "Loading sound outputs…", "Cargando salidas de sonido…", "正在加载音频输出设备…"),
    ("APPS (PC)", "APPS (PC)", "APPS (PC)", "APPS (PC)", "应用程序（电脑）"),
    ("Nenhuma app tocando som agora.", "Nenhuma app tocando som agora.", "No app playing sound right now.", "Ninguna app reproduciendo sonido ahora.", "目前没有应用在播放声音。"),
    ("saída padrão", "saída padrão", "default output", "salida predeterminada", "默认输出"),
    ("usar", "usar", "use", "usar", "使用"),
    ("toque", "toque", "ringtone", "tono", "铃声"),
    ("mídia", "mídia", "media", "multimedia", "媒体"),
    ("alarme", "alarme", "alarm", "alarma", "闹钟"),
    ("som", "som", "sound", "sonido", "声音"),
    ("vibrar", "vibrar", "vibrate", "vibrar", "振动"),
    ("silencioso", "silencioso", "silent", "silencio", "静音"),
    ("não perturbe: ligado", "não perturbe: ligado", "do not disturb: on", "no molestar: activado", "免打扰：开启"),
    ("não perturbe: desligado", "não perturbe: desligado", "do not disturb: off", "no molestar: desactivado", "免打扰：关闭"),
    ("Sem acesso a \"Não Perturbe\" no telemóvel — vibrar/silencioso não têm efeito até conceder essa permissão nas configurações dele.", "Sem acesso a \"Não Perturbe\" no celular — vibrar/silencioso não têm efeito até conceder essa permissão nas configurações dele.", "No access to \"Do Not Disturb\" on the phone — vibrate/silent have no effect until that permission is granted in its settings.", "Sin acceso a \"No molestar\" en el móvil — vibrar/silencio no tienen efecto hasta conceder ese permiso en sus ajustes.", "手机上未授予“免打扰”权限——振动/静音在授权前不会生效。"),
    ("🎙️ microfone do telemóvel: ativo", "🎙️ microfone do celular: ativo", "🎙️ phone microphone: on", "🎙️ micrófono del móvil: activo", "🎙️ 手机麦克风：已开启"),
    ("🎙️ microfone do telemóvel: desligado", "🎙️ microfone do celular: desligado", "🎙️ phone microphone: off", "🎙️ micrófono del móvil: desactivado", "🎙️ 手机麦克风：已关闭"),
    ("Selecione \"HyprLink-Mic\" como entrada de áudio em qualquer app.", "Selecione \"HyprLink-Mic\" como entrada de áudio em qualquer app.", "Select \"HyprLink-Mic\" as the audio input in any app.", "Selecciona \"HyprLink-Mic\" como entrada de audio en cualquier app.", "在任意应用中选择 “HyprLink-Mic” 作为音频输入设备。"),
    ("resolução", "resolução", "resolution", "resolución", "分辨率"),
    ("fps", "fps", "fps", "fps", "帧率"),
    ("codec", "codec", "codec", "códec", "编码格式"),
    ("parar stream", "parar stream", "stop stream", "detener transmisión", "停止推流"),
    ("iniciar stream", "iniciar stream", "start stream", "iniciar transmisión", "开始推流"),
    ("testar rede/hardware", "testar rede/hardware", "test network/hardware", "probar red/hardware", "测试网络/硬件"),
    ("aplicar", "aplicar", "apply", "aplicar", "应用"),
    ("último uso", "último uso", "last used", "último uso", "上次使用"),
    ("Sem pré-visualização aqui na GUI (custo de CPU extra por só cosmético) — só o /dev/video42 recebe o vídeo. Rotação/espelho seguem o que for ajustado no telemóvel. O teste mede vazão real da rede transmitindo por alguns segundos — não estima CPU de decodificação além da contagem de núcleos.", "Sem pré-visualização aqui na GUI (custo de CPU extra por só cosmético) — só o /dev/video42 recebe o vídeo. Rotação/espelho seguem o que for ajustado no celular. O teste mede vazão real da rede transmitindo por alguns segundos — não estima CPU de decodificação além da contagem de núcleos.", "No preview here in the GUI (extra CPU cost for something purely cosmetic) — only /dev/video42 receives the video. Rotation/mirroring follow whatever is set on the phone. The test measures real network throughput for a few seconds — it doesn't estimate decoding CPU beyond core count.", "Sin previsualización aquí en la GUI (coste extra de CPU por algo solo cosmético) — solo /dev/video42 recibe el vídeo. Rotación/espejo siguen lo ajustado en el móvil. La prueba mide el rendimiento real de la red transmitiendo unos segundos — no estima la CPU de decodificación más allá del número de núcleos.", "GUI 中没有预览（纯装饰性功能会额外消耗 CPU）——只有 /dev/video42 会接收视频。旋转/镜像跟随手机上的设置。测试会实际传输几秒钟以测量真实网络吞吐量——不会估算除核心数之外的解码 CPU 占用。"),
    ("Rato e teclado virtuais", "Mouse e teclado virtuais", "Virtual mouse and keyboard", "Ratón y teclado virtuales", "虚拟鼠标与键盘"),
    ("ESPELHO DO CURSOR", "ESPELHO DO CURSOR", "CURSOR MIRROR", "ESPEJO DEL CURSOR", "光标镜像"),
    ("aguardando…", "aguardando…", "waiting…", "esperando…", "等待中…"),
    ("SENSIBILIDADE", "SENSIBILIDADE", "SENSITIVITY", "SENSIBILIDAD", "灵敏度"),
    ("VELOCIDADE DE SCROLL", "VELOCIDADE DE SCROLL", "SCROLL SPEED", "VELOCIDAD DE DESPLAZAMIENTO", "滚动速度"),
    ("aceleração", "aceleração", "acceleration", "aceleración", "加速"),
    ("inverter scroll", "inverter scroll", "invert scroll", "invertir desplazamiento", "反转滚动"),
    ("teclado virtual", "teclado virtual", "virtual keyboard", "teclado virtual", "虚拟键盘"),
    ("Movimento, cliques e scroll chegam do telemóvel via /dev/uinput — os sliders acima já se aplicam de verdade.", "Movimento, cliques e scroll chegam do celular via /dev/uinput — os sliders acima já se aplicam de verdade.", "Movement, clicks and scroll come from the phone via /dev/uinput — the sliders above already apply for real.", "Movimiento, clics y desplazamiento llegan del móvil vía /dev/uinput — los deslizadores de arriba ya se aplican de verdad.", "移动、点击和滚动通过 /dev/uinput 从手机传来——上面的滑块已经真实生效。"),
    ("Rede e dispositivo ligado", "Rede e dispositivo ligado", "Network and connected device", "Red y dispositivo conectado", "网络与已连接设备"),
    ("REDE", "REDE", "NETWORK", "RED", "网络"),
    ("host : porta", "host : porta", "host : port", "host : puerto", "主机 : 端口"),
    ("fingerprint deste PC", "fingerprint deste PC", "this PC's fingerprint", "huella de este PC", "本机指纹"),
    ("TELEMÓVEL LIGADO AGORA", "CELULAR LIGADO AGORA", "PHONE CONNECTED NOW", "MÓVIL CONECTADO AHORA", "当前已连接手机"),
    ("fingerprint", "fingerprint", "fingerprint", "huella", "指纹"),
    ("Nenhum telemóvel ligado agora.", "Nenhum celular ligado agora.", "No phone connected right now.", "Ningún móvil conectado ahora.", "当前没有已连接的手机。"),
    ("Minimizar pra bandeja usando workspace especial do Hyprland", "Minimizar pra bandeja usando workspace especial do Hyprland", "Minimize to tray using a special Hyprland workspace", "Minimizar a la bandeja usando un espacio de trabajo especial de Hyprland", "使用 Hyprland 特殊工作区最小化到托盘"),
    ("Se desligado, o botão de minimizar some do cabeçalho — esse mecanismo é específico do Hyprland (move a janela pra uma workspace especial), pode não fazer sentido noutro compositor.", "Se desligado, o botão de minimizar some do cabeçalho — esse mecanismo é específico do Hyprland (move a janela pra uma workspace especial), pode não fazer sentido noutro compositor.", "If off, the minimize button disappears from the header — this mechanism is Hyprland-specific (moves the window to a special workspace) and may not make sense on another compositor.", "Si está desactivado, el botón de minimizar desaparece del encabezado — este mecanismo es específico de Hyprland (mueve la ventana a un espacio de trabajo especial), puede no tener sentido en otro compositor.", "关闭后，最小化按钮会从标题栏消失——这个机制是 Hyprland 特有的（把窗口移到一个特殊工作区），在其他合成器上可能没有意义。"),
    ("DISPOSITIVOS EMPARELHADOS", "DISPOSITIVOS PAREADOS", "PAIRED DEVICES", "DISPOSITIVOS EMPAREJADOS", "已配对设备"),
    ("ligado agora", "ligado agora", "connected now", "conectado ahora", "当前已连接"),
    ("pareado", "pareado", "paired", "emparejado", "已配对"),
    ("revogar", "revogar", "revoke", "revocar", "撤销"),
    ("Nenhum dispositivo pareado.", "Nenhum dispositivo pareado.", "No paired devices.", "Ningún dispositivo emparejado.", "没有已配对的设备。"),
    ("reiniciar daemon", "reiniciar daemon", "restart daemon", "reiniciar daemon", "重启守护进程"),
    ("Nível de log e retenção de histórico ainda não têm UI — dá pra reparear via QR se precisar trocar de telemóvel.", "Nível de log e retenção de histórico ainda não têm UI — dá pra reparear via QR se precisar trocar de celular.", "Log level and history retention don't have UI yet — you can re-pair via QR if you need to switch phones.", "Nivel de registro y retención de historial todavía no tienen interfaz — puedes volver a emparejar por QR si necesitas cambiar de móvil.", "日志级别和历史保留期限尚无界面设置——如需更换手机可通过二维码重新配对。"),
    ("IDIOMA", "IDIOMA", "LANGUAGE", "IDIOMA", "语言"),
];

/// Como `t()`, mas substitui um `{}` no template traduzido pelo valor —
/// cobre os templates de uma variável só (contagens, na maioria). Ordem de
/// palavras ao redor do `{}` muda por idioma; a posição do `{}` em si, não.
pub fn t1(key: &'static str, value: impl std::fmt::Display) -> String {
    t(key).replacen("{}", &value.to_string(), 1)
}

/// Como `t1`, mas pra templates com mais de um `{}` — preenche na ordem dada.
/// Assume que a ordem dos valores faz sentido em toda tradução (frases
/// técnicas curtas, sem inversão de ordem entre idiomas aqui).
pub fn tn(key: &'static str, values: &[&dyn std::fmt::Display]) -> String {
    let mut s = t(key).to_string();
    for v in values {
        s = s.replacen("{}", &v.to_string(), 1);
    }
    s
}

pub fn t(pt: &'static str) -> &'static str {
    let idx = current_index();
    if idx == 0 {
        return pt;
    }
    for (a, pt_br, en, es, zh) in TABLE {
        if *a == pt {
            return match idx {
                1 => pt_br,
                2 => en,
                3 => es,
                _ => zh,
            };
        }
    }
    pt
}

/// Console de diagnóstico: linhas vêm de ~80 `push_log(...)` espalhados pelo
/// daemon inteiro, já formatadas com dados dinâmicos (fingerprint, bytes,
/// erro de lib externa) — não dá pra chavear a linha inteira como `TABLE`
/// faz pras telas. Em vez de traduzir cada call site (reescreveria o daemon
/// inteiro), troca as frases fixas por substring no único ponto em que as
/// linhas viram texto pra tela (`gui/mod.rs`, montagem do `console`).
///
/// Só EN/ZH têm essa tabela — foi só o que o usuário pediu pro log; PT-BR e
/// ES-ES ficam com a linha original (PT-PT), como antes.
///
/// ponytail: `str::replace` em cadeia, frases longas primeiro (senão uma
/// frase curta genérica comeria um pedaço de uma frase longa antes dela ser
/// trocada). Cobre os termos mais comuns; erro cru de lib externa (`{e}`,
/// `err.error()`) segue sem tradução — já costuma vir em inglês mesmo.
#[rustfmt::skip]
const LOG_TABLE: &[(&str, &str, &str)] = &[
    ("carregando v4l2loopback (pode pedir sua senha)...", "loading v4l2loopback (may ask for your password)...", "正在加载 v4l2loopback（可能需要输入密码）..."),
    ("não foi possível carregar o v4l2loopback (pkexec cancelado ou módulo indisponível)", "could not load v4l2loopback (pkexec cancelled or module unavailable)", "无法加载 v4l2loopback（pkexec 已取消或模块不可用）"),
    ("stream fechou antes do byte de codec", "stream closed before the codec byte", "流在编码字节之前已关闭"),
    ("sem dados há 15s, encerrando stream travado", "no data for 15s, stopping stuck stream", "15秒无数据，正在停止卡住的流"),
    ("GStreamer não inicializou", "GStreamer failed to initialize", "GStreamer 未能初始化"),
    ("falha ao montar o pipeline", "failed to build the pipeline", "构建管道失败"),
    ("appsrc não encontrado no pipeline", "appsrc not found in pipeline", "管道中未找到 appsrc"),
    ("não foi possível iniciar o pipeline", "could not start the pipeline", "无法启动管道"),
    ("pipeline inesperado", "unexpected pipeline", "管道异常"),
    ("erro no pipeline GStreamer", "GStreamer pipeline error", "GStreamer 管道错误"),
    ("stream iniciado", "stream started", "流已启动"),
    ("stream encerrado", "stream stopped", "流已停止"),
    ("KB recebidos", "KB received", "KB 已接收"),
    ("wl-paste indisponível", "wl-paste unavailable", "wl-paste 不可用"),
    ("notificações:", "notifications:", "通知："),
    ("stream sem anúncio correspondente (id=", "stream with no matching announcement (id=", "流没有匹配的通知 (id="),
    ("ficheiro recebido cancelado", "file receive cancelled", "文件接收已取消"),
    ("ficheiro recebido:", "file received:", "文件已接收："),
    ("ficheiro enviado:", "file sent:", "文件已发送："),
    ("não foi possível criar", "could not create", "无法创建"),
    ("não foi possível ler", "could not read", "无法读取"),
    ("não foi possível abrir o uni-stream", "could not open the uni-stream", "无法打开单向流"),
    ("não foi possível abrir", "could not open", "无法打开"),
    ("sem conexão ativa pra enviar", "no active connection to send", "没有可用于发送的活动连接"),
    ("envio cancelado", "send cancelled", "发送已取消"),
    ("falha ao enviar", "failed to send", "发送失败"),
    ("enviando", "sending", "正在发送"),
    ("sem sink padrão detetado", "no default sink detected", "未检测到默认音频接收器"),
    ("appsink não encontrado no pipeline", "appsink not found in pipeline", "管道中未找到 appsink"),
    ("appsink com tipo inesperado", "appsink with unexpected type", "appsink 类型异常"),
    ("audio tap iniciado", "audio tap started", "音频监听已启动"),
    ("não foi possível abrir o stream", "could not open the stream", "无法打开流"),
    ("falha ao escrever o id no stream", "failed to write the id to the stream", "写入流 ID 失败"),
    ("stream aberto (id=", "stream opened (id=", "流已打开 (id="),
    ("KB capturados do PipeWire", "KB captured from PipeWire", "KB 从 PipeWire 捕获"),
    ("canal pro QUIC fechou, parando captura", "channel to QUIC closed, stopping capture", "QUIC 通道已关闭，停止捕获"),
    ("pull_sample parou", "pull_sample stopped", "pull_sample 已停止"),
    ("escrita no stream falhou", "stream write failed", "流写入失败"),
    ("KB enviados", "KB sent", "KB 已发送"),
    ("KB no total)", "KB total)", "KB 总计）"),
    ("selecione \"HyprLink-Mic\" como entrada de áudio", "select \"HyprLink-Mic\" as the audio input", "选择“HyprLink-Mic”作为音频输入"),
    ("conexão encerrada", "connection closed", "连接已关闭"),
    ("core.hello autorizado", "core.hello authorized", "core.hello 已授权"),
    ("desconectou", "disconnected", "已断开连接"),
    ("clipboard.set · telemóvel → PC", "clipboard.set · phone → PC", "clipboard.set · 手机 → 电脑"),
    ("webcam (telemóvel):", "webcam (phone):", "摄像头（手机）："),
    ("microfone: telemóvel anunciou stream de áudio", "mic: phone announced audio stream", "麦克风：手机已宣布音频流"),
    ("bateria do telemóvel:", "phone battery:", "手机电量："),
    ("carregando=", "charging=", "充电="),
    ("share.file · recebendo", "share.file · receiving", "share.file · 接收中"),
    ("tipo ainda não implementado:", "type not yet implemented:", "类型尚未实现："),
    ("HYPRLAND_INSTANCE_SIGNATURE ausente — hypr.event desativado", "HYPRLAND_INSTANCE_SIGNATURE missing — hypr.event disabled", "缺少 HYPRLAND_INSTANCE_SIGNATURE — hypr.event 已禁用"),
    ("tray: restaurando janela", "tray: restoring window", "托盘：正在还原窗口"),
    ("tray: minimizando", "tray: minimizing", "托盘：正在最小化"),
    ("pedido de ligar enviado ao telemóvel", "request to turn on sent to phone", "已向手机发送开启请求"),
    ("pedido de desligar enviado ao telemóvel", "request to turn off sent to phone", "已向手机发送关闭请求"),
    ("não foi possível enviar o pedido (sem conexão ativa?)", "could not send the request (no active connection?)", "无法发送请求（没有活动连接？）"),
    ("não foi possível pedir o stream (sem conexão ativa?)", "could not request the stream (no active connection?)", "无法请求视频流（没有活动连接？）"),
    ("não foi possível iniciar o teste (sem conexão ativa?)", "could not start the test (no active connection?)", "无法启动测试（没有活动连接？）"),
    ("microfone:", "mic:", "麦克风："),
    ("audio tap:", "audio tap:", "音频监听："),
    ("webcam:", "webcam:", "摄像头："),
];

pub fn tr_log(line: &str) -> std::borrow::Cow<'_, str> {
    let idx = current_index();
    if idx != 2 && idx != 4 {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = line.to_string();
    for (pt, en, zh) in LOG_TABLE {
        let translated = if idx == 2 { *en } else { *zh };
        out = out.replace(pt, translated);
    }
    std::borrow::Cow::Owned(out)
}
