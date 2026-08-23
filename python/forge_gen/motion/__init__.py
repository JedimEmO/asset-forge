"""forge_gen.motion — the ARDY lane: ``sweep`` (audition prompts), ``keys`` (keyframe-constrained generation), ``review`` (metrics + sheet).

``session`` holds what the inner halves share: one model load, forward
kinematics, the take and record writers. Nothing in this package imports
torch, numpy or ardy at module level; the outer halves run under the system
python and the inner halves under ``backends/ardy/.env``.
"""
