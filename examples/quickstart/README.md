# Quickstart

The smallest possible Antares deployment: one broker, no infrastructure.

```bash
docker compose -f compose.yml up -d     # or: cargo run -p antares-broker
./seed.sh                               # BROKER_URL=... to point elsewhere
```

`seed.sh` batch-creates three `TemperatureSensor` entities (with
locations) and demonstrates the three query styles: by type, by value
(`q=temperature>30` returns only the hot sensor), and geographic
(`georel=near;maxDistance==2000` returns the two sensors within 2 km).
