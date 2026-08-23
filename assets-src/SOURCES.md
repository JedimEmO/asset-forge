# Reference sources

Every reference image under `refs/` has a row here — where it came from, on
what terms, and what was made from it. A reference PNG claims integrity (its
sha256, in the lift record beside it) and this row, never regeneration: the
image is an input to the toolkit, not an output of it, and no model here will
paint it twice. The row is where its origin and its licence live, and a PNG
without one is a file nobody can account for — which is why `forge verify`
fails on it, and why a reference PNG is never committed without its row. Add
the row when you add the image; "can we ship this?" has to be answerable from
this file alone, without re-deriving anything or going back to the network.

| File | Origin | For | Date |
|---|---|---|---|
