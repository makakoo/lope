//! `cli_discovery.py`: the 15 known CLIs, probed with `shutil.which`.

pub struct CliInfo {
    pub name: &'static str,
    pub binary: &'static str,
    pub display_name: &'static str,
    pub is_default: bool,
}

pub const KNOWN_CLIS: [CliInfo; 15] = [
    CliInfo { name: "claude", binary: "claude", display_name: "Claude Code", is_default: true },
    CliInfo { name: "opencode", binary: "opencode", display_name: "OpenCode", is_default: true },
    CliInfo { name: "gemini", binary: "gemini", display_name: "Gemini CLI", is_default: false },
    CliInfo { name: "codex", binary: "codex", display_name: "OpenAI Codex", is_default: false },
    CliInfo { name: "vibe", binary: "vibe", display_name: "Mistral Vibe", is_default: false },
    CliInfo { name: "aider", binary: "aider", display_name: "Aider", is_default: false },
    CliInfo { name: "ollama", binary: "ollama", display_name: "Ollama (local)", is_default: false },
    CliInfo { name: "goose", binary: "goose", display_name: "Goose (Block)", is_default: false },
    CliInfo { name: "interpreter", binary: "interpreter", display_name: "Open Interpreter", is_default: false },
    CliInfo { name: "llama-cpp", binary: "llama-cli", display_name: "llama.cpp", is_default: false },
    CliInfo { name: "gh-copilot", binary: "gh", display_name: "GitHub Copilot CLI", is_default: false },
    CliInfo { name: "amazon-q", binary: "q", display_name: "Amazon Q", is_default: false },
    CliInfo { name: "pi", binary: "pi", display_name: "pi (Traylinx)", is_default: false },
    CliInfo { name: "qwen", binary: "qwen", display_name: "Qwen Code", is_default: false },
    CliInfo { name: "agy", binary: "agy", display_name: "Agy", is_default: false },
];

pub fn discover() -> Vec<&'static CliInfo> {
    KNOWN_CLIS.iter().filter(|c| lope_core::paths::which(c.binary).is_some()).collect()
}
