//! Persists the OAuth refresh token in the OS credential store (Windows
//! Credential Manager via DPAPI) so sign-in only happens once. Thin wrapper
//! around `keyring` — not unit-tested here since it talks to a real OS
//! secret store; exercised in the try-it-now verification instead.

const SERVICE: &str = "work-dash-windows-client";
const USER: &str = "graph-refresh-token";

pub fn save_refresh_token(token: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, USER).map_err(|e| e.to_string())?;
    entry.set_password(token).map_err(|e| e.to_string())
}

pub fn load_refresh_token() -> Option<String> {
    let entry = keyring::Entry::new(SERVICE, USER).ok()?;
    entry.get_password().ok()
}
