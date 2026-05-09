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
# Next vendors picomatch under dist/compiled; replace it with the patched package
# before producing the standalone tree so the runtime image scan stays clean.
RUN node -e "\
const fs = require('node:fs');\
const path = require('node:path');\
const { createRequire } = require('node:module');\
const webRequire = createRequire(path.join(process.cwd(), 'apps/web/package.json'));\
const pnpmStore = path.join(process.cwd(), 'node_modules/.pnpm');\
const patchedEntry = fs.readdirSync(pnpmStore).find((entry) => entry.startsWith('picomatch@4.0.4'));\
if (!patchedEntry) throw new Error('picomatch@4.0.4 not found in pnpm store');\
const patchedPackageJson = path.join(pnpmStore, patchedEntry, 'node_modules/picomatch/package.json');\
const nextPackageJson = webRequire.resolve('next/dist/compiled/picomatch/package.json');\
const patchedDir = path.dirname(patchedPackageJson);\
const nextDir = path.dirname(nextPackageJson);\
fs.rmSync(nextDir, { recursive: true, force: true });\
fs.cpSync(patchedDir, nextDir, { recursive: true });\
"
RUN pnpm --filter @cts/web build
RUN node -e "\
const fs = require('node:fs');\
const path = require('node:path');\
const pnpmStore = path.join(process.cwd(), 'node_modules/.pnpm');\
const patchedEntry = fs.readdirSync(pnpmStore).find((entry) => entry.startsWith('picomatch@4.0.4'));\
if (!patchedEntry) throw new Error('picomatch@4.0.4 not found in pnpm store');\
const patchedDir = path.join(pnpmStore, patchedEntry, 'node_modules/picomatch');\
const standalone = path.join(process.cwd(), 'apps/web/.next/standalone');\
const targets = [];\
const walk = (dir) => {\
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {\
    const child = path.join(dir, entry.name);\
    if (!entry.isDirectory()) continue;\
    if (child.endsWith(path.join('next', 'dist', 'compiled', 'picomatch'))) {\
      targets.push(child);\
      continue;\
    }\
    walk(child);\
  }\
};\
walk(standalone);\
if (targets.length === 0) throw new Error('standalone picomatch bundle not found');\
for (const target of targets) {\
  fs.rmSync(target, { recursive: true, force: true });\
  fs.cpSync(patchedDir, target, { recursive: true });\
  console.log(target);\
}\
"

FROM node:22-bookworm-slim AS runner
WORKDIR /repo
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1
COPY --from=builder --chown=node:node /repo/apps/web/.next/standalone ./
COPY --from=builder --chown=node:node /repo/apps/web/.next/static ./apps/web/.next/static
EXPOSE 3000
USER node
CMD ["node", "apps/web/server.js"]
