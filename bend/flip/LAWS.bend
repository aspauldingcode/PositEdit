# Additive Flip 3D Tab-hold laws. Owner acceptance required before root merge.
import Base
import ./state.bend as Flip

# LAW: Tab-hold open → flip next → release commits selected and closes.
law release_commits_selected:
  {Flip.release_result(Flip.flip_next(Flip.open_tab(0n), 3n), 0n) == (1n, Flip.Closed{}) : Nat & Flip.Flip}

# LAW: Opening on Tab always starts held — Flip open without Tab held is illegal.
law open_tab_holds_invariant:
  {Flip.hold_invariant(Flip.open_tab(2n)) == True{} : Bool}
