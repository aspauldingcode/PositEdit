# DLC4 pure policy models. Not a substitute for host integration tests.
# Differential targets match Rust helpers in crates/ide-spatial.

def clamp_jump(index, count):
  if count == 0:
    return None
  if index < count:
    return Some(index)
  return None

def pick_priority(debugger, failing_diag, has_diff):
  if debugger:
    return 3
  if failing_diag:
    return 2
  if has_diff:
    return 1
  return 0

def next_nest(depth):
  if depth >= 3:
    return 3
  return depth + 1

def rank_score(confidence, evidence_weight):
  weight = evidence_weight
  if weight < 1:
    weight = 1
  return confidence * weight

def validate_snapshot_version(version, workspace_empty):
  if version != 1:
    return False
  if workspace_empty:
    return False
  return True
