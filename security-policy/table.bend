# Prints decide() for every input, one row per line, in the order of
# fixtures/decision-table.json: severity patched hold decision.
import Base
import ./policy.bend as P

def severity_name(severity: P.Severity) -> String:
  match severity:
    case P.Critical{}:
      "critical"
    case P.High{}:
      "high"
    case P.Medium{}:
      "medium"
    case P.Low{}:
      "low"
    case P.Unknown{}:
      "unknown"

def hold_name(hold: P.Hold) -> String:
  match hold:
    case P.NoHold{}:
      "none"
    case P.Pinned{}:
      "pinned"
    case P.Constrained{}:
      "constrained"
    case P.MajorJump{}:
      "major_jump"
    case P.Unverified{}:
      "unverified"

def decision_name(decision: P.Decision) -> String:
  match decision:
    case P.MustMerge{}:
      "MustMerge"
    case P.Held{}:
      "Held"
    case P.Blocked{}:
      "Blocked"

def bool_name(b: Bool) -> String:
  match b:
    case False{}:
      "false"
    case True{}:
      "true"

def row(+severity: P.Severity, +patched: Bool, +hold: P.Hold) -> String:
  severity_name(severity) ++ " " ++ bool_name(patched) ++ " " ++ hold_name(hold) ++ " " ++ decision_name(P.decide(P.Alert{severity, patched, hold})) ++ "\n"

def rows_patched(+severity: P.Severity, +patched: Bool) -> String:
  row(severity, patched, P.NoHold{}) ++ row(severity, patched, P.Pinned{}) ++ row(severity, patched, P.Constrained{}) ++ row(severity, patched, P.MajorJump{}) ++ row(severity, patched, P.Unverified{})

def rows_severity(+severity: P.Severity) -> String:
  rows_patched(severity, True{}) ++ rows_patched(severity, False{})

def table() -> String:
  rows_severity(P.Critical{}) ++ rows_severity(P.High{}) ++ rows_severity(P.Medium{}) ++ rows_severity(P.Low{}) ++ rows_severity(P.Unknown{})

def main() -> IO(Unit):
  do IO<Unit>:
    IO.print(table())
