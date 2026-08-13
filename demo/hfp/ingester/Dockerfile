FROM python:3.12-slim
RUN pip install --no-cache-dir paho-mqtt requests
COPY ingester.py /app/ingester.py
CMD ["python", "-u", "/app/ingester.py"]
