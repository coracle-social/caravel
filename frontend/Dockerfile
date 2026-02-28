FROM node:20-slim

RUN npm install -g bun

WORKDIR /app

COPY package.json bun.lock ./
RUN bun install

COPY . .
RUN bun run build

EXPOSE 3000

CMD ["npx", "serve", "dist", "-s"]
