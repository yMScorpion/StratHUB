FROM python:3.12-slim

RUN apt-get update && apt-get install -y --no-install-recommends git gh ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app/apps/symphony
COPY apps/symphony /app/apps/symphony

RUN pip install --no-cache-dir -e .

EXPOSE 8765
CMD ["symphony", "serve", "--config", "/data/symphony.json"]
