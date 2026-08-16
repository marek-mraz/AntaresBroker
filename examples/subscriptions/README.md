# Subscriptions

Two delivery variants for the same subscription shape.

**HTTP callback** (runs anywhere):

```bash
BROKER_URL=http://localhost:9090 ./run.sh
# notification urn:ngsi-ld:Notification:...: urn:ngsi-ld:Door:sub:1 -> state=open
# OK: notification received
```

**MQTT** (needs the mosquitto container):

```bash
docker compose -f mqtt-compose.yml up -d
./mqtt-run.sh          # mosquitto_sub receives the Notification payload
```

The only difference is the endpoint URI: `http(s)://` vs
`mqtt(s)://host:port/topic`. `receiver.py` is a 20-line reference
notification sink you can reuse.
