# Pure document revision gate. Additive; does not touch accepted LAWS.bend.
# Models the revision check that precedes every rope mutation in ide-core.
import Base

type Doc is Data:
  Doc{rev: Nat}

type ApplyResult is Data:
  Ok{doc: Doc}
  Stale{doc: Doc}

# Gate on an already-computed equality (Bend cannot match computed values inline).
def decide(+eq: Bool, +rev: Nat) -> ApplyResult:
  match eq:
    case True{}:
      Ok{Doc{Nat.add(rev, 1n)}}
    case False{}:
      Stale{Doc{rev}}

# Successful apply bumps revision by exactly one; stale keeps the document.
def apply(+doc: Doc, +expected: Nat) -> ApplyResult:
  match doc:
    case Doc{+rev}:
      decide(Nat.is_eq(rev, expected), rev)

def revision_of(+result: ApplyResult) -> Nat:
  match result:
    case Ok{doc}:
      match doc:
        case Doc{rev}:
          rev
    case Stale{doc}:
      match doc:
        case Doc{rev}:
          rev
