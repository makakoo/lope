"""Printed JSON contract for a failed provider completion.

A zero return code with incomplete or truncated stdout stays an error.
Recovered text is opt-in, separate from answer, and redacted.
"""

from __future__ import annotations

import json

import pytest

from lope.output import fanout_payload, print_json
from lope.processes import CompletionDiagnostic, ProcessCompletionError

_STDOUT = "visible-fragment password=short-canary api_key=short-secret"
_REDACTED = "visible-fragment password=[REDACTED] api_key=[REDACTED]"
_ENV = "LOPE_RECOVER_UNVERIFIED_OUTPUT"


def _failed_result():
    error = ProcessCompletionError(
        "provider completion could not be established",
        stdout=_STDOUT,
        stderr="stderr-canary",
        cleanup_result="cleanup_failed:inspection_unknown",
        stdout_complete=False,
        stdout_truncated=True,
        returncode=0,
    )
    return ("fixture", "", CompletionDiagnostic(error))


def _emit(capsys, results):
    print_json(fanout_payload("answer", results))
    raw = capsys.readouterr().out
    return json.loads(raw), raw


def _assert_unverified(row):
    assert row["validator"] == "fixture"
    assert row["answer"] == ""
    assert row["error"] == "provider completion unverified"
    completion = row["completion"]
    assert set(completion) == {
        "cleanup",
        "returncode",
        "status",
        "stdout_complete",
        "stdout_truncated",
    }
    assert completion["status"] == "UNVERIFIED"
    assert completion["cleanup"] == "cleanup_failed:inspection_unknown"
    assert completion["returncode"] == 0
    assert type(completion["returncode"]) is int
    assert completion["stdout_complete"] is False
    assert completion["stdout_truncated"] is True


def test_failed_completion_json_omits_body_by_default(monkeypatch, capsys):
    monkeypatch.delenv(_ENV, raising=False)
    parsed, raw = _emit(capsys, [_failed_result()])
    row = parsed[0]
    _assert_unverified(row)
    assert set(row) == {"answer", "completion", "error", "validator"}
    assert "unverified_output" not in row
    for secret in ("visible-fragment", "short-canary", "short-secret", "stderr-canary"):
        assert secret not in raw


def test_opt_in_recovery_redacts_short_secrets_outside_answer(monkeypatch, capsys):
    monkeypatch.setenv(_ENV, "1")
    parsed, raw = _emit(capsys, [_failed_result()])
    row = parsed[0]
    _assert_unverified(row)
    assert set(row) == {
        "answer",
        "completion",
        "error",
        "unverified_output",
        "validator",
    }
    assert row["unverified_output"] == _REDACTED
    assert row["answer"] == ""
    for secret in ("short-canary", "short-secret", "stderr-canary"):
        assert secret not in raw


@pytest.mark.parametrize("value", ["0", "false", ""], ids=["0", "false", "empty"])
def test_recovery_ignores_zero_false_and_empty(monkeypatch, capsys, value):
    monkeypatch.setenv(_ENV, value)
    parsed, raw = _emit(capsys, [_failed_result()])
    row = parsed[0]
    _assert_unverified(row)
    assert "unverified_output" not in row
    for secret in ("visible-fragment", "short-canary", "short-secret"):
        assert secret not in raw


def test_ordinary_tuples_keep_original_json_with_recovery(monkeypatch, capsys):
    monkeypatch.setenv(_ENV, "1")
    parsed, _raw = _emit(
        capsys,
        [
            ("claude", "substantive answer", None),
            ("gemini", "", "subprocess timed out"),
        ],
    )
    assert parsed == [
        {"answer": "substantive answer", "error": None, "validator": "claude"},
        {"answer": "", "error": "subprocess timed out", "validator": "gemini"},
    ]
    for row in parsed:
        assert set(row) == {"answer", "error", "validator"}
