# syntax=docker/dockerfile:1.7
FROM node:22-bookworm-slim AS deps
WORKDIR /repo

RUN corepack enable

COPY package.json pnpm-workspace.yaml pnpm-lock.yaml* ./
COPY apps/web/package.json apps/web/
COPY packages/strategy-spec/ts/package.json packages/strategy-spec/ts/
COPY packages/strategy-spec/schema packages/strategy-spec/schema

RUN pnpm install --frozen-lockfile=false

FROM node:22-bookworm-slim AS builder
WORKDIR /repo
RUN corepack enable
COPY --from=deps /repo/node_modules /repo/node_modules
COPY --from=deps /repo/apps/web/node_modules /repo/apps/web/node_modules
COPY --from=deps /repo/packages/strategy-spec/ts/node_modules /repo/packages/strategy-spec/ts/node_modules
COPY . .
RUN pnpm --filter @cts/web build

FROM node:22-bookworm-slim AS runner
WORKDIR /repo/apps/web
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1
RUN corepack enable
COPY --from=builder --chown=node:node /repo /repo
EXPOSE 3000
USER node
CMD ["pnpm", "--filter", "@cts/web", "start"]
