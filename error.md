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

### 2026-08-05 — forge.etsi.org serves an incomplete TLS chain (infra, not a TP)

`forge.etsi.org` presents ONLY its leaf certificate (`CN=*.etsi.org`, issuer
`GeoTrust TLS RSA CA G1`) with no intermediate in the handshake. Strict chain
builders — rustls/webpki (the broker), OpenSSL (curl), python `ssl` (the
Robot suite itself) — all fail with "unable to get local issuer certificate";
browsers mask it via AIA chasing. Effect: every `@context` fetch from the
forge and every suite-side resolve fails, turning the whole run red with
`LdContextNotAvailable` (first seen as 33/33 CommonBehaviours failures; runs
on 2026-08-04 were green, so the server changed in between).

Not a broker bug and not hacked around: verification stays on. Fix is a
deliberate trust-anchor addition of the PUBLIC DigiCert intermediate
(`dev/ca-extra.pem`): brokers via `ANTARES_EXTRA_CA_FILE` (compose), the
suite + curl via `REQUESTS_CA_BUNDLE`/`CURL_CA_BUNDLE`/`SSL_CERT_FILE`
(pipeline). Remove both wirings when ETSI fixes their server
(`openssl s_client -connect forge.etsi.org:443 | grep -c CERTIFICATE` — more
than one cert = fixed). Intermediate expires 2027-11-02.
