FROM node:20-alpine AS build

WORKDIR /app

COPY package.json bun.lock ./
RUN npm install -g bun && bun install

COPY . .
RUN bun run build

FROM nginx:1.27-alpine

COPY --from=build /app/dist /usr/share/nginx/html

EXPOSE 80
