FROM node:20-alpine AS build
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM build AS verify
RUN npm run typecheck && npm test && npm run build
CMD ["npm", "test"]

FROM nginx:1.27 AS runtime
RUN mkdir -p /etc/nginx/certs
COPY docker-entrypoint.d/10-generate-lan-cert.sh /docker-entrypoint.d/10-generate-lan-cert.sh
COPY docker-entrypoint.d/15-configure-api-auth.sh /docker-entrypoint.d/15-configure-api-auth.sh
RUN sed -i 's/\r$//' /docker-entrypoint.d/10-generate-lan-cert.sh /docker-entrypoint.d/15-configure-api-auth.sh \
    && chmod +x /docker-entrypoint.d/10-generate-lan-cert.sh /docker-entrypoint.d/15-configure-api-auth.sh
COPY nginx.conf /etc/nginx/default.conf.tpl
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80 443
CMD ["nginx", "-g", "daemon off;"]
