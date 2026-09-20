# PROOF stub for Dependabot triage laws.
# When Bend is available: `bend PROOF.bend` should check that classify(decision)
# satisfies every law in LAWS.bend for the alert fixture.
#
# Until Bend proof-checking is wired, classify.py is the executable oracle
# and this file documents the intended discharge.

# def Laws.must_merge_severe(a):
#   assert (a.severity in ["critical","high"] and a.patched) => decision(a) == MustMerge

# Offline check today:
#   python3 classify.py fixtures/open-alerts.json
