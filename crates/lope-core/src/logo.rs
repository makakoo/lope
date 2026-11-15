//! `logo.py`: banner, box, mascot, tiny and the random gimmick, with the
//! same colour rules (NO_COLOR / LOPE_NO_COLOR / stdout TTY).

pub const CREAM: &str = "\x1b[38;5;230m";
pub const YELLOW: &str = "\x1b[38;5;220m";
pub const ORANGE: &str = "\x1b[38;5;208m";
pub const RED: &str = "\x1b[38;5;196m";
pub const DARKRED: &str = "\x1b[38;5;124m";
pub const BOLD_ORANGE: &str = "\x1b[1;38;5;208m";
pub const BOLD_RED: &str = "\x1b[1;38;5;196m";
pub const BOLD_YELLOW: &str = "\x1b[1;38;5;220m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";

/// `_use_color()`.
pub fn use_color() -> bool {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    if set("NO_COLOR") || set("LOPE_NO_COLOR") {
        return false;
    }
    unsafe { libc::isatty(1) == 1 }
}

pub fn c(text: &str, color: &str) -> String {
    if use_color() {
        format!("{color}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn banner() -> String {
    let lines = [
        c("▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓", CREAM),
        c("████████████████████████████████████████████████", YELLOW),
        c("                                                ", ORANGE),
        c("   ██      ██████    ██████   ███████           ", ORANGE),
        c("   ██     ██◉  ◉██   ██   ██  ██                ", ORANGE),
        c("   ██     ██ ▽▽ ██   ██████   █████             ", ORANGE),
        c("   ██     ██ ◡◡ ██   ██       ██                ", ORANGE),
        c("   ██████  ██████    ██       ███████           ", ORANGE),
        c("                                                ", ORANGE),
        c("████████████████████████████████████████████████", RED),
        c("▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓", DARKRED),
        String::new(),
        format!("    {}", c("any cli implements  ·  any cli validates", DIM)),
    ];
    lines.iter().map(|l| format!(" {l}")).collect::<Vec<_>>().join("\n")
}

pub fn boxed(version: &str) -> String {
    let version_line =
        if version.is_empty() { "sprint runner".to_string() } else { format!("sprint runner {version}") };
    [
        format!("   {}", c("╭─────────────────╮", YELLOW)),
        format!("   {}   {}    {}", c("│", YELLOW), c("L  O  P  E", BOLD_ORANGE), c("│", YELLOW)),
        format!("   {}        {}        {}", c("│", YELLOW), c("⟲", RED), c("│", YELLOW)),
        format!("   {}", c("╰─────────────────╯", YELLOW)),
        format!("   {}", c(&format!(" {version_line}"), DIM)),
    ]
    .join("\n")
}

pub fn mascot(line: &str) -> String {
    [
        format!("        {}                 {}", c("▄▄▄▄▄▄", YELLOW), c("~~~", DIM)),
        format!("       {}{}{}           {}", c("█", YELLOW), c(" ◉  ◉ ", BOLD_RED), c("█", YELLOW), c("~", DIM)),
        format!("       {}{}{}         {}", c("█", YELLOW), c("  ▽▽  ", RED), c("█", YELLOW), c("~", DIM)),
        format!("        {}", c("▀▀▀▀▀▀", YELLOW)),
        format!("          {}", c("│", DIM)),
        format!("       {}", c("╭──┴──╮", ORANGE)),
        format!("       {} {} {}", c("│", ORANGE), c("LOPE", BOLD_ORANGE), c("│", ORANGE)),
        format!("       {}", c("╰─────╯", ORANGE)),
        format!("          {}", c("╱ ╲", DIM)),
        format!("         {}", c("╱   ╲", DIM)),
        String::new(),
        format!("  {}", c(line, DIM)),
    ]
    .join("\n")
}

pub fn tiny() -> String {
    format!("{}{} {} {}", c("▓", ORANGE), c("▓", RED), c("LOPE", BOLD_ORANGE), c("»", DIM))
}

const GIMMICK_LINES: [&str; 14] = [
    "runs the validator loop",
    "sniffs out the bugs",
    "peers into the code",
    "woke up for this sprint",
    "does a little dance",
    "beams quietly",
    "shipped. noticed.",
    "ensemble assembled",
    "majority vote incoming",
    "reading the files...",
    "thinking about it",
    "consulting the oracles",
    "N validators · 1 verdict",
    "watching for drift",
];

/// A uniform float in [0, 1) from the OS RNG.
pub fn random_unit() -> f64 {
    let b = uuid::Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes(b[..8].try_into().unwrap()) >> 11;
    n as f64 / (1u64 << 53) as f64
}

/// `maybe_gimmick(rate)`: only on a colour TTY, never with LOPE_GIMMICK_OFF.
pub fn maybe_gimmick(rate: f64) -> String {
    if std::env::var_os("LOPE_GIMMICK_OFF").is_some_and(|v| !v.is_empty()) || !use_color() {
        return String::new();
    }
    if random_unit() > rate {
        return String::new();
    }
    let idx = (random_unit() * GIMMICK_LINES.len() as f64) as usize % GIMMICK_LINES.len();
    mascot(GIMMICK_LINES[idx])
}
