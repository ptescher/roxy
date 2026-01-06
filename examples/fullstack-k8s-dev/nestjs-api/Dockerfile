# Build stage
FROM node:22-alpine AS builder

WORKDIR /app

# Copy package files and patches
COPY package*.json ./
COPY patches ./patches/

# Install all dependencies (including devDependencies for build)
# This also runs postinstall which applies patches
RUN npm ci

# Copy source files
COPY tsconfig.json nest-cli.json ./
COPY src ./src/

# Build the application
RUN npm run build

# Prune dev dependencies after build
RUN npm prune --omit=dev

# Production stage
FROM node:22-alpine AS production

WORKDIR /app

# Copy package.json for metadata
COPY package*.json ./

# Copy pruned node_modules from builder (already patched)
COPY --from=builder /app/node_modules ./node_modules

# Copy built application from builder stage
COPY --from=builder /app/dist ./dist

# Run as non-root user
USER node

EXPOSE 3000

CMD ["node", "dist/main"]
