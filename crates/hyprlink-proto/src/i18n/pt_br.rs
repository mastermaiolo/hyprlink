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
];
