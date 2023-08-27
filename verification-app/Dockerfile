# Dockerfile

# Stage 1: Build the application
FROM node:18 as build

WORKDIR /app

COPY package.json yarn.lock ./

RUN yarn install

COPY . .

RUN yarn build

# Stage 2: Serve the application using a lightweight Node.js image
FROM node:18-alpine

WORKDIR /app

COPY --from=build /app/build ./build
COPY package.json yarn.lock ./

RUN yarn install --production

EXPOSE 3000

CMD ["yarn", "start"]