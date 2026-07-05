//! CLI entry point for desktop sharing orchestration.

use metis_remote::{autostart_from_config, disable, enable, set_password, status};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "metis_remote=info,warn".into()),
        )
        .init();

    let code = match run(std::env::args().skip(1).collect()) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("metis-remote: {err}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some("status") => {
            let snap = status();
            let json = serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())?;
            println!("{json}");
            Ok(())
        }
        Some("enable") => enable(),
        Some("disable") => disable(),
        Some("autostart") => autostart_from_config(),
        Some("set-credentials") => {
            let username = args.get(1).cloned().or_else(|| std::env::var("USER").ok()).ok_or_else(
                || "usage: metis-remote set-credentials <username> <password>".to_string(),
            )?;
            let password = args.get(2).cloned().ok_or_else(|| {
                "usage: metis-remote set-credentials <username> <password>".to_string()
            })?;
            set_password(&username, &password)
        }
        Some(cmd) => Err(format!("unknown command: {cmd}")),
    }
}

fn print_help() {
    eprintln!(
        "Usage: metis-remote {{status|enable|disable|autostart|set-credentials USER PASS}}

  status          Print JSON status (for Settings UI)
  enable          Start session-sharing RDP per remote.json
  disable         Stop RDP and clear enabled flag
  autostart       Enable sharing when remote.json enabled + auto_start
  set-credentials Set RDP login (grdctl session store)"
    );
}
