# Additive document-revision laws. Owner acceptance required before root merge.
import Base
import ./state.bend as Doc

# LAW: a matching expected revision always yields Ok with rev+1.
law decide_fresh_bumps:
  for rev: Nat
  {Doc.decide(True{}, rev) == Doc.Ok{Doc.Doc{Nat.add(rev, 1n)}} : Doc.ApplyResult}

# LAW: a mismatched expected revision always yields Stale with the same rev.
law decide_stale_preserves:
  for rev: Nat
  {Doc.decide(False{}, rev) == Doc.Stale{Doc.Doc{rev}} : Doc.ApplyResult}

# LAW: apply at revision 0 with expected 0 bumps to 1.
law apply_zero_fresh:
  {Doc.apply(Doc.Doc{0n}, 0n) == Doc.Ok{Doc.Doc{1n}} : Doc.ApplyResult}

# LAW: apply with a stale expected revision does not mutate.
law apply_stale_keeps_rev:
  {Doc.apply(Doc.Doc{3n}, 2n) == Doc.Stale{Doc.Doc{3n}} : Doc.ApplyResult}

# LAW: a successful apply's reported revision is exactly predecessor+1.
law apply_fresh_revision_is_succ:
  {Doc.revision_of(Doc.apply(Doc.Doc{7n}, 7n)) == 8n : Nat}
