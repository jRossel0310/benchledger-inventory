//! Dev helper: store or clear the DigiKey OAuth2 client-credentials pair in
//! the OS credential store, going THROUGH `inventory_core::secrets` (the
//! same module the app itself reads from), so this is guaranteed to write
//! in a format the app can read back.
//!
//! Credentials are read from environment variables, never argv/flags —
//! command-line arguments are trivially visible in shell history and
//! process listings, so requiring env vars means an operator has to
//! deliberately set them for this one invocation. Typical use (PowerShell):
//!
//!   $env:DIGIKEY_CLIENT_ID = "..."
//!   $env:DIGIKEY_CLIENT_SECRET = "..."
//!   cargo run -p inventory-core --bin set_digikey_credentials
//!   Remove-Item Env:\DIGIKEY_CLIENT_ID, Env:\DIGIKEY_CLIENT_SECRET
//!
//! Never prints the credential values — only their lengths, so a
//! transcript/log of this run confirms something was stored without
//! being itself a secret.
//!
//! `--clear` removes both stored entries instead.

use inventory_core::secrets::{
    clear_digikey_credentials, store_digikey_credentials, DigiKeyCredentials,
};

fn main() {
    let clear_requested = std::env::args().skip(1).any(|arg| arg == "--clear");

    if clear_requested {
        match clear_digikey_credentials() {
            Ok(()) => println!("cleared"),
            Err(err) => {
                eprintln!("failed to clear DigiKey credentials: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    let client_id = std::env::var("DIGIKEY_CLIENT_ID").unwrap_or_default();
    let client_secret = std::env::var("DIGIKEY_CLIENT_SECRET").unwrap_or_default();

    if client_id.is_empty() || client_secret.is_empty() {
        eprintln!("set DIGIKEY_CLIENT_ID and DIGIKEY_CLIENT_SECRET env vars, then re-run");
        std::process::exit(1);
    }

    let client_id_len = client_id.len();
    let client_secret_len = client_secret.len();
    let creds = DigiKeyCredentials {
        client_id,
        client_secret,
    };

    match store_digikey_credentials(&creds) {
        Ok(()) => {
            println!(
                "stored DigiKey credentials (client_id length={client_id_len}, client_secret length={client_secret_len})"
            );
        }
        Err(err) => {
            eprintln!("failed to store DigiKey credentials: {err}");
            std::process::exit(1);
        }
    }
}
