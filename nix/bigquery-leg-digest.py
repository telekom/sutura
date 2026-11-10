# Reduces the declared-principal leg's output to what a reader needs when the job log is not
# readable: nextest outcome lines, panic locations and messages, assertion lines, and `observed,`
# lines. Everything else is dropped, and what stays is redacted before it is written.
#
#   bigquery-leg-digest.py <raw-log> <digest>
#
# Resource values come from the environment the leg already runs with and are replaced by a
# variable name (the two table variables) or `<masked>` (project, dataset). The pattern redaction
# after that is a backstop for what no variable names, not a proof: a secret shaped differently
# from the patterns below would pass. A long token needs both a capital and a digit, so a long
# snake_case test name survives. The digest is written once, at the end, so a failure here
# leaves no partial file.
import os
import re
import sys

TABLES = ("SUTURA_BQ_SPLIT_TABLE", "SUTURA_BQ_A_ONLY_TABLE")
MASKED = ("SUTURA_BQ_PROJECT", "SUTURA_BQ_DATASET")
MAX_LINE = 2000

ANSI = re.compile(r"\x1b\[[0-9;]*m")
OUTCOME = re.compile(r"^\s*(PASS|FAIL|SKIP|TIMEOUT|LEAK|FLAKY|SIG[A-Z]+)\b")
ASSERTION = re.compile(r"assertion|^\s*(left|right):")
OBSERVED = re.compile(r"^\s*observed,")
PANICKED = re.compile(r"panicked at")
STREAM_COUNT = re.compile(r"^the split table answered [0-9]+ streams$")
SECRET_SHAPES = (
    re.compile(r"\S*@\S*"),
    re.compile(r"\S*projects/\S*"),
    re.compile(r"eyJ[\w-]*(\.[\w-]+)*"),
    re.compile(
        r"(?=[A-Za-z0-9_+/=-]*[A-Z])(?=[A-Za-z0-9_+/=-]*[0-9])[A-Za-z0-9_+/=-]{32,}"
    ),
    re.compile(r"[0-9a-f]{32,}"),
)


def replacements():
    pairs = [(os.environ.get(name, ""), name) for name in TABLES]
    pairs += [(os.environ.get(name, ""), "<masked>") for name in MASKED]
    return sorted((p for p in pairs if p[0]), key=lambda p: -len(p[0]))


def redact(line, pairs):
    for value, label in pairs:
        line = line.replace(value, label)
    for shape in SECRET_SHAPES:
        line = shape.sub("<redacted>", line)
    return line[:MAX_LINE]


def digest(lines, pairs):
    keep = False
    for raw in lines:
        line = ANSI.sub("", raw.rstrip("\n"))
        if keep:
            keep = False
            yield redact(line, pairs)
            continue
        if PANICKED.search(line):
            keep = True
            yield redact(line, pairs)
        elif (
            OUTCOME.match(line)
            or ASSERTION.search(line)
            or OBSERVED.match(line)
            or STREAM_COUNT.fullmatch(line)
        ):
            yield redact(line, pairs)


def main(raw_path, digest_path):
    pairs = replacements()
    with open(raw_path, encoding="utf-8", errors="replace") as raw:
        kept = list(digest(raw, pairs))
    with open(digest_path, "w", encoding="utf-8") as out:
        out.write("\n".join(kept) + ("\n" if kept else ""))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
