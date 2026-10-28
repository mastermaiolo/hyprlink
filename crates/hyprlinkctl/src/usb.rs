//! `hyprlinkctl usb setup` — põe a ligação de rede da ancoragem USB sem rota
//! por omissão, para a internet do PC não passar pelo telemóvel.
//!
//! Só corre quando o utilizador o pede. Mostra o comando `nmcli` antes e pede
//! confirmação no terminal; corre-o **como utilizador** (sem `pkexec`). Se o
//! `nmcli` recusar, explica e sai. Nunca mexe na firewall (só a sugere, no
//! `doctor`).

use std::io::{BufRead, Write};
use std::process::ExitCode;

use hyprlink_env::net::{self, UsbReport};
use hyprlink_env::{Env, RealRunner};

pub fn run(args: &[String]) -> ExitCode {
    if args.get(1).map(String::as_str) != Some("setup") {
        eprintln!(
            "uso: hyprlinkctl usb setup   (aplica ipv4/ipv6.never-default à ligação da ancoragem USB)"
        );
        return ExitCode::from(2);
    }
    let mut env = Env::real();
    env.runner = Box::new(RealRunner {
        envs: hyprlink_env::instance::real_session_env(),
        ..RealRunner::default()
    });
    let report = net::usb_report(&env, net::DAEMON_PORT);
    let stdin = std::io::stdin();
    let code = setup(
        report.as_ref(),
        &mut stdin.lock(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        |a| {
            let o = std::process::Command::new("nmcli")
                .args(a)
                .output()
                .map_err(|e| format!("não consegui correr o nmcli: {e}"))?;
            if o.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&o.stderr);
                Err(err.trim().to_string())
            }
        },
    );
    ExitCode::from(code)
}

/// Sim (`s`, `sim`, `y`, `yes`, sem distinguir maiúsculas)?
fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_lowercase().as_str(),
        "s" | "sim" | "y" | "yes"
    )
}

/// A lógica toda, sem terminal nem `nmcli` reais. `apply` recebe os
/// argumentos do `nmcli`. Devolve o código de saída.
fn setup(
    report: Option<&UsbReport>,
    input: &mut impl BufRead,
    out: &mut impl Write,
    err: &mut impl Write,
    apply: impl FnOnce(&[String]) -> Result<(), String>,
) -> u8 {
    let Some(r) = report else {
        let _ = writeln!(
            err,
            "Sem interface de ancoragem USB ativa. Liga a ancoragem USB no telemóvel e repete."
        );
        return 1;
    };
    let Some(conn) = &r.connection else {
        let _ = writeln!(
            err,
            "Não encontrei a ligação do NetworkManager de {} (nmcli ausente ou a interface não é gerida por ele).\n\
             Marca ipv4.never-default e ipv6.never-default nessa ligação à mão.",
            r.interfaces.join(", ")
        );
        return 1;
    };
    if !r.default_route_via_usb {
        let _ = writeln!(
            out,
            "De momento a rota por omissão não passa pelo telemóvel; o comando evita que passe no futuro."
        );
    }
    let _ = writeln!(
        out,
        "Vou correr (como o teu utilizador, sem pkexec):\n\n  {}\n",
        net::never_default_cmd(conn)
    );
    let _ = write!(out, "Aplicar? [s/N] ");
    let _ = out.flush();
    let mut answer = String::new();
    if input.read_line(&mut answer).is_err() || !is_yes(&answer) {
        let _ = writeln!(out, "Nada alterado.");
        return 0;
    }
    match apply(&net::never_default_args(conn)) {
        Ok(()) => {
            let _ = writeln!(
                out,
                "Feito. Desliga e volta a ligar a ancoragem (ou: nmcli connection up \"{conn}\") para a rota sair."
            );
            0
        }
        Err(why) => {
            let _ = writeln!(
                err,
                "O nmcli recusou: {why}\n\
                 Não insisto (sem pkexec). Corre o comando acima num terminal com permissão, \
                 ou pede ao administrador."
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn report(conn: Option<&str>, route: bool) -> UsbReport {
        UsbReport {
            interfaces: vec!["usb0".into()],
            default_route_via_usb: route,
            connection: conn.map(str::to_string),
            never_default_cmd: None,
            firewall: None,
        }
    }

    /// Corre `setup` com a resposta dada; devolve (código, stdout, stderr, args do nmcli).
    fn go(
        r: Option<&UsbReport>,
        answer: &str,
        nm: Result<(), String>,
    ) -> (u8, String, String, Option<Vec<String>>) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let seen = RefCell::new(None);
        let code = setup(r, &mut answer.as_bytes(), &mut out, &mut err, |a| {
            *seen.borrow_mut() = Some(a.to_vec());
            nm
        });
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            seen.into_inner(),
        )
    }

    #[test]
    fn respostas_sim_e_nao() {
        for y in ["s", "S\n", "sim", "Yes", " y "] {
            assert!(is_yes(y), "{y}");
        }
        for n in ["", "n", "não", "talvez", "ss"] {
            assert!(!is_yes(n), "{n}");
        }
    }

    #[test]
    fn mostra_o_comando_antes_e_so_corre_com_confirmacao() {
        let r = report(Some("Wired connection 1"), true);
        let (code, out, _, ran) = go(Some(&r), "s\n", Ok(()));
        assert_eq!(code, 0);
        let shown = out
            .find("nmcli connection modify \"Wired connection 1\"")
            .unwrap();
        let asked = out.find("Aplicar?").unwrap();
        assert!(shown < asked, "o comando aparece antes da pergunta");
        assert_eq!(
            ran.unwrap(),
            vec![
                "connection",
                "modify",
                "Wired connection 1",
                "ipv4.never-default",
                "yes",
                "ipv6.never-default",
                "yes"
            ]
        );
        assert!(out.contains("Feito."));
    }

    #[test]
    fn sem_confirmacao_nao_corre_nada() {
        let r = report(Some("X"), true);
        for a in ["", "n\n", "\n"] {
            let (code, out, _, ran) = go(Some(&r), a, Ok(()));
            assert_eq!((code, ran), (0, None));
            assert!(out.contains("Nada alterado."));
        }
    }

    #[test]
    fn nmcli_recusa_explica_e_sai_com_erro() {
        let r = report(Some("X"), true);
        let (code, _, err, _) = go(Some(&r), "s\n", Err("Not authorized".into()));
        assert_eq!(code, 1);
        assert!(err.contains("Not authorized") && err.contains("sem pkexec"));
    }

    #[test]
    fn sem_usb_ou_sem_ligacao_nao_corre_nada() {
        let (code, _, err, ran) = go(None, "s\n", Ok(()));
        assert_eq!((code, ran), (1, None));
        assert!(err.contains("Sem interface"));
        let r = report(None, true);
        let (code, _, err, ran) = go(Some(&r), "s\n", Ok(()));
        assert_eq!((code, ran), (1, None));
        assert!(err.contains("never-default"));
    }
}
