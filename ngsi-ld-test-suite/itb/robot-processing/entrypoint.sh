#!/bin/sh

java -jar /app/app.jar &
APP_PID=$!

echo "Waiting for service to be ready..."
until curl -sf http://localhost:8080/actuator/health; do
  sleep 2
done
echo "Service ready"

echo "Running init..."
curl -X POST http://localhost:8080/api/init \
     -H "Content-Type: application/json" \
     -d '{}'

wait $APP_PID