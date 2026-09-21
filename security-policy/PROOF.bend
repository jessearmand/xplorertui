# Proofs of LAWS.bend. Every law is a finite case split: each case either
# computes to the claim ({==}) or rests on an assumption that cannot hold.
import Base
import ./policy.bend as P
import ./LAWS.bend as Laws

def exfalso(-Goal: Type, e: Empty) -> Goal:
  match e:

def not_must_merge(d: P.Decision) -> Type:
  match d:
    case P.MustMerge{}:
      Empty
    case P.Held{}:
      Unit
    case P.Blocked{}:
      Unit

# Held and Blocked are not MustMerge.
def held_clash(e: {P.Held{} == P.MustMerge{} : P.Decision}) -> Empty:
  %e : not_must_merge(_)
  Unit{}

def blocked_clash(e: {P.Blocked{} == P.MustMerge{} : P.Decision}) -> Empty:
  %e : not_must_merge(_)
  Unit{}

def Laws.patched_unheld_must_merge(severity):
  {==}

def Laws.patched_held_is_held(severity, hold):
  match hold:
    case P.NoHold{}:
      differs => exfalso({P.MustMerge{} == P.Held{} : P.Decision}, differs({==}))
    case P.Pinned{}:
      differs => {==}
    case P.MajorJump{}:
      differs => {==}

def Laws.unpatched_blocked(severity, hold):
  {==}

def Laws.must_merge_needs_patch(severity, patched, hold):
  match patched:
    case False{}:
      e => exfalso({False{} == True{} : Bool}, blocked_clash(e))
    case True{}:
      e => {==}

def Laws.must_merge_needs_no_hold(severity, patched, hold):
  match patched hold:
    case False{} P.NoHold{}:
      e => {==}
    case False{} P.Pinned{}:
      e => exfalso({P.Pinned{} == P.NoHold{} : P.Hold}, blocked_clash(e))
    case False{} P.MajorJump{}:
      e => exfalso({P.MajorJump{} == P.NoHold{} : P.Hold}, blocked_clash(e))
    case True{} P.NoHold{}:
      e => {==}
    case True{} P.Pinned{}:
      e => exfalso({P.Pinned{} == P.NoHold{} : P.Hold}, held_clash(e))
    case True{} P.MajorJump{}:
      e => exfalso({P.MajorJump{} == P.NoHold{} : P.Hold}, held_clash(e))

def Laws.severity_never_changes_decision(one, other, patched, hold):
  {==}

def Laws.rank_orders_severities():
  {==}
