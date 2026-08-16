# Federation pair

Two brokers; a Context Source Registration on A routes `ParkingSpot`
queries to B. See the federation guide for the model.

```bash
docker compose -f compose.yml up -d
./run.sh
```

`run.sh` creates an entity only B knows, registers B at A, queries A and
asserts B's entity comes back through the registration. The demo network
sets `ANTARES_EGRESS_ALLOW_PRIVATE=true` — never do that facing the
internet (SSRF guard).
