# The triage rule for Dependabot alerts. LAWS.bend states what it must obey.
# policy.py mirrors it; test_bend_conformance.py checks that the two agree.
import Base

type Severity is Data:
  Critical{}
  High{}
  Medium{}
  Low{}
  Unknown{}

# Why a patched fix cannot simply be merged.
type Hold is Data:
  NoHold{}
  Pinned{}
  MajorJump{}

type Decision is Data:
  MustMerge{}
  Held{}
  Blocked{}

type Alert is Data:
  Alert{severity: Severity, patched: Bool, hold: Hold}

def decide_fields(patched: Bool, hold: Hold) -> Decision:
  match patched hold:
    case False{} _:
      Blocked{}
    case True{} NoHold{}:
      MustMerge{}
    case True{} Pinned{}:
      Held{}
    case True{} MajorJump{}:
      Held{}

def decide(a: Alert) -> Decision:
  match a:
    case Alert{severity, patched, hold}:
      decide_fields(patched, hold)

# Work order: lower ranks are handled first.
def rank(severity: Severity) -> Nat:
  match severity:
    case Critical{}:
      0n
    case High{}:
      1n
    case Medium{}:
      2n
    case Low{}:
      3n
    case Unknown{}:
      4n
