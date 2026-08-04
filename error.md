# ETSI test-suite tool bugs (never hack the broker around these)

Log of defects in the ETSI Robot suite itself, per the testing guide: prove
the suite contradicts the spec, record it here, leave the broker correct.
Each entry: TP id, what the suite does, why it is wrong (spec clause), status.

## Known (inherited from the Scorpio campaign in this workspace)

### IOP QueryEntities 04_01 / 04_02 — duplicate-id setup → self-inflicted 409

The suite's own setup creates two payloads with the same entity id on one
broker; the second create correctly gets 409 AlreadyExists (CIM 009 5.6.1),
which the test then reports as a failure. Broker behaviour is spec-correct.
Status: open upstream; excluded from gating conclusions. (See memory
`multi-broker-fed-stack.md`.)

## New entries

*(none yet for Antares — every failure so far was a real broker bug and was
fixed broker-side)*
