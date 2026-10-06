//! Português do Brasil: só o que difere do pt-PT (o resto cai para pt-PT).

pub const TABLE: &[(&str, &str)] = &[
    // ── fmt.rs ──
    ("TELEMÓVEL", "CELULAR"),
    ("LIGADO", "CONECTADO"),
    ("A LIGAR…", "CONECTANDO…"),
    ("DESLIGADO", "DESCONECTADO"),
    ("câmara", "câmera"),
    ("ficheiros", "arquivos"),
    ("estado do telemóvel", "estado do celular"),
    ("multimédia do telemóvel", "mídia do celular"),
    ("Câmara", "Câmera"),
    ("Ficheiros", "Arquivos"),
    ("Multimédia", "Multimídia"),
    ("Volume do telemóvel", "Volume do celular"),
    ("ainda por implementar", "ainda não implementado"),
    ("sem telemóvel ligado", "nenhum celular conectado"),
    ("recusado pelo telemóvel", "recusado pelo celular"),
    ("A TRANSFERIR", "TRANSFERINDO"),
    ("sem telemóvel", "sem celular"),
    // ── tray.rs ──
    ("A ligar ao telemóvel…", "Conectando ao celular…"),
    ("Nenhum telemóvel ligado", "Nenhum celular conectado"),
    ("Sem telemóvel", "Sem celular"),
    ("Microfone do telemóvel", "Microfone do celular"),
    ("Espelhar ecrã", "Espelhar tela"),
    // ── app.rs, ui.rs, graphics.rs ──
    ("Secretária", "Mesa"),
    ("Câmara & Ecrã", "Câmera & Tela"),
    ("Partilha", "Compartilhamento"),
    ("Definições", "Configurações"),
    ("{} pasta(s) ignorada(s) — só se enviam ficheiros.", "{} pasta(s) ignorada(s) — só se enviam arquivos."),
    ("A LIGAR AO TELEMÓVEL…", "CONECTANDO AO CELULAR…"),
    ("SEM LIGAÇÃO", "SEM CONEXÃO"),
    ("Sem seletor de ficheiros — instala xdg-desktop-portal-gtk, ou larga o ficheiro aqui.", "Sem seletor de arquivos — instale xdg-desktop-portal-gtk, ou solte o arquivo aqui."),
    ("Escolher ficheiros para o telemóvel", "Escolher arquivos para o celular"),
];
