"""Small rendering helpers for Lope command output.

v0.7 adds structured consensus, synthesis, memory, and export surfaces. Keep
new renderers here instead of growing cli.py further. Existing commands can move
onto these helpers gradually without changing their stdout contracts.
"""

from __future__ import annotations

import json
import os
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple

from .redaction import redact_text, redact_mapping


def print_json(payload: Any) -> None:
    """Print deterministic, redacted JSON for machine-readable CLI modes."""
    if isinstance(payload, dict):
        payload = redact_mapping(payload)
    elif isinstance(payload, list):
        payload = [redact_mapping(x) if isinstance(x, dict) else x for x in payload]
    print(json.dumps(payload, indent=2, sort_keys=True))


def section(title: str, body: Optional[str] = None, *, underline: str = "━") -> str:
    """Return a Lope-style section block."""
    header = f"{underline * 3} {title} {underline * 3}"
    if body is None or body == "":
        return header
    return f"{header}\n{redact_text(body).rstrip()}"


def bullet_list(items: Iterable[str], *, prefix: str = "- ") -> str:
    """Render redacted bullets, omitting empty entries."""
    return "\n".join(f"{prefix}{redact_text(item).strip()}" for item in items if str(item).strip())


def fanout_payload(label: str, results: Sequence[Tuple[str, str, Optional[str]]]) -> List[Dict[str, Any]]:
    """Convert fan-out tuples into the stable JSON shape used by ask/review."""
    from .processes import CompletionDiagnostic
    from .diagnostics import redact_diagnostic

    payload = []
    for name, answer, error in results:
        row = {"validator": name, label: redact_text(answer), "error": redact_text(error) if error else None}
        if isinstance(error, CompletionDiagnostic):
            c = error.completion
            row["completion"] = {
                "status": "UNVERIFIED", "cleanup": c.cleanup_result,
                "stdout_complete": c.stdout_complete,
                "stdout_truncated": c.stdout_truncated, "returncode": c.returncode,
            }
            if os.environ.get("LOPE_RECOVER_UNVERIFIED_OUTPUT") == "1":
                row["unverified_output"] = redact_text(redact_diagnostic(c.stdout))
        payload.append(row)
    return payload


__all__ = ["print_json", "section", "bullet_list", "fanout_payload"]
