# scorpio-test-mosquitto — the MQTT server the ETSI suite's MqttUtils.resource
# launches per 058_* test (`docker run -e CONF=<file> scorpio-test-mosquitto`).
# Build context = ngsi-ld-test-suite/resources/mqttUtils/mosquitto (the suite's
# own confs + password file stay the single source):
#   docker build -t scorpio-test-mosquitto:latest \
#     -f compose-files/mosquitto/Dockerfile \
#     ngsi-ld-test-suite/resources/mqttUtils/mosquitto
# The pipeline does this automatically (dev/etsi-pipeline.sh).
#
# A second listener on 8085 serves the non-default-port TPs (058_01_02/_05);
# mosquitto's default per_listener_settings=false keeps auth global, so the
# password conf protects BOTH ports.
FROM eclipse-mosquitto:2
COPY . /mosquitto/config/
CMD ["/bin/sh", "-c", "cp /mosquitto/config/${CONF:-mosquitto.conf} /tmp/antares-mqtt.conf && printf '\nlistener 8085\n' >> /tmp/antares-mqtt.conf && exec mosquitto -c /tmp/antares-mqtt.conf"]
