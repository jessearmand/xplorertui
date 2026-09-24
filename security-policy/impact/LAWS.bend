# What the starlette + transformers upgrades may do to xplorertui, as laws over
# model.bend. PROOF.bend discharges every law. Gate: `bend PROOF.bend`.
#
# The laws are only as true as model.bend's tables. EVIDENCE.md records where each
# row comes from; change a table and the proofs re-check whether the claims survive.
import Base
import ./model.bend as M

# LAW: features that never call mlx-server (timelines, search, profiles, threads,
# bookmarks, the JSONL CLI, auth, the HF and OpenRouter pickers) cannot be affected
# by any of the four packages.
law x_features_unaffected:
  for f: M.Feature
  for p: M.Pkg
  {M.is_mlx_feature(f) == False{} : Bool} -> {M.verdict(f, p) == M.Unaffected{} : M.Verdict}

# LAW: no feature, under any of the four packages, is left with an exposure that an
# old-vs-new run did not cover.
law nothing_needs_device_check:
  for f: M.Feature
  for p: M.Pkg
  {M.verdict(f, p) != M.NeedsDeviceCheck{} : M.Verdict}

# LAW: starlette reaches every mlx feature (it serves every endpoint), and every one
# of those exposures was run old-vs-new with identical responses.
law starlette_verified_for_mlx_features:
  for f: M.Feature
  {M.is_mlx_feature(f) == True{} : Bool} -> {M.verdict(f, M.Starlette{}) == M.VerifiedSame{} : M.Verdict}

# LAW: the startup/`:probe` capability check only hits /health, so the ML packages
# cannot affect it.
law probe_only_sees_starlette:
  for p: M.Pkg
  {p != M.Starlette{} : M.Pkg} -> {M.verdict(M.MlxProbe{}, p) == M.Unaffected{} : M.Verdict}

# LAW: the safetensors bump can only reach a feature through chat (mlx-vlm's header
# read); embeddings load weights with mx.load.
law safetensors_only_through_chat:
  for f: M.Feature
  {M.exposed(f, M.Safetensors{}) == True{} : Bool} -> {M.uses(f, M.Chat{}) == True{} : Bool}

# LAW: no feature calls /v1/embeddings/multimodal ...
law multimodal_unreachable:
  for f: M.Feature
  {M.uses(f, M.Multimodal{}) == False{} : Bool}

# LAW: ... which matters, because its model path was not run: the gap is recorded,
# not assumed away. If a feature starts using it, nothing_needs_device_check breaks.
law multimodal_gap_recorded:
  {M.is_same(M.evidence(M.Multimodal{}, M.Transformers{})) == False{} : Bool}

# LAW: an exposure without identical old-vs-new evidence is flagged, never waved through.
law gaps_are_flagged:
  {M.judge(True{}, False{}) == M.NeedsDeviceCheck{} : M.Verdict}
