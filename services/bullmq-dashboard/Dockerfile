FROM node:22-alpine

WORKDIR /app

COPY package.json ./
RUN npm install --omit=dev

COPY --chown=node:node server.js ./

USER node

EXPOSE 3000

CMD ["npm", "start"]