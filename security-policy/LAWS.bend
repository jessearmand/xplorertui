# Dependabot must-merge policy for xplorertui: the rules `P.decide` must obey.
# Humans edit this file; PROOF.bend discharges every law. Gate: `bend PROOF.bend`.
#
# Intent: a fix that exists gets merged unless something concrete holds it.
# Severity never changes the decision; it only sets the order of work.
import Base
import ./policy.bend as P

# --- Rules -------------------------------------------------------------------

# LAW: a patched vulnerability with nothing in the way must be merged, at any severity.
law patched_unheld_must_merge:
  for severity: P.Severity
  {P.decide(P.Alert{severity, True{}, P.NoHold{}}) == P.MustMerge{} : P.Decision}

# LAW: a patched vulnerability that something holds (a pin, a breaking upgrade) is Held.
law patched_held_is_held:
  for severity: P.Severity
  for hold: P.Hold
  {hold != P.NoHold{} : P.Hold} -> {P.decide(P.Alert{severity, True{}, hold}) == P.Held{} : P.Decision}

# LAW: with no patched version there is nothing to merge: Blocked, whatever else is true.
law unpatched_blocked:
  for severity: P.Severity
  for hold: P.Hold
  {P.decide(P.Alert{severity, False{}, hold}) == P.Blocked{} : P.Decision}

# --- Invariants: what no future rule change may break --------------------------

# LAW: MustMerge is only ever said about something that has a patch.
law must_merge_needs_patch:
  for severity: P.Severity
  for patched: Bool
  for hold: P.Hold
  {P.decide(P.Alert{severity, patched, hold}) == P.MustMerge{} : P.Decision} -> {patched == True{} : Bool}

# LAW: MustMerge is only ever said about something nothing holds.
law must_merge_needs_no_hold:
  for severity: P.Severity
  for patched: Bool
  for hold: P.Hold
  {P.decide(P.Alert{severity, patched, hold}) == P.MustMerge{} : P.Decision} -> {hold == P.NoHold{} : P.Hold}

# LAW: severity never changes the decision.
law severity_never_changes_decision:
  for one: P.Severity
  for other: P.Severity
  for patched: Bool
  for hold: P.Hold
  {P.decide(P.Alert{one, patched, hold}) == P.decide(P.Alert{other, patched, hold}) : P.Decision}

# LAW: severity sets the order: critical, high, medium, low, then unknown.
law rank_orders_severities:
  {(Nat.is_lt(P.rank(P.Critical{}), P.rank(P.High{})) && (Nat.is_lt(P.rank(P.High{}), P.rank(P.Medium{})) && (Nat.is_lt(P.rank(P.Medium{}), P.rank(P.Low{})) && Nat.is_lt(P.rank(P.Low{}), P.rank(P.Unknown{}))))) == True{} : Bool}
