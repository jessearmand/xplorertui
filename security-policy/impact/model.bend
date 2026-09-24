# Blast radius of the starlette 0.52.1 -> 1.7.0 and transformers 5.3.0 -> 5.17.0
# upgrades (which also move tokenizers 0.22.2 -> 0.23.2, safetensors 0.7.0 -> 0.8.0)
# on xplorertui's features. LAWS.bend states the claims; PROOF.bend proves them.
#
# The tables below are facts read from the code (uses, touches) and results of
# running old vs new side by side (evidence). EVIDENCE.md says where each comes from.
import Base

# Everything a user can do. Only the last four ever talk to mlx-server.
type Feature is Data:
  HomeTimeline{}
  Mentions{}
  Bookmarks{}
  Search{}
  UserProfile{}
  Thread{}
  CliJsonl{}
  Auth{}
  HfModelPicker{}
  OpenRouterModels{}
  SemanticRank{}
  ClusterTimeline{}
  ClusterTopics{}
  MlxProbe{}

# mlx-server endpoints reachable from src/mlx/client.rs.
type Endpoint is Data:
  Health{}
  Embeddings{}
  Chat{}
  Multimodal{}

# The packages the two Dependabot bumps move.
type Pkg is Data:
  Starlette{}
  Transformers{}
  Tokenizers{}
  Safetensors{}

# What running old vs new showed for one endpoint and one package.
type Evidence is Data:
  SameBytes{}
  NotRun{}

type Verdict is Data:
  Unaffected{}
  VerifiedSame{}
  NeedsDeviceCheck{}

def is_mlx_feature(f: Feature) -> Bool:
  match f:
    case SemanticRank{}:
      True{}
    case ClusterTimeline{}:
      True{}
    case ClusterTopics{}:
      True{}
    case MlxProbe{}:
      True{}
    case _:
      False{}

# Which endpoints a feature calls (src/app/dispatch.rs, src/app/mod.rs).
# Nothing calls MlxClient::embed_multimodal, so no feature uses Multimodal.
def uses(f: Feature, e: Endpoint) -> Bool:
  match f e:
    case SemanticRank{} Embeddings{}:
      True{}
    case ClusterTimeline{} Health{}:
      True{}
    case ClusterTimeline{} Embeddings{}:
      True{}
    case ClusterTopics{} Chat{}:
      True{}
    case MlxProbe{} Health{}:
      True{}
    case _ _:
      False{}

# Which packages an endpoint's code path imports (mlx-server/server.py and the
# MLX libraries it calls). Health is web layer only. Embeddings load weights with
# mx.load and never import the safetensors package; mlx-vlm (Chat, Multimodal)
# reads the safetensors header with safetensors.safe_open.
def touches(e: Endpoint, p: Pkg) -> Bool:
  match e p:
    case _ Starlette{}:
      True{}
    case Health{} _:
      False{}
    case Embeddings{} Safetensors{}:
      False{}
    case _ _:
      True{}

# Old vs new, run side by side (EVIDENCE.md). Multimodal's model path was not run;
# no feature reaches it.
def evidence(e: Endpoint, p: Pkg) -> Evidence:
  match e p:
    case Multimodal{} Transformers{}:
      NotRun{}
    case Multimodal{} Tokenizers{}:
      NotRun{}
    case _ _:
      SameBytes{}

def is_same(ev: Evidence) -> Bool:
  match ev:
    case SameBytes{}:
      True{}
    case NotRun{}:
      False{}

def hits(f: Feature, p: Pkg, +e: Endpoint) -> Bool:
  uses(f, e) && touches(e, p)

# A feature is exposed to a package when some endpoint it uses imports it.
def exposed(+f: Feature, +p: Pkg) -> Bool:
  hits(f, p, Health{}) || (hits(f, p, Embeddings{}) || (hits(f, p, Chat{}) || hits(f, p, Multimodal{})))

def covered(f: Feature, +p: Pkg, +e: Endpoint) -> Bool:
  Bool.not(hits(f, p, e)) || is_same(evidence(e, p))

# Every exposure is backed by an old-vs-new run that came out identical.
def verified(+f: Feature, +p: Pkg) -> Bool:
  covered(f, p, Health{}) && (covered(f, p, Embeddings{}) && (covered(f, p, Chat{}) && covered(f, p, Multimodal{})))

def judge(is_exposed: Bool, is_verified: Bool) -> Verdict:
  match is_exposed is_verified:
    case False{} _:
      Unaffected{}
    case True{} True{}:
      VerifiedSame{}
    case True{} False{}:
      NeedsDeviceCheck{}

def verdict(+f: Feature, +p: Pkg) -> Verdict:
  judge(exposed(f, p), verified(f, p))
