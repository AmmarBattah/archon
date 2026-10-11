"""Run a test command and pass only if enough tests really passed.

Usage: python3 expect_tests.py KIND MIN_PASSED -- COMMAND [ARGS...]

KIND is `unittest`, `node` (TAP reporter) or `cargo`. An exit status of 0
is not enough: code under test that exits early, a Cargo.toml that stops
compiling the hidden tests, or skipped tests would all exit 0. The runner's
own summary must report no failures and at least MIN_PASSED passing tests.
The command's output is copied to stderr for the grade log.
"""

import re
import subprocess
import sys

UNITTEST_RAN = re.compile(r"^Ran (\d+) tests? in ", re.MULTILINE)
UNITTEST_STATUS = re.compile(r"^(OK|FAILED)(?: \((.*)\))?\s*$", re.MULTILINE)
NODE_COUNT = re.compile(r"^# (tests|pass|fail|cancelled|skipped|todo) (\d+)\s*$", re.MULTILINE)
CARGO_RESULT = re.compile(
    r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", re.MULTILINE
)


def unittest_passed(output):
    # The runner's own summary comes last; tests may print similar lines.
    ran = UNITTEST_RAN.findall(output)
    status = UNITTEST_STATUS.findall(output)
    if not ran or not status:
        return None, "no unittest summary"
    verdict, details = status[-1]
    if verdict != "OK":
        return None, f"unittest reported {verdict} ({details})"
    counts = dict(part.split("=") for part in details.split(", ") if "=" in part)
    not_run = int(counts.get("skipped", 0)) + int(counts.get("expected failures", 0))
    return int(ran[-1]) - not_run, None


def node_passed(output):
    # The last value of each count is the run's summary.
    counts = {name: int(value) for name, value in NODE_COUNT.findall(output)}
    if "pass" not in counts:
        return None, "no TAP summary (was --test-reporter=tap passed?)"
    bad = {k: counts.get(k, 0) for k in ("fail", "cancelled")}
    if any(bad.values()):
        return None, f"node reported {bad}"
    return counts["pass"] - counts.get("todo", 0), None


def cargo_passed(output):
    results = CARGO_RESULT.findall(output)
    if not results:
        return None, "no `test result:` line"
    if any(verdict != "ok" or int(failed) for verdict, _, failed, _ in results):
        return None, "a cargo test binary failed"
    return sum(int(passed) for _, passed, _, _ in results), None


PARSERS = {"unittest": unittest_passed, "node": node_passed, "cargo": cargo_passed}


def main():
    args = sys.argv[1:]
    assert len(args) >= 4 and args[2] == "--", __doc__
    kind, minimum, command = args[0], int(args[1]), args[3:]
    assert kind in PARSERS, f"unknown KIND {kind!r}"
    assert minimum > 0, "MIN_PASSED must be positive"
    run = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    sys.stderr.write(run.stdout)
    if run.returncode != 0:
        print(f"expect_tests: {command[0]} exited {run.returncode}", file=sys.stderr)
        sys.exit(1)
    passed, problem = PARSERS[kind](run.stdout)
    if problem is None and passed < minimum:
        problem = f"only {passed} tests passed, expected at least {minimum}"
    if problem is not None:
        print(f"expect_tests: {problem}", file=sys.stderr)
        sys.exit(1)
    print(f"expect_tests: {passed} {kind} tests passed (minimum {minimum})", file=sys.stderr)


if __name__ == "__main__":
    main()
