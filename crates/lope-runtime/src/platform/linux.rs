//! Linux: same group probing; leader state from /proc when available.

pub fn leader_state(pid: i32) -> String {
    if let Ok(raw) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        if let Some((_, tail)) = raw.rsplit_once(')') {
            if let Some(s) = tail.split_whitespace().next() {
                return s.to_string();
            }
        }
    }
    lope_state::process::state(pid as i64)
}
