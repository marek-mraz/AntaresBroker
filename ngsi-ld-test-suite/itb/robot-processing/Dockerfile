###
# THE APP NEED TO BE BUILD FROM THE REPO ROOT TO USE THE EXISTING REQUIREMENTS.TXT
###

# Step 1 : Build Java project avec Maven + JDK 17
FROM maven:3.9.9-eclipse-temurin-17 AS builder
WORKDIR /app
COPY itb/robot-processing/test-runner-service/ .
RUN mvn clean package -DskipTests

# Step 2 : Final image with Java + Python + Robot Framework
FROM eclipse-temurin:17-jdk

RUN apt-get update && apt-get install -y \
    python3-venv \
    python3-pip \
    git \
    zip \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt /tmp/requirements.txt

RUN python3 -m venv /opt/venv \
    && /opt/venv/bin/pip install -r /tmp/requirements.txt
        
RUN mkdir -p /opt/robot-tests/working-repo /opt/robot-tests/listeners && \
    chown -R 1000:1000 /opt/robot-tests

COPY itb/robot-processing/test-runner-service/src/main/resources/listeners /opt/robot-tests/listeners

ENV PYTHONPATH="/opt/robot-tests"
ENV PATH="/opt/venv/bin:$PATH"

WORKDIR /app

COPY --from=builder /app/target/test-runner-service-*.jar app.jar

COPY itb/robot-processing/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

EXPOSE 8080
ENTRYPOINT ["/entrypoint.sh"]