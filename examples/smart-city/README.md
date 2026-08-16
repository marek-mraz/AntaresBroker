# Smart-city dataset

Fifty entities across four Smart Data Models types — `ParkingSpot`,
`Streetlight`, `AirQualityObserved`, `WasteContainer` — scattered around a
city center (Banská Bystrica), with the questions a city dashboard asks:

```bash
BROKER_URL=http://localhost:9090 ./seed.sh
# — free parking spots                  13 results
# — streetlights switched off           4 results
# — air quality: pm25 over 25           4 results
# — waste containers at least 70% full  3 results
# — anything within 500 m of the square 16 results
```

`entities.json` is a plain NGSI-LD batch payload (`entityOperations/
upsert` — re-runnable). Use it as the starting point for a digital-twin
demo: add a subscription per question (see the subscriptions example) and
the answers push themselves.
