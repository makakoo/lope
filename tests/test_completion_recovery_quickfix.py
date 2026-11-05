import json
import sys
from types import SimpleNamespace

import pytest

from lope import supervisor
from lope.processes import ProcessCompletionError, CompletionDiagnostic
from lope.generic_validators import GenericSubprocessValidator
from lope.provider_errors import ProviderToolPermissionError
from lope.retry_policy import classify_failure
from lope.invocation import _outcome_for
from lope.runtime import OutcomeClass
from lope.output import fanout_payload
from lope.cli import _fanout_generate, _fanout_quorum


def test_darwin_presence_never_launches_ps(monkeypatch):
    monkeypatch.setattr(supervisor.sys, 'platform', 'darwin')
    monkeypatch.setattr(supervisor.os, 'killpg', lambda *a: None)
    def forbidden(*a, **k):
        pytest.fail('Darwin cleanup must not launch ps')
    monkeypatch.setattr(supervisor.subprocess, 'run', forbidden)
    assert supervisor._process_group_count(123) == 1


def test_permission_denied_is_present(monkeypatch):
    def denied(*a):
        raise PermissionError()
    monkeypatch.setattr(supervisor.os, 'killpg', denied)
    assert supervisor._process_group_count(123) == 1


@pytest.mark.parametrize('counts,expected', [([-1, 0], 0), ([-1]*10, -1)])
def test_unknown_retried_within_deadline(monkeypatch, counts, expected):
    now = [0.0]
    calls = []
    monkeypatch.setattr(supervisor.time, 'monotonic', lambda: now[0])
    monkeypatch.setattr(supervisor.time, 'sleep', lambda n: now.__setitem__(0, now[0]+n))
    def census(*a, **k):
        calls.append(1)
        return counts.pop(0)
    monkeypatch.setattr(supervisor, '_process_group_count', census)
    assert supervisor._wait_group_exit(123, .15) == expected
    assert 2 <= len(calls) <= 4
    assert now[0] <= .15


def test_proven_absence_never_resignalled(monkeypatch):
    class Process:
        pid = 123
        exited = False
        def poll(self): return 0 if self.exited else None
        def wait(self, timeout): self.exited = True
    calls = []
    monkeypatch.setattr(supervisor, '_signal_tree', lambda p, s: calls.append(s) or 'already_exited')
    def forbidden(*a, **k): pytest.fail('group already proven absent')
    monkeypatch.setattr(supervisor, '_wait_group_exit', forbidden)
    assert supervisor._terminate_and_reap(Process())[0].startswith('clean')
    assert len(calls) == 1


def completion():
    return ProcessCompletionError('api_key=reason-secret', stdout='OUTPUT api_key=canary-secret',
        stderr='private stderr', cleanup_result='cleanup_failed:inspection_unknown',
        stdout_complete=True, stdout_truncated=False, returncode=0)


def test_typed_error_and_classification_do_not_leak():
    error = completion()
    assert 'secret' not in str(error) + repr(error)
    assert classify_failure(error).kind == 'cleanup_failed'
    assert not classify_failure(error).retryable
    assert _outcome_for(error) == OutcomeClass.CLEANUP_FAILED


@pytest.mark.parametrize('permission', [False, True])
def test_generic_preserves_completion_or_permission_priority(monkeypatch, permission):
    validator = GenericSubprocessValidator({'name':'fixture', 'type':'subprocess', 'command':[sys.executable]})
    error = completion()
    if permission:
        error.stderr = 'jetski: no output produced because a tool required the "write" permission that headless mode cannot prompt for, so it was auto-denied.'
    def fail(*a, **k): raise error
    monkeypatch.setattr(validator, '_run', fail)
    with pytest.raises(ProviderToolPermissionError if permission else ProcessCompletionError):
        validator.generate('fixture')


def test_fanout_recovery_never_counts_as_answer(monkeypatch):
    def fail(*a, **k): raise completion()
    validator = SimpleNamespace(name='fixture', generate=fail)
    results = _fanout_generate(SimpleNamespace(_validators=[validator]), 'fixture', 5)
    assert results[0][1] == ''
    assert isinstance(results[0][2], CompletionDiagnostic)
    assert not _fanout_quorum(results)[0]
    monkeypatch.delenv('LOPE_RECOVER_UNVERIFIED_OUTPUT', raising=False)
    payload = fanout_payload('answer', results)
    assert 'unverified_output' not in payload[0]
    assert payload[0]['completion']['status'] == 'UNVERIFIED'
    monkeypatch.setenv('LOPE_RECOVER_UNVERIFIED_OUTPUT', '1')
    payload = fanout_payload('answer', results)
    assert payload[0]['unverified_output'].startswith('OUTPUT')
    assert 'canary-secret' not in json.dumps(payload)
    assert payload[0]['answer'] == '' and payload[0]['error']
    assert payload[0]['completion']['stdout_complete'] is True
