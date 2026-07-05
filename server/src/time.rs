use chrono::Local;

pub fn now_iso() -> String {
    Local::now().to_rfc3339()
}

pub fn today() -> String {
    Local::now().date_naive().to_string()
}
